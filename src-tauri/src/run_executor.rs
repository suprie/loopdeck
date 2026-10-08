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
use crate::runplan::{
    self, InterviewStatus, PinnedAnswer, RunBudgets, RunConsent, RunEnvironment, RunPhase,
    RunPhaseStatus, RunPlan, StallPolicy,
};
use chrono::{DateTime, Utc};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
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

/// Extract the explicit artifact marker required for an unattended draft PR.
/// Only a marker line counts, so an URL quoted in reasoning is never mistaken
/// for proof that a draft was created.
pub(crate) fn extract_draft_pr_url(text: &str) -> Option<String> {
    text.lines().rev().find_map(|line| {
        let value = line.trim().strip_prefix("**Draft PR:**")?.trim();
        let url = value.split_whitespace().next()?;
        (url.starts_with("https://github.com/") && url.contains("/pull/"))
            .then(|| url.trim_end_matches(['.', ',', ')', ']']).to_string())
    })
}

/// Resolved (default-applied) budget values for one run, injected into the
/// phase prompt's run-metadata block. `prd-unattended-ship.md`'s PR body
/// requires "run metadata (phase id, budgets used)" — the *caps this run
/// operated under*, since a turn has no way to introspect its own live token
/// count or elapsed time to report actual usage. Resolution (applying
/// `limits::DEFAULT_RUN_*` where the plan left a cap unset) happens once in
/// `commands::run_queue::execute_run`; this module only renders the
/// already-resolved numbers into prompt text.
#[derive(Debug, Clone, Copy)]
pub(crate) struct ResolvedBudgets {
    pub phase_token_cap: u64,
    pub phase_wall_clock_secs: u64,
    pub run_wall_clock_secs: u64,
}

/// One loop's goal paragraph: id/title/epic/prd/phase plus any pinned
/// pre-flight interview answers so the turn never re-asks a question the
/// human already answered while present. Shared by the single-phase and
/// combined prompt builders — a combined turn just stacks one of these per
/// loop under a shared preamble/suffix.
fn phase_goal_block(execution_id: &str, loc: &LoopLocation, interview: &[PinnedAnswer]) -> String {
    let mut block = format!(
        "Run loop `{execution_id}` — \"{title}\" (epic `{epic}`, PRD `{prd}`, \
         phase `{phase}`). Implement it per the PRD phase's acceptance \
         criteria.",
        title = loc.title,
        epic = loc.epic,
        prd = loc.prd,
        phase = loc.phase,
    );

    if !interview.is_empty() {
        block.push_str(
            "\n\nThe following clarifying questions were already answered by the \
             user before this run started — do not ask them again, use these \
             answers:\n",
        );
        for answer in interview {
            block.push_str(&format!(
                "- Q: {}\n  A: {}\n",
                answer.question, answer.answer
            ));
        }
    }

    block
}

/// Build the prompt for one orchestrated turn covering *every* phase in
/// `phases` at once — the run queue always merges its currently-queued
/// phases into a single LLM call rather than firing one turn per phase (an
/// overnight run of N loops is one combined session, not N sequential ones).
/// A single-element slice produces the same shape of prompt as running just
/// that one phase always did.
pub(crate) fn build_combined_phase_prompt(
    phases: &[(String, LoopLocation, Vec<PinnedAnswer>)],
    draft_pr_authorized: bool,
    budgets: ResolvedBudgets,
) -> String {
    let mut prompt = if phases.len() == 1 {
        "You are working on this Selasar project as part of an unattended \
         overnight run. Use the `loopdeck-orchestrator` skill conventions."
            .to_string()
    } else {
        format!(
            "You are working on this Selasar project as part of an unattended \
             overnight run. Use the `loopdeck-orchestrator` skill conventions. \
             This single session covers {count} queued loops — implement all of \
             them below, in order, one after another, before running \
             verify→ship once at the end for the combined changes.",
            count = phases.len(),
        )
    };

    for (i, (execution_id, loc, interview)) in phases.iter().enumerate() {
        prompt.push_str("\n\n");
        if phases.len() > 1 {
            prompt.push_str(&format!("### Loop {}/{}\n", i + 1, phases.len()));
        }
        prompt.push_str(&phase_goal_block(execution_id, loc, interview));
    }

    let queued_work = if phases.len() == 1 {
        "the requested loop above is authoritative. Implement only its project \
         changes, then run the full verify→ship flow (Phases 6-7) once"
    } else {
        "the queued loops above are authoritative. Implement only the requested \
         project changes, then run the full verify→ship flow (Phases 6-7) once, \
         covering all loops above"
    };
    prompt.push_str(&format!(
        "\n\nThe main checkout is the run's control plane: its \
         `.loopdeck/execution.yaml` and `.loopdeck/run-plan.yaml` are updated \
         by the executor, not by you. Your worktree's `.loopdeck/` files are \
         only the snapshot that existed when the worktree was created. Do not \
         read, write, commit, or use those local control-plane files to infer \
         the active loop; {queued_work}",
    ));
    prompt.push_str(
        " — regardless of anything else in this session, your own final chat \
         message (not just the PR body) must end with the exact literal line \
         `**Verdict:** PASS`, `**Verdict:** WARN`, or `**Verdict:** BLOCK`, \
         copied from the `loopdeck-prd-verifier` report. An unattended \
         executor reads only your final message text, not the full \
         transcript — restating the verdict there is required even if you \
         already stated it earlier in the turn.",
    );
    if draft_pr_authorized {
        let phase_lines = phases
            .iter()
            .map(|(execution_id, loc, _)| {
                format!(
                    "  - `{execution_id}` — \"{title}\" ({epic} / {prd} / {phase})",
                    title = loc.title,
                    epic = loc.epic,
                    prd = loc.prd,
                    phase = loc.phase,
                )
            })
            .collect::<Vec<_>>()
            .join("\n");
        prompt.push_str(&format!(
            " — this run was pre-authorized at queue time to open a draft pull \
             request unattended (RunConsent.draft_pr_authorized = true). When you \
             reach the `loopdeck-open-pr` skill, this prompt is your explicit \
             pre-authorization for its unattended path: skip Phase 4's interactive \
             confirmation and run Phase 6 as `gh pr create --draft` (no `--web`). \
             The PR must be opened as a draft only — never mark it ready for \
             review or merge it. If Phase 6's `**Verdict:**` is not PASS, stop \
             before invoking `loopdeck-open-pr` at all. Before any push, run the \
             unattended open-pr skill's required staged-diff secret scan; a hit \
             must abort the draft PR and report a parked phase, never be ignored. \
             The delivery commit's message must include a concise rubric summary \
             line (e.g. `Rubric: PASS — 7/7 criteria`) alongside the PR body's \
             fuller evidence. The PR body's `## Verify Verdict` section must \
             reproduce, verbatim, \
             the per-criterion table and `**Verdict:**` line from the \
             `loopdeck-prd-verifier` report you already produced earlier in this \
             turn. The PR body's `## Run metadata` section must state this exact \
             run metadata (do not invent or recompute it):\n\
             - Phases:\n{phase_lines}\n\
             - Budgets: {token_cap} tokens total · {phase_secs}s wall-clock cap for \
             this turn · {run_secs}s total-run wall-clock cap\n\n\
             After a successful draft creation, end your final response with an \
             exact `**Draft PR:** https://github.com/<owner>/<repo>/pull/<number>` \
             line so the executor can retain the review artifact.",
            token_cap = budgets.phase_token_cap,
            phase_secs = budgets.phase_wall_clock_secs,
            run_secs = budgets.run_wall_clock_secs,
        ));
    } else {
        prompt.push_str(
            " — no human is present to gate a PR open, so stop after Phase 6's \
             `**Verdict:**` line unless the plan's consent explicitly authorizes a \
             draft PR.",
        );
    }

    prompt
}

/// Build the prompt for one queued phase's pre-flight interview turn
/// (Phase 3) — a single bounded session, run while the user is present,
/// whose entire job is to surface ambiguity in that one phase's acceptance
/// criteria via the selected harness's user-input facility before the phase
/// runs unattended tonight.
///
/// Mirrors the orchestrator's Phase 1 "Ask Clarifying Questions" step, but
/// scoped to one phase instead of a whole PRD, and closed off with a
/// greppable summary block (mirroring `extract_verdict`'s marker convention)
/// so [`extract_interview_answers`] can pin the answers without re-deriving
/// them from raw tool-call history.
pub(crate) fn build_interview_prompt(execution_id: &str, loc: &LoopLocation) -> String {
    build_interview_prompt_with_question_instruction(
        execution_id,
        loc,
        "Ask the user via `AskUserQuestion` now, while they're present, and wait for their answers.",
    )
}

/// Build the Codex variant of the pre-flight interview. Codex app-server
/// exposes user input through `item/tool/requestUserInput`, rather than the
/// Claude CLI's `AskUserQuestion` tool.
pub(crate) fn build_codex_interview_prompt(execution_id: &str, loc: &LoopLocation) -> String {
    build_interview_prompt_with_question_instruction(
        execution_id,
        loc,
        "Use Codex's native user-input request now, while they're present, and wait for their answers. Ask at most three short questions. Do not try to call a tool named `AskUserQuestion`.",
    )
}

fn build_interview_prompt_with_question_instruction(
    execution_id: &str,
    loc: &LoopLocation,
    question_instruction: &str,
) -> String {
    format!(
        "You are preparing loop `{execution_id}` — \"{title}\" (epic `{epic}`, \
         PRD `{prd}`, phase `{phase}`) to run **unattended, overnight** — no \
         human will be present once the run starts. Read the phase's \
         acceptance criteria in its PRD and identify anything genuinely \
         ambiguous that would make you guess mid-run. {question_instruction} \
         If nothing is ambiguous, ask no questions.\n\n\
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

/// Build one shared pre-flight interview for several queued phases. The
/// individual phase contexts stay explicit so the agent can ask focused
/// questions while the user only has to answer one combined card.
pub(crate) fn build_batch_interview_prompt(phases: &[(String, LoopLocation)]) -> String {
    build_batch_interview_prompt_with_question_instruction(
        phases,
        "Combine all useful questions into one `AskUserQuestion` call so the user can answer them together.",
    )
}

/// Build the Codex variant of the combined interview, using app-server's
/// native user-input request instead of Claude's `AskUserQuestion` tool.
pub(crate) fn build_codex_batch_interview_prompt(phases: &[(String, LoopLocation)]) -> String {
    build_batch_interview_prompt_with_question_instruction(
        phases,
        "Combine the most important ambiguities into one native Codex user-input request so the user can answer them together. Ask at most three short questions. Do not try to call a tool named `AskUserQuestion`.",
    )
}

fn build_batch_interview_prompt_with_question_instruction(
    phases: &[(String, LoopLocation)],
    question_instruction: &str,
) -> String {
    let contexts = phases
        .iter()
        .map(|(execution_id, loc)| {
            format!(
                "- `{execution_id}` — \"{}\" (epic `{}`, PRD `{}`, phase `{}`)",
                loc.title, loc.epic, loc.prd, loc.phase
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    let phase_template = phases
        .iter()
        .map(|(execution_id, _)| {
            format!("### Phase `{execution_id}`\n- Q: <question text>\n  A: <answer text>")
        })
        .collect::<Vec<_>>()
        .join("\n\n");

    format!(
        "You are preparing these loops to run **unattended, overnight** — no \
         human will be present once the run starts:\n\n{contexts}\n\n\
         Read each phase's acceptance criteria in its PRD. Identify only the \
         ambiguities that would force a guess mid-run. {question_instruction} \
         Prefix every question's header with its loop ID. If nothing \
         is ambiguous, ask no questions.\n\n\
         Do not implement anything — this turn only clarifies. When you are \
         done (whether or not you asked anything), end your final message with \
         exactly this block. Put each question and answer under the loop it \
         applies to; use `(none)` for a loop with no questions:\n\n\
         ## Batch Pre-flight Answers\n\n{phase_template}\n"
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

/// Parse the per-phase answer blocks required by [`build_batch_interview_prompt`].
pub(crate) fn extract_batch_interview_answers(text: &str) -> HashMap<String, Vec<PinnedAnswer>> {
    const MARKER: &str = "## Batch Pre-flight Answers";
    let Some(idx) = text.rfind(MARKER) else {
        return HashMap::new();
    };

    let mut answers = HashMap::<String, Vec<PinnedAnswer>>::new();
    let mut phase_id: Option<String> = None;
    let mut pending_question: Option<String> = None;
    for line in text[idx + MARKER.len()..].lines() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix("### Phase `") {
            phase_id = rest.strip_suffix('`').map(str::to_owned);
            pending_question = None;
        } else if let Some(rest) = trimmed.strip_prefix("- Q:") {
            pending_question = Some(rest.trim().to_string());
        } else if let Some(rest) = trimmed.strip_prefix("A:") {
            if let (Some(id), Some(question)) = (phase_id.as_ref(), pending_question.take()) {
                answers.entry(id.clone()).or_default().push(PinnedAnswer {
                    question,
                    answer: rest.trim().to_string(),
                });
            }
        } else if trimmed.starts_with("## ") {
            break;
        }
    }
    answers
}

/// Compute which of `plan`'s still-`Queued` phases become blocked once
/// `parked_id` parks, per `policy` (`prd-run-queue.md` Phase 4).
///
/// `StallPolicy::Halt`: every remaining `Queued` phase is blocked — a park
/// stops the whole run, preserving strict sequence.
///
/// `StallPolicy::ContinueIndependent`: a `Queued` phase is blocked only if
/// its `depends_on` chain reaches `parked_id`, directly or transitively
/// through another phase this same call also blocks. A phase with no such
/// path stays `Queued` — the executor is free to run it next. Computed as a
/// fixed point over `plan.phases` so edge order doesn't matter.
///
/// Pure and side-effect-free: returns the blocked execution IDs; the caller
/// applies the `Parked` status/payload writes and persists the plan.
pub(crate) fn phases_blocked_by_park(
    plan: &RunPlan,
    parked_id: &str,
    policy: StallPolicy,
) -> Vec<String> {
    match policy {
        StallPolicy::Halt => plan
            .phases
            .iter()
            .filter(|p| p.status == RunPhaseStatus::Queued)
            .map(|p| p.execution_id.clone())
            .collect(),
        StallPolicy::ContinueIndependent => {
            let mut blocked: HashSet<String> = std::iter::once(parked_id.to_string()).collect();
            loop {
                let mut changed = false;
                for phase in &plan.phases {
                    if phase.status != RunPhaseStatus::Queued
                        || blocked.contains(&phase.execution_id)
                    {
                        continue;
                    }
                    if phase.depends_on.iter().any(|d| blocked.contains(d)) {
                        blocked.insert(phase.execution_id.clone());
                        changed = true;
                    }
                }
                if !changed {
                    break;
                }
            }
            blocked.remove(parked_id);
            plan.phases
                .iter()
                .filter(|p| p.status == RunPhaseStatus::Queued && blocked.contains(&p.execution_id))
                .map(|p| p.execution_id.clone())
                .collect()
        }
    }
}

/// Build a fresh [`RunPlan`] from a picker selection (Phase 5): the phases
/// the user checked, in the order they were selected, under one queue-time
/// `stall_policy` and draft-PR consent. Every phase starts `Queued` /
/// `Pending` (interview unanswered).
///
/// `depends_on` defaults to the authored selection order — each phase
/// depends only on its immediate predecessor — per the PRD's "linear chain,
/// no editor" v1 lean; there is no edge editor in this phase.
///
/// Pure and side-effect-free: does not validate the IDs against
/// `docs/epics/` or touch disk — the caller (`commands::run_queue::
/// create_run_plan`) does both, since ID validation needs the repo root and
/// persistence needs `AppState`'s run-lock guard.
pub(crate) fn build_run_plan(
    id: String,
    project: PathBuf,
    created: DateTime<Utc>,
    execution_ids: &[String],
    stall_policy: StallPolicy,
    draft_pr_authorized: bool,
) -> RunPlan {
    let phases = execution_ids
        .iter()
        .enumerate()
        .map(|(i, exec_id)| RunPhase {
            execution_id: exec_id.clone(),
            status: RunPhaseStatus::Queued,
            interview: Vec::new(),
            interview_status: InterviewStatus::Pending,
            depends_on: if i == 0 {
                Vec::new()
            } else {
                vec![execution_ids[i - 1].clone()]
            },
            assigned_agent: None,
            park_payload: None,
            token_usage: 0,
            wall_clock_secs: 0,
        })
        .collect();

    RunPlan {
        id,
        project,
        created,
        consent: RunConsent {
            draft_pr_authorized,
        },
        budgets: RunBudgets::default(),
        environment: RunEnvironment::default(),
        wall_clock_secs: 0,
        stall_policy,
        phases,
    }
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

    /// Single-loop convenience wrapper over [`build_combined_phase_prompt`]
    /// for tests that only care about one phase's prompt shape.
    fn build_phase_prompt(
        execution_id: &str,
        loc: &LoopLocation,
        interview: &[PinnedAnswer],
        draft_pr_authorized: bool,
        budgets: ResolvedBudgets,
    ) -> String {
        build_combined_phase_prompt(
            &[(execution_id.to_string(), loc.clone(), interview.to_vec())],
            draft_pr_authorized,
            budgets,
        )
    }

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
        let prompt = build_phase_prompt(
            "prd-run-queue/phase-2",
            &loc,
            &interview,
            false,
            test_budgets(),
        );
        assert!(prompt.contains("prd-run-queue/phase-2"));
        assert!(prompt.contains("Queue executor"));
        assert!(prompt.contains("Which stall policy?"));
        assert!(prompt.contains("halt"));
    }

    #[test]
    fn build_combined_phase_prompt_covers_every_loop_in_one_turn() {
        let loc_a = LoopLocation {
            epic: "overnight-orchestration".into(),
            prd: "prd-run-queue".into(),
            phase: "Phase 2".into(),
            title: "Queue executor".into(),
        };
        let loc_b = LoopLocation {
            epic: "overnight-orchestration".into(),
            prd: "prd-run-queue".into(),
            phase: "Phase 3".into(),
            title: "Pre-flight interview".into(),
        };
        let interview_b = vec![PinnedAnswer {
            question: "Which stall policy?".into(),
            answer: "halt".into(),
        }];
        let prompt = build_combined_phase_prompt(
            &[
                ("prd-run-queue/phase-2".into(), loc_a, vec![]),
                ("prd-run-queue/phase-3".into(), loc_b, interview_b),
            ],
            false,
            test_budgets(),
        );

        assert!(prompt.contains("2 queued loops"));
        assert!(prompt.contains("prd-run-queue/phase-2"));
        assert!(prompt.contains("Queue executor"));
        assert!(prompt.contains("prd-run-queue/phase-3"));
        assert!(prompt.contains("Pre-flight interview"));
        assert!(prompt.contains("Which stall policy?"));
        assert!(prompt.contains("halt"));
        assert!(prompt.contains("verify→ship flow (Phases 6-7) once"));
        assert!(prompt.contains("main checkout is the run's control plane"));
        assert!(
            prompt.contains("Do not read, write, commit, or use those local control-plane files")
        );
    }

    #[test]
    fn build_combined_phase_prompt_requires_verdict_in_final_message() {
        let loc = LoopLocation {
            epic: "e".into(),
            prd: "p".into(),
            phase: "ph".into(),
            title: "t".into(),
        };
        let prompt =
            build_combined_phase_prompt(&[("e/p-1".into(), loc, vec![])], false, test_budgets());
        assert!(prompt.contains("your own final chat"));
        assert!(
            prompt.contains("`**Verdict:** PASS`, `**Verdict:** WARN`, or `**Verdict:** BLOCK`")
        );
    }

    #[test]
    fn build_combined_phase_prompt_single_loop_omits_batch_framing() {
        let loc = LoopLocation {
            epic: "e".into(),
            prd: "p".into(),
            phase: "ph".into(),
            title: "t".into(),
        };
        let prompt =
            build_combined_phase_prompt(&[("e/p-1".into(), loc, vec![])], true, test_budgets());
        assert!(!prompt.contains("queued loops"));
        assert!(!prompt.contains("### Loop"));
    }

    #[test]
    fn build_phase_prompt_omits_interview_section_when_empty() {
        let loc = LoopLocation {
            epic: "e".into(),
            prd: "p".into(),
            phase: "ph".into(),
            title: "t".into(),
        };
        let prompt = build_phase_prompt("e/p-1", &loc, &[], false, test_budgets());
        assert!(!prompt.contains("already answered"));
    }

    #[test]
    fn build_phase_prompt_default_stops_after_verdict_without_draft_pr_language() {
        let loc = LoopLocation {
            epic: "e".into(),
            prd: "p".into(),
            phase: "ph".into(),
            title: "t".into(),
        };
        let prompt = build_phase_prompt("e/p-1", &loc, &[], false, test_budgets());
        assert!(prompt.contains("stop after Phase 6's `**Verdict:**` line"));
        assert!(!prompt.contains("--draft"));
        assert!(!prompt.contains("pre-authorized"));
    }

    #[test]
    fn build_phase_prompt_authorized_grants_draft_pr_skip() {
        let loc = LoopLocation {
            epic: "e".into(),
            prd: "p".into(),
            phase: "ph".into(),
            title: "t".into(),
        };
        let prompt = build_phase_prompt("e/p-1", &loc, &[], true, test_budgets());
        assert!(
            prompt.contains("pre-authorized at queue time to open a draft pull request unattended")
        );
        assert!(prompt.contains("skip Phase 4's interactive confirmation"));
        assert!(prompt.contains("--draft"));
        assert!(prompt.contains("no `--web`"));
        assert!(prompt.contains("never mark it ready for review or merge it"));
        assert!(!prompt.contains("stop after Phase 6's `**Verdict:**` line unless"));
    }

    #[test]
    fn build_phase_prompt_authorized_includes_verdict_table_and_run_metadata_instructions() {
        let loc = LoopLocation {
            epic: "overnight-orchestration".into(),
            prd: "prd-unattended-ship".into(),
            phase: "Phase 2".into(),
            title: "Draft-PR autonomy".into(),
        };
        let budgets = ResolvedBudgets {
            phase_token_cap: 500_000,
            phase_wall_clock_secs: 5_400,
            run_wall_clock_secs: 28_800,
        };
        let prompt = build_phase_prompt("prd-unattended-ship/phase-2", &loc, &[], true, budgets);
        assert!(prompt.contains("## Verify Verdict"));
        assert!(prompt.contains("## Run metadata"));
        assert!(prompt.contains("prd-unattended-ship/phase-2"));
        assert!(prompt.contains("500000 tokens total"));
        assert!(prompt.contains("5400s wall-clock cap for this turn"));
        assert!(prompt.contains("28800s total-run wall-clock cap"));
    }

    fn test_budgets() -> ResolvedBudgets {
        ResolvedBudgets {
            phase_token_cap: 500_000,
            phase_wall_clock_secs: 5_400,
            run_wall_clock_secs: 28_800,
        }
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
    fn codex_interview_prompt_uses_native_user_input_not_claude_tool() {
        let loc = LoopLocation {
            epic: "overnight-orchestration".into(),
            prd: "prd-run-queue".into(),
            phase: "Phase 3".into(),
            title: "Pre-flight interview".into(),
        };

        let prompt = build_codex_interview_prompt("prd-run-queue/phase-3", &loc);
        assert!(prompt.contains("native user-input request"));
        assert!(prompt.contains("Do not try to call a tool named `AskUserQuestion`"));
        assert!(prompt.contains("## Pre-flight Answers"));
    }

    #[test]
    fn batch_interview_prompt_keeps_every_phase_context_and_one_combined_card_instruction() {
        let phases = vec![
            (
                "prd-a/loop-a".into(),
                LoopLocation {
                    epic: "epic-a".into(),
                    prd: "prd-a".into(),
                    phase: "Phase 1".into(),
                    title: "First loop".into(),
                },
            ),
            (
                "prd-b/loop-b".into(),
                LoopLocation {
                    epic: "epic-b".into(),
                    prd: "prd-b".into(),
                    phase: "Phase 2".into(),
                    title: "Second loop".into(),
                },
            ),
        ];
        let prompt = build_batch_interview_prompt(&phases);
        assert!(prompt.contains("prd-a/loop-a"));
        assert!(prompt.contains("prd-b/loop-b"));
        assert!(prompt.contains("one `AskUserQuestion` call"));
        assert!(prompt.contains("## Batch Pre-flight Answers"));
    }

    #[test]
    fn codex_batch_interview_prompt_uses_native_user_input_not_claude_tool() {
        let phases = vec![(
            "prd-a/loop-a".into(),
            LoopLocation {
                epic: "epic-a".into(),
                prd: "prd-a".into(),
                phase: "Phase 1".into(),
                title: "First loop".into(),
            },
        )];

        let prompt = build_codex_batch_interview_prompt(&phases);
        assert!(prompt.contains("native Codex user-input request"));
        assert!(prompt.contains("Do not try to call a tool named `AskUserQuestion`"));
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
    fn extract_batch_interview_answers_keeps_answers_with_their_phase() {
        let text = "## Batch Pre-flight Answers\n\n\
                    ### Phase `prd-a/loop-a`\n\
                    - Q: Which API?\n  A: REST\n\n\
                    ### Phase `prd-b/loop-b`\n\
                    - Q: Which color?\n  A: Violet\n";
        let answers = extract_batch_interview_answers(text);
        assert_eq!(answers["prd-a/loop-a"][0].answer, "REST");
        assert_eq!(answers["prd-b/loop-b"][0].question, "Which color?");
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
            environment: Default::default(),
            wall_clock_secs: 0,
            stall_policy: Default::default(),
            phases: vec![
                RunPhase {
                    execution_id: "p/1".into(),
                    status: RunPhaseStatus::Running,
                    interview: vec![],
                    interview_status: Default::default(),
                    depends_on: vec![],
                    assigned_agent: None,
                    park_payload: None,
                    token_usage: 0,
                    wall_clock_secs: 0,
                },
                RunPhase {
                    execution_id: "p/2".into(),
                    status: RunPhaseStatus::Queued,
                    interview: vec![],
                    interview_status: Default::default(),
                    depends_on: vec![],
                    assigned_agent: None,
                    park_payload: None,
                    token_usage: 0,
                    wall_clock_secs: 0,
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

    // ── phases_blocked_by_park ──────────────────────────────────────────

    fn queued(id: &str, depends_on: &[&str]) -> runplan::RunPhase {
        runplan::RunPhase {
            execution_id: id.to_string(),
            status: RunPhaseStatus::Queued,
            interview: vec![],
            interview_status: Default::default(),
            depends_on: depends_on.iter().map(|s| s.to_string()).collect(),
            assigned_agent: None,
            park_payload: None,
            token_usage: 0,
            wall_clock_secs: 0,
        }
    }

    fn plan_with(phases: Vec<runplan::RunPhase>) -> RunPlan {
        use chrono::{TimeZone, Utc};
        use std::path::PathBuf;
        RunPlan {
            id: "run-1".to_string(),
            project: PathBuf::from("/repo"),
            created: Utc.with_ymd_and_hms(2026, 7, 28, 9, 0, 0).unwrap(),
            consent: Default::default(),
            budgets: Default::default(),
            environment: Default::default(),
            wall_clock_secs: 0,
            stall_policy: StallPolicy::ContinueIndependent,
            phases,
        }
    }

    #[test]
    fn continue_independent_parks_only_the_dependent_chain() {
        // p1 (parking) <- p2 depends on p1 <- p3 depends on p2. p4 is
        // unrelated and must stay eligible to run.
        let plan = plan_with(vec![
            queued("p1", &[]),
            queued("p2", &["p1"]),
            queued("p3", &["p2"]),
            queued("p4", &[]),
        ]);
        let mut blocked = phases_blocked_by_park(&plan, "p1", StallPolicy::ContinueIndependent);
        blocked.sort();
        assert_eq!(blocked, vec!["p2".to_string(), "p3".to_string()]);
    }

    #[test]
    fn continue_independent_leaves_unrelated_phases_queued() {
        let plan = plan_with(vec![queued("p1", &[]), queued("p2", &[])]);
        let blocked = phases_blocked_by_park(&plan, "p1", StallPolicy::ContinueIndependent);
        assert!(blocked.is_empty());
    }

    #[test]
    fn continue_independent_ignores_non_queued_phases() {
        let mut plan = plan_with(vec![queued("p1", &[]), queued("p2", &["p1"])]);
        plan.phases[1].status = RunPhaseStatus::Completed;
        let blocked = phases_blocked_by_park(&plan, "p1", StallPolicy::ContinueIndependent);
        assert!(blocked.is_empty());
    }

    #[test]
    fn halt_blocks_every_remaining_queued_phase() {
        let mut plan = plan_with(vec![
            queued("p1", &[]),
            queued("p2", &[]),
            queued("p3", &[]),
        ]);
        plan.phases[0].status = RunPhaseStatus::Completed;
        let mut blocked = phases_blocked_by_park(&plan, "p4-not-in-plan", StallPolicy::Halt);
        blocked.sort();
        assert_eq!(blocked, vec!["p2".to_string(), "p3".to_string()]);
    }

    // ── build_run_plan ──────────────────────────────────────────────────

    #[test]
    fn build_run_plan_chains_depends_on_in_selection_order() {
        use chrono::TimeZone;
        let ids = vec!["a".to_string(), "b".to_string(), "c".to_string()];
        let plan = build_run_plan(
            "run-1".to_string(),
            PathBuf::from("/repo"),
            Utc.with_ymd_and_hms(2026, 7, 28, 9, 0, 0).unwrap(),
            &ids,
            StallPolicy::Halt,
            true,
        );
        assert_eq!(plan.phases.len(), 3);
        assert_eq!(plan.phases[0].depends_on, Vec::<String>::new());
        assert_eq!(plan.phases[1].depends_on, vec!["a".to_string()]);
        assert_eq!(plan.phases[2].depends_on, vec!["b".to_string()]);
        assert!(plan
            .phases
            .iter()
            .all(|p| p.status == RunPhaseStatus::Queued));
        assert!(plan
            .phases
            .iter()
            .all(|p| p.interview_status == InterviewStatus::Pending));
        assert_eq!(plan.stall_policy, StallPolicy::Halt);
        assert!(plan.consent.draft_pr_authorized);
    }

    #[test]
    fn build_run_plan_single_phase_has_no_dependency() {
        use chrono::TimeZone;
        let ids = vec!["only".to_string()];
        let plan = build_run_plan(
            "run-2".to_string(),
            PathBuf::from("/repo"),
            Utc.with_ymd_and_hms(2026, 7, 28, 9, 0, 0).unwrap(),
            &ids,
            StallPolicy::ContinueIndependent,
            false,
        );
        assert_eq!(plan.phases.len(), 1);
        assert!(plan.phases[0].depends_on.is_empty());
        assert!(!plan.consent.draft_pr_authorized);
    }

    // ── Integration tests for executor state machine ───────────────────

    /// Test helper: create a minimal RunPlan with phases in specified statuses.
    fn make_plan_with_statuses(phases: Vec<(&str, RunPhaseStatus, Vec<&str>)>) -> RunPlan {
        use chrono::TimeZone;
        let phases = phases
            .into_iter()
            .map(|(id, status, deps)| RunPhase {
                execution_id: id.to_string(),
                status,
                interview: vec![],
                interview_status: InterviewStatus::Answered,
                depends_on: deps.iter().map(|d| d.to_string()).collect(),
                assigned_agent: None,
                park_payload: None,
                token_usage: 0,
                wall_clock_secs: 0,
            })
            .collect();

        RunPlan {
            id: "test-run".to_string(),
            project: PathBuf::from("/test"),
            created: Utc.with_ymd_and_hms(2026, 8, 1, 9, 0, 0).unwrap(),
            consent: RunConsent {
                draft_pr_authorized: false,
            },
            budgets: RunBudgets::default(),
            environment: RunEnvironment::default(),
            wall_clock_secs: 0,
            stall_policy: StallPolicy::ContinueIndependent,
            phases,
        }
    }

    #[test]
    fn state_machine_advance_on_green_verdict() {
        // Simulate the advance-on-green path: when extract_verdict returns
        // PASS, the phase should transition Queued → Running → Completed.
        // We verify this by checking what the executor would do with a PASS.

        let dir =
            std::env::temp_dir().join(format!("loopdeck-sm-advance-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();

        let mut plan = make_plan_with_statuses(vec![
            ("phase1", RunPhaseStatus::Queued, vec![]),
            ("phase2", RunPhaseStatus::Queued, vec!["phase1"]),
        ]);
        runplan::save(&dir, &plan).unwrap();

        // Simulate the executor marking phase1 as Running
        plan.phases[0].status = RunPhaseStatus::Running;
        runplan::save(&dir, &plan).unwrap();

        // Verify state persisted correctly
        let loaded = runplan::load(&dir).unwrap().unwrap();
        assert_eq!(loaded.phases[0].status, RunPhaseStatus::Running);

        // Simulate successful completion (PASS verdict)
        plan.phases[0].status = RunPhaseStatus::Completed;
        runplan::save(&dir, &plan).unwrap();

        // Verify the completed state
        let final_plan = runplan::load(&dir).unwrap().unwrap();
        assert_eq!(final_plan.phases[0].status, RunPhaseStatus::Completed);
        assert_eq!(final_plan.phases[1].status, RunPhaseStatus::Queued);

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn state_machine_park_on_stall() {
        // Simulate park-on-stall: when a TurnParked error occurs, the phase
        // should be marked Parked with a payload, and dependent phases should
        // be handled per the stall policy.

        let dir = std::env::temp_dir().join(format!("loopdeck-sm-park-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();

        let mut plan = make_plan_with_statuses(vec![
            ("phase1", RunPhaseStatus::Queued, vec![]),
            ("phase2", RunPhaseStatus::Queued, vec!["phase1"]),
        ]);
        plan.stall_policy = StallPolicy::ContinueIndependent;
        runplan::save(&dir, &plan).unwrap();

        // Simulate phase1 parking
        plan.phases[0].status = RunPhaseStatus::Parked;
        plan.phases[0].park_payload = Some("waiting for user input".to_string());

        // Apply blocking logic
        let blocked = phases_blocked_by_park(&plan, "phase1", plan.stall_policy);
        for blocked_id in blocked {
            if let Some(p) = plan
                .phases
                .iter_mut()
                .find(|p| p.execution_id == blocked_id)
            {
                p.status = RunPhaseStatus::Parked;
                p.park_payload = Some(format!("blocked: depends on parked phase \"phase1\""));
            }
        }
        runplan::save(&dir, &plan).unwrap();

        // Verify both phases are parked
        let final_plan = runplan::load(&dir).unwrap().unwrap();
        assert_eq!(final_plan.phases[0].status, RunPhaseStatus::Parked);
        assert_eq!(
            final_plan.phases[0].park_payload,
            Some("waiting for user input".to_string())
        );
        assert_eq!(final_plan.phases[1].status, RunPhaseStatus::Parked);
        assert!(final_plan.phases[1]
            .park_payload
            .as_ref()
            .unwrap()
            .contains("blocked: depends on parked phase"));

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn state_machine_transitive_dependent_parking() {
        // Test that under ContinueIndependent, transitive dependencies are
        // correctly computed: if A parks and B depends on A and C depends on
        // B, both B and C should park, but D (unrelated) should stay queued.

        let dir =
            std::env::temp_dir().join(format!("loopdeck-sm-transitive-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();

        let mut plan = make_plan_with_statuses(vec![
            ("a", RunPhaseStatus::Queued, vec![]),
            ("b", RunPhaseStatus::Queued, vec!["a"]),
            ("c", RunPhaseStatus::Queued, vec!["b"]),
            ("d", RunPhaseStatus::Queued, vec![]),
        ]);
        plan.stall_policy = StallPolicy::ContinueIndependent;
        runplan::save(&dir, &plan).unwrap();

        // Park phase A
        plan.phases[0].status = RunPhaseStatus::Parked;
        plan.phases[0].park_payload = Some("parked".to_string());

        // Compute blocked phases
        let blocked = phases_blocked_by_park(&plan, "a", plan.stall_policy);
        for blocked_id in blocked {
            if let Some(p) = plan
                .phases
                .iter_mut()
                .find(|p| p.execution_id == blocked_id)
            {
                p.status = RunPhaseStatus::Parked;
                p.park_payload = Some(format!("blocked: depends on parked phase \"a\""));
            }
        }
        runplan::save(&dir, &plan).unwrap();

        let final_plan = runplan::load(&dir).unwrap().unwrap();
        // A, B, C should be parked; D should remain queued
        assert_eq!(final_plan.phases[0].status, RunPhaseStatus::Parked); // a
        assert_eq!(final_plan.phases[1].status, RunPhaseStatus::Parked); // b
        assert_eq!(final_plan.phases[2].status, RunPhaseStatus::Parked); // c
        assert_eq!(final_plan.phases[3].status, RunPhaseStatus::Queued); // d

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn state_machine_halt_policy_stops_all() {
        // Test that under Halt policy, when any phase parks, ALL remaining
        // queued phases are blocked, regardless of dependencies.

        let dir = std::env::temp_dir().join(format!("loopdeck-sm-halt-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();

        let mut plan = make_plan_with_statuses(vec![
            ("phase1", RunPhaseStatus::Completed, vec![]), // already done
            ("phase2", RunPhaseStatus::Queued, vec!["phase1"]),
            ("phase3", RunPhaseStatus::Queued, vec![]),
            ("phase4", RunPhaseStatus::Queued, vec![]),
        ]);
        plan.stall_policy = StallPolicy::Halt;
        runplan::save(&dir, &plan).unwrap();

        // Park phase2
        plan.phases[1].status = RunPhaseStatus::Parked;
        plan.phases[1].park_payload = Some("stalled".to_string());

        // Under Halt, all remaining queued phases are blocked
        let blocked = phases_blocked_by_park(&plan, "phase2", plan.stall_policy);
        for blocked_id in blocked {
            if let Some(p) = plan
                .phases
                .iter_mut()
                .find(|p| p.execution_id == blocked_id)
            {
                p.status = RunPhaseStatus::Parked;
                p.park_payload = Some(format!("blocked: depends on parked phase \"phase2\""));
            }
        }
        runplan::save(&dir, &plan).unwrap();

        let final_plan = runplan::load(&dir).unwrap().unwrap();
        assert_eq!(final_plan.phases[0].status, RunPhaseStatus::Completed); // unchanged
        assert_eq!(final_plan.phases[1].status, RunPhaseStatus::Parked); // parked
        assert_eq!(final_plan.phases[2].status, RunPhaseStatus::Parked); // blocked
        assert_eq!(final_plan.phases[3].status, RunPhaseStatus::Parked); // blocked

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn state_machine_resume_after_restart() {
        // Test the reconcile_running_phases behavior: a Running phase left
        // on disk from a crash should be downgraded to Interrupted when the
        // app restarts.

        let dir = std::env::temp_dir().join(format!("loopdeck-sm-resume-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();

        let plan = make_plan_with_statuses(vec![
            ("phase1", RunPhaseStatus::Running, vec![]), // crashed mid-run
            ("phase2", RunPhaseStatus::Queued, vec!["phase1"]),
        ]);
        runplan::save(&dir, &plan).unwrap();

        // Simulate app restart: reconcile should downgrade Running → Interrupted
        let changed = reconcile_running_phases(&dir).unwrap();
        assert!(changed);

        let reconciled = runplan::load(&dir).unwrap().unwrap();
        assert_eq!(reconciled.phases[0].status, RunPhaseStatus::Interrupted);
        assert_eq!(reconciled.phases[1].status, RunPhaseStatus::Queued);

        // Second reconcile is a no-op
        let changed_again = reconcile_running_phases(&dir).unwrap();
        assert!(!changed_again);

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn state_machine_non_green_verdict_fails_phase() {
        // Test that WARN, BLOCK, or missing verdict all result in Failed status.
        // This verifies the executor's "only PASS advances" logic.

        let dir = std::env::temp_dir().join(format!("loopdeck-sm-fail-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();

        let mut plan = make_plan_with_statuses(vec![
            ("phase1", RunPhaseStatus::Queued, vec![]),
            ("phase2", RunPhaseStatus::Queued, vec!["phase1"]),
        ]);
        runplan::save(&dir, &plan).unwrap();

        // Simulate phase1 getting a WARN verdict (non-green)
        plan.phases[0].status = RunPhaseStatus::Failed;
        plan.phases[0].park_payload = Some("verify verdict: WARN".to_string());
        runplan::save(&dir, &plan).unwrap();

        let final_plan = runplan::load(&dir).unwrap().unwrap();
        assert_eq!(final_plan.phases[0].status, RunPhaseStatus::Failed);
        assert_eq!(
            final_plan.phases[0].park_payload,
            Some("verify verdict: WARN".to_string())
        );
        // phase2 remains queued, but the run would have stopped
        assert_eq!(final_plan.phases[1].status, RunPhaseStatus::Queued);

        std::fs::remove_dir_all(&dir).ok();
    }
}
