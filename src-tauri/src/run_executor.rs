//! Sequential run-queue executor — `prd-run-queue.md` Phase 2.
//!
//! Turns a persisted [`crate::runplan::RunPlan`] into one orchestrated
//! `claude_session` turn per queued phase, advancing only on a green verify
//! verdict. This module holds the pieces that don't need `AppState`
//! (verdict parsing, prompt building, startup reconciliation); the actual
//! session-driving loop lives in `commands::run_queue`, alongside the rest of
//! this codebase's session orchestration (`commands::agent`), since it needs
//! `AppState` to spawn turns through the existing `claude_session` pipeline.

use crate::epic::LoopLocation;
use crate::error::AppError;
use crate::runplan::{self, PinnedAnswer, RunPhaseStatus};
use std::path::Path;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

/// Per-project handle for a live queued run. See `AppState::run_handles`.
pub struct RunHandle {
    /// Checked by the executor loop between phases (and passed through to
    /// `cancel_run`, which also fires the project's interrupt slot so an
    /// in-flight turn stops immediately rather than waiting for the phase to
    /// finish on its own).
    pub cancel: Arc<AtomicBool>,
}

impl RunHandle {
    pub fn new() -> Self {
        Self {
            cancel: Arc::new(AtomicBool::new(false)),
        }
    }
}

impl Default for RunHandle {
    fn default() -> Self {
        Self::new()
    }
}

/// Roll-up verdict from `loopdeck-prd-verifier`'s report, per its documented
/// greppable line: `**Verdict:** PASS | WARN | BLOCK`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RunVerdict {
    Pass,
    Warn,
    Block,
}

/// Extract the roll-up verdict from a turn's final response text.
///
/// Scans for the **last** `**Verdict:**` occurrence (a turn's own reasoning
/// may quote the marker while explaining the convention; the final one is the
/// actual roll-up) and reads the token that follows. Returns `None` when no
/// marker is present or the token after it isn't one of the three known
/// verdicts — both treated identically by the executor (non-green, phase
/// does not advance).
pub(crate) fn extract_verdict(text: &str) -> Option<RunVerdict> {
    const MARKER: &str = "**Verdict:**";
    let idx = text.rfind(MARKER)?;
    let rest = text[idx + MARKER.len()..]
        .trim_start()
        .trim_start_matches('*');
    if rest.starts_with("PASS") {
        Some(RunVerdict::Pass)
    } else if rest.starts_with("WARN") {
        Some(RunVerdict::Warn)
    } else if rest.starts_with("BLOCK") {
        Some(RunVerdict::Block)
    } else {
        None
    }
}

/// Build the prompt for one queued phase's orchestrated turn.
///
/// Mirrors `commands::agent::build_next_loop_prompt`'s shape (same
/// `loopdeck-orchestrator` framing a human-initiated "Start Loop" uses) but
/// targets a specific phase by stable execution ID instead of the first
/// unchecked `loops.md` step, and injects the pre-flight interview's pinned
/// answers so the turn never re-asks a question the human already answered
/// while present.
pub(crate) fn build_phase_prompt(
    execution_id: &str,
    loc: &LoopLocation,
    interview: &[PinnedAnswer],
) -> String {
    let mut prompt = format!(
        "You are working on this LoopDeck project as part of an unattended \
         overnight run. Use the `loopdeck-orchestrator` skill conventions. \
         Run loop `{execution_id}` — \"{title}\" (epic `{epic}`, PRD `{prd}`, \
         phase `{phase}`). Implement it per the PRD phase's acceptance \
         criteria.",
        title = loc.title,
        epic = loc.epic,
        prd = loc.prd,
        phase = loc.phase,
    );

    if !interview.is_empty() {
        prompt.push_str(
            "\n\nThe following clarifying questions were already answered by the \
             user before this run started — do not ask them again, use these \
             answers:\n",
        );
        for answer in interview {
            prompt.push_str(&format!(
                "- Q: {}\n  A: {}\n",
                answer.question, answer.answer
            ));
        }
    }

    prompt.push_str(
        "\n\nWhen done, update `.loopdeck/loops.md` (mark the step `[x]`, refresh \
         `## Current`) and append any architectural decisions to \
         `.loopdeck/decisions.md` per the memory convention. Run the full \
         verify→ship flow (Phases 6-7) — no human is present to gate a PR \
         open, so stop after Phase 6's `**Verdict:**` line unless the plan's \
         consent explicitly authorizes a draft PR.",
    );

    prompt
}

/// Build the prompt for one queued phase's pre-flight interview turn
/// (Phase 3) — a single bounded session, run while the user is present,
/// whose entire job is to surface ambiguity in that one phase's acceptance
/// criteria via `AskUserQuestion` before the phase runs unattended tonight.
///
/// Mirrors the orchestrator's Phase 1 "Ask Clarifying Questions" step, but
/// scoped to one phase instead of a whole PRD, and closed off with a
/// greppable summary block (mirroring `extract_verdict`'s marker convention)
/// so [`extract_interview_answers`] can pin the answers without re-deriving
/// them from raw tool-call history.
pub(crate) fn build_interview_prompt(execution_id: &str, loc: &LoopLocation) -> String {
    format!(
        "You are preparing loop `{execution_id}` — \"{title}\" (epic `{epic}`, \
         PRD `{prd}`, phase `{phase}`) to run **unattended, overnight** — no \
         human will be present once the run starts. Read the phase's \
         acceptance criteria in its PRD and identify anything genuinely \
         ambiguous that would make you guess mid-run. Ask the user via \
         `AskUserQuestion` now, while they're present, and wait for their \
         answers. If nothing is ambiguous, ask no questions.\n\n\
         Do not implement anything — this turn only clarifies. When you are \
         done (whether or not you asked anything), end your final message \
         with exactly this block, restating each question you asked and the \
         user's answer verbatim, or `(none)` if you asked nothing:\n\n\
         ## Pre-flight Answers\n\
         - Q: <question text>\n\
         \x20 A: <answer text>\n",
        title = loc.title,
        epic = loc.epic,
        prd = loc.prd,
        phase = loc.phase,
    )
}

/// Parse the `## Pre-flight Answers` block [`build_interview_prompt`] asks
/// for out of a completed interview turn's final response text.
///
/// Looks for the **last** `## Pre-flight Answers` occurrence (same
/// last-one-wins rationale as `extract_verdict` — the turn's own reasoning
/// may quote the format while explaining it) and reads `- Q: … / A: …` pairs
/// from the lines that follow, stopping at the next heading or end of text.
/// A malformed or missing block yields an empty list rather than an error —
/// callers treat "no answers" identically to "nothing was ambiguous."
pub(crate) fn extract_interview_answers(text: &str) -> Vec<PinnedAnswer> {
    const MARKER: &str = "## Pre-flight Answers";
    let Some(idx) = text.rfind(MARKER) else {
        return Vec::new();
    };
    let body = &text[idx + MARKER.len()..];

    let mut answers = Vec::new();
    let mut pending_question: Option<String> = None;
    for line in body.lines() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix("- Q:") {
            pending_question = Some(rest.trim().to_string());
        } else if let Some(rest) = trimmed.strip_prefix("A:") {
            if let Some(question) = pending_question.take() {
                answers.push(PinnedAnswer {
                    question,
                    answer: rest.trim().to_string(),
                });
            }
        } else if trimmed.starts_with('#') {
            break;
        }
    }
    answers
}

/// Startup reconciliation, mirroring `conversation::reconcile_interrupted`:
/// a `Running` phase left on disk from a killed/crashed process is not
/// actually running on a fresh process — nothing has resumed it yet — so
/// downgrade it to `Interrupted` (P1: "resumable across app restarts").
/// `queue_run` also calls this before deciding whether a run is already
/// active, so a stale `Running` phase from a prior crash can't block a new
/// queue attempt forever.
///
/// Returns `true` if the plan was rewritten. A missing plan is a no-op.
pub fn reconcile_running_phases(repo_path: &Path) -> Result<bool, AppError> {
    let Some(mut plan) = runplan::load(repo_path)? else {
        return Ok(false);
    };
    let mut changed = false;
    for phase in &mut plan.phases {
        if phase.status == RunPhaseStatus::Running {
            phase.status = RunPhaseStatus::Interrupted;
            changed = true;
        }
    }
    if changed {
        runplan::save(repo_path, &plan)?;
    }
    Ok(changed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_verdict_reads_pass() {
        let text = "some report\n\n**Verdict:** PASS\n\nall good";
        assert_eq!(extract_verdict(text), Some(RunVerdict::Pass));
    }

    #[test]
    fn extract_verdict_reads_warn() {
        assert_eq!(
            extract_verdict("**Verdict:** WARN — one partial"),
            Some(RunVerdict::Warn)
        );
    }

    #[test]
    fn extract_verdict_reads_block() {
        assert_eq!(
            extract_verdict("**Verdict:** BLOCK — one failure"),
            Some(RunVerdict::Block)
        );
    }

    #[test]
    fn extract_verdict_uses_last_occurrence() {
        // The verifier skill's own explanatory text may quote the marker
        // convention before the real roll-up line — the real one is last.
        let text = "The format is `**Verdict:** PASS | WARN | BLOCK`.\n\n\
                     ## Report\n\n**Verdict:** BLOCK";
        assert_eq!(extract_verdict(text), Some(RunVerdict::Block));
    }

    #[test]
    fn extract_verdict_none_when_absent() {
        assert_eq!(extract_verdict("no verdict line here"), None);
    }

    #[test]
    fn extract_verdict_none_on_unknown_token() {
        assert_eq!(extract_verdict("**Verdict:** MAYBE"), None);
    }

    #[test]
    fn build_phase_prompt_includes_pinned_answers() {
        let loc = LoopLocation {
            epic: "overnight-orchestration".into(),
            prd: "prd-run-queue".into(),
            phase: "Phase 2".into(),
            title: "Queue executor".into(),
        };
        let interview = vec![PinnedAnswer {
            question: "Which stall policy?".into(),
            answer: "halt".into(),
        }];
        let prompt = build_phase_prompt("prd-run-queue/phase-2", &loc, &interview);
        assert!(prompt.contains("prd-run-queue/phase-2"));
        assert!(prompt.contains("Queue executor"));
        assert!(prompt.contains("Which stall policy?"));
        assert!(prompt.contains("halt"));
    }

    #[test]
    fn build_phase_prompt_omits_interview_section_when_empty() {
        let loc = LoopLocation {
            epic: "e".into(),
            prd: "p".into(),
            phase: "ph".into(),
            title: "t".into(),
        };
        let prompt = build_phase_prompt("e/p-1", &loc, &[]);
        assert!(!prompt.contains("already answered"));
    }

    #[test]
    fn build_interview_prompt_names_the_phase_and_the_marker() {
        let loc = LoopLocation {
            epic: "overnight-orchestration".into(),
            prd: "prd-run-queue".into(),
            phase: "Phase 3".into(),
            title: "Pre-flight interview".into(),
        };
        let prompt = build_interview_prompt("prd-run-queue/phase-3", &loc);
        assert!(prompt.contains("prd-run-queue/phase-3"));
        assert!(prompt.contains("Pre-flight interview"));
        assert!(prompt.contains("AskUserQuestion"));
        assert!(prompt.contains("## Pre-flight Answers"));
    }

    #[test]
    fn extract_interview_answers_parses_one_pair() {
        let text = "I asked one question.\n\n## Pre-flight Answers\n\
                     - Q: Which port should the server bind?\n  A: 8080\n";
        let answers = extract_interview_answers(text);
        assert_eq!(answers.len(), 1);
        assert_eq!(answers[0].question, "Which port should the server bind?");
        assert_eq!(answers[0].answer, "8080");
    }

    #[test]
    fn extract_interview_answers_parses_multiple_pairs() {
        let text = "## Pre-flight Answers\n\
                     - Q: Q1?\n  A: A1\n\
                     - Q: Q2?\n  A: A2\n";
        let answers = extract_interview_answers(text);
        assert_eq!(answers.len(), 2);
        assert_eq!(answers[1].question, "Q2?");
        assert_eq!(answers[1].answer, "A2");
    }

    #[test]
    fn extract_interview_answers_empty_when_none_asked() {
        let text = "## Pre-flight Answers\n(none)\n";
        assert!(extract_interview_answers(text).is_empty());
    }

    #[test]
    fn extract_interview_answers_empty_when_marker_absent() {
        assert!(extract_interview_answers("no marker here").is_empty());
    }

    #[test]
    fn extract_interview_answers_uses_last_occurrence() {
        // The turn's own reasoning may quote the format while explaining it —
        // the real block is last, same convention as `extract_verdict`.
        let text = "The format is `## Pre-flight Answers` then `- Q:` / `A:` \
                     pairs.\n\n## Pre-flight Answers\n- Q: Real question?\n  A: Real answer\n";
        let answers = extract_interview_answers(text);
        assert_eq!(answers.len(), 1);
        assert_eq!(answers[0].question, "Real question?");
        assert_eq!(answers[0].answer, "Real answer");
    }

    #[test]
    fn reconcile_running_phases_downgrades_running_to_interrupted() {
        use crate::runplan::{RunPhase, RunPlan};
        use chrono::{TimeZone, Utc};
        use std::path::PathBuf;

        let dir = std::env::temp_dir().join(format!("loopdeck-runexec-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();

        let plan = RunPlan {
            id: "run-1".to_string(),
            project: PathBuf::from("/repo"),
            created: Utc.with_ymd_and_hms(2026, 7, 28, 9, 0, 0).unwrap(),
            consent: Default::default(),
            budgets: Default::default(),
            stall_policy: Default::default(),
            phases: vec![
                RunPhase {
                    execution_id: "p/1".into(),
                    status: RunPhaseStatus::Running,
                    interview: vec![],
                    interview_status: Default::default(),
                    depends_on: vec![],
                    park_payload: None,
                },
                RunPhase {
                    execution_id: "p/2".into(),
                    status: RunPhaseStatus::Queued,
                    interview: vec![],
                    interview_status: Default::default(),
                    depends_on: vec![],
                    park_payload: None,
                },
            ],
        };
        runplan::save(&dir, &plan).unwrap();

        let changed = reconcile_running_phases(&dir).unwrap();
        assert!(changed);

        let reloaded = runplan::load(&dir).unwrap().unwrap();
        assert_eq!(reloaded.phases[0].status, RunPhaseStatus::Interrupted);
        assert_eq!(reloaded.phases[1].status, RunPhaseStatus::Queued);

        // Idempotent: a second pass is a no-op.
        assert!(!reconcile_running_phases(&dir).unwrap());

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn reconcile_running_phases_noop_when_no_plan() {
        let dir = std::env::temp_dir().join(format!("loopdeck-runexec-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        assert!(!reconcile_running_phases(&dir).unwrap());
        std::fs::remove_dir_all(&dir).ok();
    }
}
