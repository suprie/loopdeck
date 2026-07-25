//! Migration from legacy `.loopdeck/loops.md` to structured
//! `.loopdeck/execution.yaml` (milestone 0.2.1, Phase 4).
//!
//! Exact-match, no-fuzzy identity migration. Each legacy Current/History record
//! is matched to a PRD checklist loop (stable ID) by exact `(epic, prd, title)`
//! when the record carries the promote-bridge `**Epic**`/`**PRD**` back-reference,
//! else by exact title-only equality across every PRD. Exactly one match assigns
//! the ID; zero or many leave the record **unmatched**.
//!
//! Unmatched records are preserved verbatim in `loops.legacy.md` (the renamed
//! original — no data loss) and surfaced in the preview report; they are never
//! written to `execution.yaml` (no schema change to the canonical store).
//!
//! `build_preview` is pure (no I/O) so the test matrix can exercise matching
//! without touching disk; `apply` is the crash-safe, idempotent write path. The
//! IPC layer (`commands::execution`) and the `loopdeck state migrate` CLI wrap
//! this module — no transition logic is duplicated.

use crate::epic;
use crate::error::AppError;
use crate::execution::{
    self, ActiveLoop, ExecutionState, HistoryLoop, LoopOrigin, Outcome, QueuedLoop,
};
use crate::limits;
use crate::memory;
use chrono::{DateTime, NaiveDate, TimeZone, Utc};
use serde::Serialize;
use std::collections::HashSet;
use std::path::Path;

// ── Wire types (returned over IPC) ──────────────────────────────────

/// One legacy record from `loops.md` (Current or History), parsed richly enough
/// to carry the spec back-references the lenient `memory::Loop` parser drops.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct LegacyLoopRecord {
    /// Display title (`**Goal**` or the History heading title).
    pub title: String,
    /// `**Epic**`/`**PRD**` back-reference (phase is optional — the promote
    /// bridge doesn't write it). `None` for hand-authored or older entries, in
    /// which case matching falls back to title-only exact.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub origin_hint: Option<LoopOrigin>,
    /// Raw status string ("in_progress" | "completed" | "abandoned").
    pub status: String,
    /// Raw `**Started**` / heading-date value (often `YYYY-MM-DD`).
    #[serde(default)]
    pub started: String,
    /// Raw `**Completed**` value when present.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub completed: Option<String>,
}

/// A `## Next Steps` checklist item, preserved with its checked state.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct LegacyNextStep {
    pub text: String,
    pub checked: bool,
}

/// The full legacy `loops.md` content, parsed richly for migration.
#[derive(Debug, Clone, PartialEq, Serialize, Default)]
pub struct LegacyLoops {
    pub current: Option<LegacyLoopRecord>,
    pub next_steps: Vec<LegacyNextStep>,
    pub history: Vec<LegacyLoopRecord>,
}

/// A legacy record that could not be mapped to exactly one PRD loop, surfaced
/// for reconciliation. Its text lives on in `loops.legacy.md`; it is NOT written
/// to `execution.yaml`. The `label` is a synthetic report reference only.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct UnmatchedRecord {
    pub label: String,
    pub title: String,
    /// "current" | "history".
    pub section: &'static str,
    /// "no match" | "ambiguous" | "duplicate id" — never a guess.
    pub reason: &'static str,
}

/// A Next Steps item that could not become a queue entry (its backtick token
/// wasn't a known loop ID). Preserved verbatim so no checklist text is lost.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct UnconvertedNextStep {
    pub text: String,
    pub checked: bool,
}

/// The migration preview: the planned `execution.yaml` plus everything that
/// could not be mapped. Read-only — computing it writes nothing.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct MigrationPreview {
    /// The structured state that would be written on confirmation.
    pub planned: ExecutionState,
    /// True when the Current loop mapped to a stable ID.
    pub current_matched: bool,
    /// Count of History records that mapped.
    pub matched_history: usize,
    /// Legacy records that did not map (reconciliation panel).
    pub unmatched: Vec<UnmatchedRecord>,
    /// Next Steps that could not become queue entries (preserved text).
    pub unconverted_next_steps: Vec<UnconvertedNextStep>,
    /// `true` when `execution.yaml` already exists — migration is unavailable.
    pub execution_yaml_present: bool,
    /// `true` when a `loops.md` was found.
    pub loops_md_present: bool,
}

impl MigrationPreview {
    /// Migration is available iff a `loops.md` exists and the project is not yet
    /// structured (`execution.yaml` absent).
    pub fn available(&self) -> bool {
        self.loops_md_present && !self.execution_yaml_present
    }
}

// ── Internal matching types ─────────────────────────────────────────

/// Outcome of matching one legacy record against the spec index.
#[derive(Debug, Clone, PartialEq)]
enum MatchOutcome {
    /// Exactly one PRD loop matched.
    Matched { id: String, origin: LoopOrigin },
    /// Zero (missing) or more than one (ambiguous).
    Unmatched(UnmatchedReason),
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum UnmatchedReason {
    NoMatch,
    Ambiguous,
}

impl UnmatchedReason {
    fn label(self) -> &'static str {
        match self {
            UnmatchedReason::NoMatch => "no match",
            UnmatchedReason::Ambiguous => "ambiguous",
        }
    }
}

/// A PRD checklist loop in the spec index: stable ID, origin, title.
#[derive(Debug, Clone)]
struct SpecEntry {
    id: String,
    origin: LoopOrigin,
    title: String,
}

// ── Rich legacy parse ───────────────────────────────────────────────

/// Parse `loops.md` content into the rich [`LegacyLoops`] shape (capturing the
/// Epic/PRD back-references `memory::Loop` drops). The legacy read path
/// (`memory::parse_loops`) is left untouched; this is migration-only.
pub fn parse_loops_for_migration(content: &str) -> LegacyLoops {
    let mut current: Option<LegacyLoopRecord> = None;
    let mut next_steps: Vec<LegacyNextStep> = Vec::new();
    let mut history: Vec<LegacyLoopRecord> = Vec::new();

    // Prepend a newline so a leading `## ` is caught, mirroring `parse_loops_content`.
    let normalized = format!("\n{content}");
    for section in normalized.split("\n## ") {
        if let Some(rest) = section.strip_prefix("Current") {
            current = parse_current_record(rest);
        } else if let Some(rest) = section.strip_prefix("Next Steps") {
            next_steps = parse_next_steps(rest);
        } else if let Some(rest) = section.strip_prefix("History") {
            history = parse_history_records(rest);
        }
        // Preamble (`# Loops` header) is ignored.
    }

    LegacyLoops {
        current,
        next_steps,
        history,
    }
}

#[derive(Default)]
struct OriginParts {
    epic: Option<String>,
    prd: Option<String>,
    phase: Option<String>,
}

impl OriginParts {
    /// A hint is present only when the promote bridge's `**Epic**` + `**PRD**`
    /// were both captured (phase is optional and rarely written).
    fn into_hint(self) -> Option<LoopOrigin> {
        match (self.epic, self.prd) {
            (Some(epic), Some(prd)) => Some(LoopOrigin {
                epic,
                prd,
                phase: self.phase.unwrap_or_default(),
            }),
            _ => None,
        }
    }
}

fn parse_current_record(section: &str) -> Option<LegacyLoopRecord> {
    if section.contains("_No active loop._") {
        return None;
    }

    let mut title = String::new();
    let mut origin = OriginParts::default();
    let mut status = String::from("in_progress");
    let mut started = String::new();
    let mut completed: Option<String> = None;

    for line in section.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        if let Some((key, value)) = memory::parse_kv_line(trimmed) {
            match key.to_lowercase().as_str() {
                "goal" => title = value.to_string(),
                "started" => started = value.to_string(),
                "status" => status = value.to_string(),
                "completed" => completed = Some(value.to_string()),
                "epic" => origin.epic = Some(value.to_string()),
                "prd" => origin.prd = Some(value.to_string()),
                "phase" => origin.phase = Some(value.to_string()),
                _ => {}
            }
        }
    }

    if title.is_empty() {
        return None;
    }

    Some(LegacyLoopRecord {
        title,
        origin_hint: origin.into_hint(),
        status,
        started,
        completed,
    })
}

fn parse_next_steps(section: &str) -> Vec<LegacyNextStep> {
    section
        .lines()
        .filter_map(|line| {
            let trimmed = line.trim();
            if let Some(rest) = trimmed.strip_prefix("- [ ] ") {
                Some(LegacyNextStep {
                    text: rest.to_string(),
                    checked: false,
                })
            } else {
                trimmed
                    .strip_prefix("- [x] ")
                    .or_else(|| trimmed.strip_prefix("- [X] "))
                    .map(|rest| LegacyNextStep {
                        text: rest.to_string(),
                        checked: true,
                    })
            }
        })
        .collect()
}

fn parse_history_records(section: &str) -> Vec<LegacyLoopRecord> {
    let mut out = Vec::new();
    let entries: Vec<&str> = section.split("\n### ").collect();
    for (i, entry) in entries.iter().enumerate() {
        if i == 0 {
            // Text before the first `### ` may itself be a `YYYY-MM-DD — Title` entry.
            if let Some(first_line) = entry.lines().next() {
                let trimmed = first_line.trim();
                if trimmed.contains(" — ") || trimmed.contains(" - ") {
                    if let Some(rec) = parse_history_entry(entry) {
                        out.push(rec);
                    }
                }
            }
            continue;
        }
        if let Some(rec) = parse_history_entry(entry) {
            out.push(rec);
        }
    }
    out
}

fn parse_history_entry(entry: &str) -> Option<LegacyLoopRecord> {
    let mut lines = entry.lines();
    let heading = lines.next()?.trim();
    let (started, title) = memory::split_heading(heading)?;

    let mut status = String::from("completed");
    let mut completed: Option<String> = None;
    let mut origin = OriginParts::default();

    for line in lines {
        let trimmed = line.trim();
        if let Some((key, value)) = memory::parse_kv_line(trimmed) {
            match key.to_lowercase().as_str() {
                "status" => status = value.to_string(),
                "completed" => completed = Some(value.to_string()),
                "epic" => origin.epic = Some(value.to_string()),
                "prd" => origin.prd = Some(value.to_string()),
                "phase" => origin.phase = Some(value.to_string()),
                _ => {}
            }
        }
    }

    Some(LegacyLoopRecord {
        title,
        origin_hint: origin.into_hint(),
        status,
        started,
        completed,
    })
}

// ── Spec index ──────────────────────────────────────────────────────

/// Build the spec index: every PRD checklist loop that carries a stable ID,
/// with its origin and title. Loops without an ID are skipped (they can't be a
/// migration target). Mirrors `epic::find_loop_by_id`'s walk but collects all.
fn build_spec_index(repo_path: &Path) -> Vec<SpecEntry> {
    let mut out = Vec::new();
    for epic in epic::parse_epics(repo_path) {
        for prd in &epic.prds {
            for phase in &prd.phases {
                for item in &phase.loops {
                    if let Some(id) = &item.id {
                        out.push(SpecEntry {
                            id: id.clone(),
                            origin: LoopOrigin {
                                epic: epic.slug.clone(),
                                prd: prd.slug.clone(),
                                phase: phase.name.clone(),
                            },
                            title: item.title.clone(),
                        });
                    }
                }
            }
        }
    }
    out
}

// ── Matching ────────────────────────────────────────────────────────

/// Match a legacy record against the spec index. Exact only — epic+prd+title
/// when the record has a back-ref, else title-only; "exactly one" wins.
fn match_legacy(record: &LegacyLoopRecord, spec: &[SpecEntry]) -> MatchOutcome {
    let title = record.title.trim();
    let hits: Vec<&SpecEntry> = spec
        .iter()
        .filter(|e| e.title.trim() == title && origin_qualifies(e, record))
        .collect();
    match hits.len() {
        1 => MatchOutcome::Matched {
            id: hits[0].id.clone(),
            origin: hits[0].origin.clone(),
        },
        0 => MatchOutcome::Unmatched(UnmatchedReason::NoMatch),
        _ => MatchOutcome::Unmatched(UnmatchedReason::Ambiguous),
    }
}

/// With a back-ref, only entries in the same epic+prd qualify (title still must
/// match exactly). Without one, every entry qualifies and title-alone decides.
fn origin_qualifies(entry: &SpecEntry, record: &LegacyLoopRecord) -> bool {
    match &record.origin_hint {
        Some(hint) => entry.origin.epic == hint.epic && entry.origin.prd == hint.prd,
        None => true,
    }
}

/// Extract the first backtick-quoted token from a Next Steps line. Membership in
/// the spec ID set (checked by the caller) decides whether it becomes a queue
/// entry; no grammar guessing here.
fn leading_backtick_token(text: &str) -> Option<String> {
    let bytes = text.as_bytes();
    let mut start = None;
    for (i, &b) in bytes.iter().enumerate() {
        if b == b'`' {
            if start.is_none() {
                start = Some(i + 1);
            } else {
                let s = start?;
                if s < i {
                    return Some(text[s..i].to_string());
                }
                return None;
            }
        }
    }
    None
}

// ── Preview (pure) ──────────────────────────────────────────────────

/// Assemble the migration preview from parsed legacy content + the spec index.
/// Pure — performs no I/O and writes nothing (so a cancelled preview leaves the
/// project untouched; PRD acceptance criterion 7). `now` seeds timestamps for
/// records whose legacy date can't be parsed, and is injected so tests are
/// deterministic.
fn build_preview(legacy: &LegacyLoops, spec: &[SpecEntry], now: DateTime<Utc>) -> MigrationPreview {
    let mut planned = ExecutionState::default();
    let mut current_matched = false;
    let mut matched_history = 0usize;
    let mut unmatched: Vec<UnmatchedRecord> = Vec::new();
    let mut unconverted: Vec<UnconvertedNextStep> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();
    let mut label_n = 0usize;

    let mut next_label = || {
        label_n += 1;
        format!("legacy/{label_n}")
    };

    // Current → ActiveLoop (or unmatched).
    if let Some(rec) = &legacy.current {
        match match_legacy(rec, spec) {
            MatchOutcome::Matched { id, origin } if !seen.contains(&id) => {
                seen.insert(id.clone());
                planned.current = Some(ActiveLoop {
                    id,
                    title: rec.title.clone(),
                    origin,
                    started_at: parse_legacy_dt(&rec.started, now),
                    attempt: 1,
                });
                current_matched = true;
            }
            MatchOutcome::Matched { .. } => {
                // ID already claimed (e.g. also appears in history) — don't
                // produce an invalid current+history collision; preserve it.
                unmatched.push(UnmatchedRecord {
                    label: next_label(),
                    title: rec.title.clone(),
                    section: "current",
                    reason: "duplicate id",
                });
            }
            MatchOutcome::Unmatched(reason) => {
                unmatched.push(UnmatchedRecord {
                    label: next_label(),
                    title: rec.title.clone(),
                    section: "current",
                    reason: reason.label(),
                });
            }
        }
    }

    // History → HistoryLoop (or unmatched).
    for rec in &legacy.history {
        match match_legacy(rec, spec) {
            MatchOutcome::Matched { id, origin } if !seen.contains(&id) => {
                seen.insert(id.clone());
                let outcome = outcome_from_status(&rec.status);
                let started_at = parse_legacy_dt(&rec.started, now);
                let completed_at = rec
                    .completed
                    .as_deref()
                    .map(|c| parse_legacy_dt(c, started_at))
                    .unwrap_or(started_at);
                let reason = (outcome == Outcome::Abandoned)
                    .then(|| "(migrated from legacy loops.md; no reason recorded)".to_string());
                planned.history.push(HistoryLoop {
                    id,
                    title: rec.title.clone(),
                    origin,
                    outcome,
                    started_at,
                    completed_at,
                    attempt: 1,
                    git: None,
                    reason,
                });
                matched_history += 1;
            }
            MatchOutcome::Matched { .. } => {
                unmatched.push(UnmatchedRecord {
                    label: next_label(),
                    title: rec.title.clone(),
                    section: "history",
                    reason: "duplicate id",
                });
            }
            MatchOutcome::Unmatched(reason) => {
                unmatched.push(UnmatchedRecord {
                    label: next_label(),
                    title: rec.title.clone(),
                    section: "history",
                    reason: reason.label(),
                });
            }
        }
    }

    // Next Steps → queue only when the line's backtick token is a known spec ID.
    // Everything else is preserved verbatim (no checklist text is lost).
    for step in &legacy.next_steps {
        let converts = leading_backtick_token(&step.text)
            .filter(|tok| spec.iter().any(|e| e.id == *tok))
            .filter(|tok| !seen.contains(tok));
        match converts {
            Some(id) => {
                let entry = spec
                    .iter()
                    .find(|e| e.id == id)
                    .expect("id validated against the spec index above");
                seen.insert(id.clone());
                planned.queue.push(QueuedLoop {
                    id,
                    title: entry.title.clone(),
                    origin: entry.origin.clone(),
                    queued_at: now,
                });
            }
            None => {
                unconverted.push(UnconvertedNextStep {
                    text: step.text.clone(),
                    checked: step.checked,
                });
            }
        }
    }

    MigrationPreview {
        planned,
        current_matched,
        matched_history,
        unmatched,
        unconverted_next_steps: unconverted,
        execution_yaml_present: false,
        loops_md_present: true,
    }
}

fn outcome_from_status(status: &str) -> Outcome {
    match status.trim().to_lowercase().as_str() {
        "abandoned" => Outcome::Abandoned,
        _ => Outcome::Completed,
    }
}

/// Parse a legacy date/datetime (RFC3339 or `YYYY-MM-DD`) to `DateTime<Utc>`,
/// falling back to `fallback` when it can't be parsed. Legacy dates are often
/// date-only, so a parse failure must never abort the migration.
fn parse_legacy_dt(s: &str, fallback: DateTime<Utc>) -> DateTime<Utc> {
    let s = s.trim();
    if s.is_empty() {
        return fallback;
    }
    if let Ok(dt) = DateTime::parse_from_rfc3339(s) {
        return dt.with_timezone(&Utc);
    }
    if let Ok(d) = NaiveDate::parse_from_str(s, "%Y-%m-%d") {
        if let Some(ndt) = d.and_hms_opt(0, 0, 0) {
            return Utc.from_utc_datetime(&ndt);
        }
    }
    fallback
}

// ── Preview + apply (I/O) ───────────────────────────────────────────

/// Compute the migration preview for a project. Read-only — writes nothing.
///
/// Short-circuits to `available() == false` when `execution.yaml` already exists
/// (the project is already structured; a lingering `loops.md` is a legacy
/// artifact, not something to re-migrate).
pub fn preview(repo_path: &Path, now: DateTime<Utc>) -> Result<MigrationPreview, AppError> {
    let exec_path = execution::execution_path(repo_path);
    let loops_path = memory::loops_md_path(repo_path);
    let execution_yaml_present = exec_path.exists();
    let loops_md_present = loops_path.exists();

    if execution_yaml_present {
        return Ok(MigrationPreview {
            planned: ExecutionState::default(),
            current_matched: false,
            matched_history: 0,
            unmatched: Vec::new(),
            unconverted_next_steps: Vec::new(),
            execution_yaml_present: true,
            loops_md_present,
        });
    }

    let mut p = if loops_md_present {
        let content = limits::read_bounded_to_string(&loops_path, limits::SPEC_MAX_BYTES)?;
        let legacy = parse_loops_for_migration(&content);
        let spec = build_spec_index(repo_path);
        build_preview(&legacy, &spec, now)
    } else {
        empty_preview()
    };
    p.execution_yaml_present = false;
    p.loops_md_present = loops_md_present;
    Ok(p)
}

/// Perform the confirmed migration: recompute the preview **server-side**
/// (never trust a client-supplied snapshot), validate, atomically write
/// `execution.yaml`, then rename `loops.md` → `loops.legacy.md`.
///
/// Crash-safe ordering: write `execution.yaml` first (validated + atomic), then
/// `fs::rename` (POSIX-atomic). At every crash point the worst case is the
/// "both exist" state that detection treats as execution-authoritative, and a
/// re-run completes the rename idempotently (see below). The original content is
/// never lost: it is the rename target.
///
/// **Idempotent** against the crash window: if `execution.yaml` exists and
/// `loops.md` is still present without a `loops.legacy.md`, this just finishes
/// the rename and returns the current structured state. A fully-migrated project
/// (`loops.md` absent) returns its current state with no rewrite.
pub fn apply(repo_path: &Path, now: DateTime<Utc>) -> Result<ExecutionState, AppError> {
    let exec_path = execution::execution_path(repo_path);
    let loops_path = memory::loops_md_path(repo_path);
    let legacy_path = repo_path.join(".loopdeck").join("loops.legacy.md");

    // Already structured — reconcile any half-finished rename, then return.
    if exec_path.exists() {
        if loops_path.exists() && !legacy_path.exists() {
            std::fs::rename(&loops_path, &legacy_path)?;
        }
        return execution::load(repo_path).map(|loaded| loaded.state);
    }

    // Fresh migration needs a loops.md.
    if !loops_path.exists() {
        return Err(AppError::ExecutionState(
            "no .loopdeck/loops.md to migrate".to_string(),
        ));
    }

    // Recompute server-side and validate before touching disk.
    let content = limits::read_bounded_to_string(&loops_path, limits::SPEC_MAX_BYTES)?;
    let legacy = parse_loops_for_migration(&content);
    let spec = build_spec_index(repo_path);
    let planned = build_preview(&legacy, &spec, now).planned;
    planned.validate()?;

    // 1. Atomic write execution.yaml (validates again + OCC revision 0; no
    //    primary to back up yet). A failure here leaves loops.md untouched.
    execution::save(repo_path, &planned, 0)?;

    // 2. Rename loops.md → loops.legacy.md. A crash here is recovered by the
    //    idempotent branch above on the next apply.
    std::fs::rename(&loops_path, &legacy_path)?;

    Ok(planned)
}

fn empty_preview() -> MigrationPreview {
    MigrationPreview {
        planned: ExecutionState::default(),
        current_matched: false,
        matched_history: 0,
        unmatched: Vec::new(),
        unconverted_next_steps: Vec::new(),
        execution_yaml_present: false,
        loops_md_present: false,
    }
}

#[cfg(test)]
mod tests {
    //! Pure matching/preview tests run against synthetic `loops.md` strings +
    //! an in-memory spec index (no filesystem). The I/O `apply` path and the
    //! dogfood parse of this repo's own `loops.md` are exercised separately.
    use super::*;
    use crate::execution::Outcome;
    use std::path::{Path, PathBuf};

    /// A fixed timestamp so preview tests are deterministic.
    fn now() -> DateTime<Utc> {
        DateTime::parse_from_rfc3339("2026-07-25T12:00:00+00:00")
            .unwrap()
            .with_timezone(&Utc)
    }

    /// Build a spec index from `(id, epic, prd, title)` tuples — no filesystem.
    fn spec_of(rows: &[(&str, &str, &str, &str)]) -> Vec<SpecEntry> {
        rows.iter()
            .map(|(id, epic, prd, title)| SpecEntry {
                id: id.to_string(),
                origin: LoopOrigin {
                    epic: epic.to_string(),
                    prd: prd.to_string(),
                    phase: "phase".to_string(),
                },
                title: title.to_string(),
            })
            .collect()
    }

    fn legacy_with_current(title: &str, epic: Option<&str>, prd: Option<&str>) -> LegacyLoops {
        let hint = match (epic, prd) {
            (Some(e), Some(p)) => Some(LoopOrigin {
                epic: e.to_string(),
                prd: p.to_string(),
                phase: String::new(),
            }),
            _ => None,
        };
        LegacyLoops {
            current: Some(LegacyLoopRecord {
                title: title.to_string(),
                origin_hint: hint,
                status: "in_progress".to_string(),
                started: "2026-07-25".to_string(),
                completed: None,
            }),
            next_steps: Vec::new(),
            history: Vec::new(),
        }
    }

    #[test]
    fn empty_loops_md_yields_empty_preview() {
        let p = build_preview(&LegacyLoops::default(), &spec_of(&[]), now());
        assert!(p.planned.current.is_none());
        assert!(p.planned.history.is_empty());
        assert!(p.planned.queue.is_empty());
        assert!(p.unmatched.is_empty());
        assert!(!p.current_matched);
    }

    #[test]
    fn current_with_backref_matches_triple_exactly() {
        let spec = spec_of(&[
            ("a/promote", "epic-1", "prd-a", "Add the Promote button"),
            ("b/promote", "epic-1", "prd-b", "Add the Promote button"), // same title, other PRD
        ]);
        let legacy = legacy_with_current("Add the Promote button", Some("epic-1"), Some("prd-a"));
        let p = build_preview(&legacy, &spec, now());
        assert!(p.current_matched);
        let current = p.planned.current.unwrap();
        assert_eq!(current.id, "a/promote");
        assert_eq!(current.origin.prd, "prd-a");
    }

    #[test]
    fn ambiguous_title_only_is_unmatched() {
        // No back-ref → title-only; two entries share the exact title → ambiguous.
        let spec = spec_of(&[
            ("a/x", "e", "p1", "Same Title"),
            ("b/x", "e", "p2", "Same Title"),
        ]);
        let legacy = legacy_with_current("Same Title", None, None);
        let p = build_preview(&legacy, &spec, now());
        assert!(!p.current_matched);
        assert_eq!(p.unmatched.len(), 1);
        assert_eq!(p.unmatched[0].section, "current");
        assert_eq!(p.unmatched[0].reason, "ambiguous");
    }

    #[test]
    fn missing_origin_is_unmatched_and_text_preserved() {
        let spec = spec_of(&[("a/x", "e", "p", "Known Title")]);
        let legacy = legacy_with_current("A Title With No Match", None, None);
        let p = build_preview(&legacy, &spec, now());
        assert!(!p.current_matched);
        assert_eq!(p.unmatched.len(), 1);
        assert_eq!(p.unmatched[0].reason, "no match");
        assert_eq!(p.unmatched[0].title, "A Title With No Match");
    }

    #[test]
    fn history_maps_outcome_and_timestamps() {
        let spec = spec_of(&[("a/render", "e", "p", "Render milestones")]);
        let legacy = LegacyLoops {
            current: None,
            next_steps: Vec::new(),
            history: vec![LegacyLoopRecord {
                title: "Render milestones".to_string(),
                origin_hint: None,
                status: "completed".to_string(),
                started: "2026-07-24".to_string(),
                completed: Some("2026-07-24".to_string()),
            }],
        };
        let p = build_preview(&legacy, &spec, now());
        assert_eq!(p.matched_history, 1);
        let h = &p.planned.history[0];
        assert_eq!(h.id, "a/render");
        assert_eq!(h.outcome, Outcome::Completed);
        assert!(h.reason.is_none());
        assert_eq!(h.started_at, h.completed_at);
    }

    #[test]
    fn abandoned_history_gets_reason() {
        let spec = spec_of(&[("a/x", "e", "p", "Abandon me")]);
        let legacy = LegacyLoops {
            current: None,
            next_steps: Vec::new(),
            history: vec![LegacyLoopRecord {
                title: "Abandon me".to_string(),
                origin_hint: None,
                status: "abandoned".to_string(),
                started: "2026-07-20".to_string(),
                completed: Some("2026-07-20".to_string()),
            }],
        };
        let p = build_preview(&legacy, &spec, now());
        let h = &p.planned.history[0];
        assert_eq!(h.outcome, Outcome::Abandoned);
        assert!(h.reason.is_some());
    }

    #[test]
    fn next_step_with_known_id_becomes_queue_entry() {
        let spec = spec_of(&[("a/clobber", "e", "p", "Prevent replacing an active loop")]);
        let legacy = LegacyLoops {
            current: None,
            next_steps: vec![LegacyNextStep {
                text: "`a/clobber` Prevent replacing an active loop".to_string(),
                checked: false,
            }],
            history: Vec::new(),
        };
        let p = build_preview(&legacy, &spec, now());
        assert_eq!(p.planned.queue.len(), 1);
        assert_eq!(p.planned.queue[0].id, "a/clobber");
        assert!(p.unconverted_next_steps.is_empty());
    }

    #[test]
    fn next_step_without_known_id_is_preserved_unconverted() {
        let spec = spec_of(&[("a/x", "e", "p", "Real")]);
        let legacy = LegacyLoops {
            current: None,
            next_steps: vec![
                LegacyNextStep {
                    text: "Some prose next step with no id".to_string(),
                    checked: true,
                },
                LegacyNextStep {
                    text: "`not-an-id` misc".to_string(),
                    checked: false,
                },
            ],
            history: Vec::new(),
        };
        let p = build_preview(&legacy, &spec, now());
        assert!(p.planned.queue.is_empty());
        assert_eq!(p.unconverted_next_steps.len(), 2);
        assert!(p.unconverted_next_steps[0].checked);
    }

    #[test]
    fn duplicate_id_across_records_is_preserved_not_duplicated() {
        // Same title in Current and History → both map to the same ID. Current
        // wins; the history duplicate is preserved as unmatched, never written.
        let spec = spec_of(&[("a/x", "e", "p", "Same Title")]);
        let legacy = LegacyLoops {
            current: Some(LegacyLoopRecord {
                title: "Same Title".to_string(),
                origin_hint: None,
                status: "in_progress".to_string(),
                started: "2026-07-25".to_string(),
                completed: None,
            }),
            next_steps: Vec::new(),
            history: vec![LegacyLoopRecord {
                title: "Same Title".to_string(),
                origin_hint: None,
                status: "completed".to_string(),
                started: "2026-07-24".to_string(),
                completed: Some("2026-07-24".to_string()),
            }],
        };
        let p = build_preview(&legacy, &spec, now());
        assert!(p.current_matched);
        assert!(p.planned.current.is_some());
        // History dup is NOT written (would collide with current's id).
        assert!(p.planned.history.is_empty());
        assert_eq!(p.matched_history, 0);
        assert!(p
            .unmatched
            .iter()
            .any(|u| u.section == "history" && u.reason == "duplicate id"));
        // And the planned state must be valid.
        assert!(p.planned.validate().is_ok());
    }

    #[test]
    fn parse_loops_for_migration_captures_back_refs() {
        let md = "# Loops\n\n## Current\n\n\
- **Goal**: Add the Promote button\n\
- **Status**: in_progress\n\
- **Epic**: support-project-management\n\
- **PRD**: prd-epics-view\n\n\
## Next Steps\n\n- [ ] `a/x` Do a thing\n\n\
## History\n\n### 2026-07-20 — Old Loop\n- **Status**: completed\n";
        let parsed = parse_loops_for_migration(md);
        let current = parsed.current.expect("current parsed");
        assert_eq!(current.title, "Add the Promote button");
        let hint = current.origin_hint.expect("back-ref captured");
        assert_eq!(hint.epic, "support-project-management");
        assert_eq!(hint.prd, "prd-epics-view");
        assert_eq!(parsed.next_steps.len(), 1);
        assert_eq!(parsed.history.len(), 1);
        assert_eq!(parsed.history[0].title, "Old Loop");
    }

    #[test]
    fn dogfood_preview_of_this_repos_loops_md_is_well_formed() {
        // Same dogfood posture as epic::tests / commands::execution::tests:
        // parse this repo's real loops.md + spec layer and assert the preview is
        // structurally sound (valid planned state, every record mapped or
        // surfaced as unmatched — never silently dropped).
        let root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("CARGO_MANIFEST_DIR has a parent (the repo root)");
        let loops_md = memory::loops_md_path(root);
        let content = std::fs::read_to_string(&loops_md).expect("this repo has a loops.md");
        let legacy = parse_loops_for_migration(&content);
        let spec = build_spec_index(root);
        let p = build_preview(&legacy, &spec, now());
        assert!(
            p.planned.validate().is_ok(),
            "migrating this repo must yield a valid ExecutionState"
        );
        // Every legacy history record is accounted for: mapped or surfaced.
        let surfaced_history = p
            .unmatched
            .iter()
            .filter(|u| u.section == "history")
            .count();
        assert_eq!(
            p.matched_history + surfaced_history,
            legacy.history.len(),
            "every history record must be mapped or surfaced, none silently dropped"
        );
    }

    // ── apply (I/O) ───────────────────────────────────────────────────
    //
    // Matching→valid-state is covered by the pure tests + the dogfood test
    // above. These exercise `apply`'s file mechanics: atomic write, rename,
    // idempotency, no-write-on-preview. They run with no `docs/epics/` spec, so
    // every legacy record is unmatched — the planned state is empty, but the
    // file operations are real and identical to the matched case.

    const LEGACY_MD: &str = "# Loops\n\n## Current\n\n\
- **Goal**: Some in-progress loop\n\
- **Status**: in_progress\n\n\
## Next Steps\n\n- [ ] prose step with no id\n\n\
## History\n\n### 2026-07-20 — Old Completed\n- **Status**: completed\n";

    /// A unique temp dir containing a `.loopdeck/loops.md` with `body` (no spec).
    fn temp_repo(name: &str, body: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "loopdeck-migration-{name}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(dir.join(".loopdeck")).unwrap();
        std::fs::write(dir.join(".loopdeck").join("loops.md"), body).unwrap();
        dir
    }

    #[test]
    fn apply_round_trip_writes_and_preserves_original() {
        let dir = temp_repo("round_trip", LEGACY_MD);
        let loops_path = memory::loops_md_path(&dir);
        let exec_path = execution::execution_path(&dir);
        let legacy_path = dir.join(".loopdeck").join("loops.legacy.md");
        let original = std::fs::read(&loops_path).unwrap();

        let state = apply(&dir, now()).expect("apply succeeds");
        // execution.yaml written and reloads to the same state.
        assert!(exec_path.exists());
        assert_eq!(execution::load(&dir).unwrap().state, state);
        // loops.md renamed away; legacy snapshot is byte-identical to the original.
        assert!(!loops_path.exists(), "loops.md should be renamed away");
        assert!(legacy_path.exists(), "loops.legacy.md should exist");
        assert_eq!(std::fs::read(&legacy_path).unwrap(), original);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn apply_is_idempotent() {
        let dir = temp_repo("idempotent", LEGACY_MD);
        let first = apply(&dir, now()).expect("first apply");
        // Second apply: already structured → returns the current state, no error,
        // no second execution.yaml/legacy write beyond what the first did.
        let second = apply(&dir, now()).expect("second apply is a no-op");
        assert_eq!(first, second);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn apply_completes_a_crashed_rename() {
        // Simulate a crash after execution.yaml was written but before the
        // rename: execution.yaml + loops.md present, no loops.legacy.md.
        let dir = temp_repo("crashed", LEGACY_MD);
        let loops_path = memory::loops_md_path(&dir);
        let legacy_path = dir.join(".loopdeck").join("loops.legacy.md");
        execution::save(&dir, &ExecutionState::default(), 0).unwrap();
        assert!(loops_path.exists());
        assert!(!legacy_path.exists());

        apply(&dir, now()).expect("apply finishes the rename");
        assert!(!loops_path.exists(), "rename completed");
        assert!(legacy_path.exists(), "legacy snapshot created");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn preview_writes_nothing() {
        // Computing a preview must not touch the filesystem (criterion 7:
        // cancelling migration produces no filesystem changes).
        let dir = temp_repo("preview", LEGACY_MD);
        let exec_path = execution::execution_path(&dir);
        preview(&dir, now()).expect("preview ok");
        assert!(
            !exec_path.exists(),
            "preview must not create execution.yaml"
        );
        assert!(memory::loops_md_path(&dir).exists(), "loops.md untouched");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn apply_without_loops_md_errors() {
        let dir = std::env::temp_dir().join(format!(
            "loopdeck-migration-empty-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(dir.join(".loopdeck")).unwrap();
        let err = apply(&dir, now()).unwrap_err();
        assert!(matches!(err, AppError::ExecutionState(_)));
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
