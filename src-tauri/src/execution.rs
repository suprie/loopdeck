//! Structured execution state — `.loopdeck/execution.yaml`.
//!
//! Phase 2 of `prd-structured-execution-state.md` (milestone 0.2.1). This is
//! the app-managed structured-state counterpart to the human-authored spec
//! layer ([`crate::epic`]) and the legacy runtime layer ([`crate::memory`] /
//! `loops.md`). One YAML file holds the current loop, the queue, and the
//! completion history, joined to PRD checklist items by a stable loop ID
//! (Phase 1, [`crate::epic::PrdLoop::id`]).
//!
//! This module is the **data layer**: versioned types, validation, pure
//! transitions, and crash-safe persistence with non-destructive recovery. The
//! `commands::execution` IPC layer wraps the transitions; the `loopdeck-state`
//! CLI (for bundled skills/hooks) and the derived-progress UI are later phases.

use crate::error::AppError;
use crate::limits;
use crate::persist;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::path::{Path, PathBuf};

/// Schema version this Selasar writes and reads. A file whose
/// `schema_version` differs is handled read-only — it is never rewritten.
pub const CURRENT_SCHEMA_VERSION: u32 = 1;

fn default_schema_version() -> u32 {
    CURRENT_SCHEMA_VERSION
}

/// Default attempt number for a loop's first run (attempts start at 1).
fn default_attempt() -> u32 {
    1
}

// ── Types ───────────────────────────────────────────────────────────

/// Where a loop originated: the epic / PRD / phase it was promoted from.
/// A navigation back-reference; the stable loop ID is the join key.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LoopOrigin {
    pub epic: String,
    pub prd: String,
    pub phase: String,
}

/// Local Git delivery evidence recorded on completion. Optional — a loop can
/// be `completed` (implemented) before it is `committed`. Phase 6 enriches
/// this with remote provider data behind an optional interface.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct GitEvidence {
    pub commit: String,
}

/// Outcome of a finished loop, recorded in `history`.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Outcome {
    Completed,
    Abandoned,
}

/// The loop currently in progress (`current:`). Membership here *is* the
/// `in_progress` status — there is no separate `status` field to drift from
/// the derived-state table in the PRD.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ActiveLoop {
    pub id: String,
    pub title: String,
    pub origin: LoopOrigin,
    pub started_at: DateTime<Utc>,
    /// Monotonic attempt number for retries (resolved open-question lean: same
    /// logical ID plus an increasing `attempt`). Defaults to 1.
    #[serde(default = "default_attempt")]
    pub attempt: u32,
    /// Delivery links accumulated in flight (branch, rubric result; PR/commit
    /// only appear on the terminal record). `prd-verified-delivery-reconciliation` Phase 1.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub delivery: Option<crate::delivery::DeliveryLinks>,
}

/// A planned-but-not-started loop (`queue:`).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct QueuedLoop {
    pub id: String,
    pub title: String,
    pub origin: LoopOrigin,
    pub queued_at: DateTime<Utc>,
}

/// A finished loop (`history:`), completed or abandoned.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct HistoryLoop {
    pub id: String,
    pub title: String,
    pub origin: LoopOrigin,
    pub outcome: Outcome,
    pub started_at: DateTime<Utc>,
    pub completed_at: DateTime<Utc>,
    #[serde(default = "default_attempt")]
    pub attempt: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub git: Option<GitEvidence>,
    /// Terminal delivery record (branch, commit, PR, rubric) — persisted by
    /// the verified-delivery pipeline only after PR creation succeeds.
    /// Presence never implies completion on its own; the checklist does.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub delivery: Option<crate::delivery::DeliveryLinks>,
    /// Required when `outcome == Abandoned`; absent otherwise.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

/// The full structured execution state — the on-disk shape of
/// `.loopdeck/execution.yaml`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ExecutionState {
    #[serde(default = "default_schema_version")]
    pub schema_version: u32,
    #[serde(default)]
    pub revision: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub current: Option<ActiveLoop>,
    #[serde(default)]
    pub queue: Vec<QueuedLoop>,
    #[serde(default)]
    pub history: Vec<HistoryLoop>,
}

impl Default for ExecutionState {
    /// A fresh, empty state: current schema, no loops, revision 0.
    fn default() -> Self {
        Self {
            schema_version: CURRENT_SCHEMA_VERSION,
            revision: 0,
            current: None,
            queue: Vec::new(),
            history: Vec::new(),
        }
    }
}

// ── Validation ──────────────────────────────────────────────────────

/// One structural problem found in an [`ExecutionState`]. Non-fatal on load
/// (surfaced as a warning so data is never lost); fatal on write.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidationIssue {
    pub kind: &'static str,
    pub message: String,
}

impl std::fmt::Display for ValidationIssue {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl ValidationIssue {
    fn new(kind: &'static str, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
        }
    }
}

impl ExecutionState {
    /// Check the invariants that make an [`ExecutionState`] internally
    /// consistent. Returns every issue found (empty = valid). Does NOT check
    /// the schema version — that is handled separately at load/save because a
    /// version mismatch is a read-only compatibility event, not a structural
    /// defect.
    pub fn validate_invariants(&self) -> Vec<ValidationIssue> {
        let mut issues = Vec::new();

        // Queue: no duplicate IDs.
        let mut queue_seen: HashSet<&str> = HashSet::new();
        for q in &self.queue {
            if !queue_seen.insert(q.id.as_str()) {
                issues.push(ValidationIssue::new(
                    "duplicate_id",
                    format!("loop \"{}\" appears more than once in the queue", q.id),
                ));
            }
        }

        if let Some(current) = &self.current {
            if current.attempt == 0 {
                issues.push(ValidationIssue::new(
                    "bad_attempt",
                    format!(
                        "current loop \"{}\" has attempt 0 (attempts start at 1)",
                        current.id
                    ),
                ));
            }
            // Current must not also be queued.
            if self.queue.iter().any(|q| q.id == current.id) {
                issues.push(ValidationIssue::new(
                    "duplicate_id",
                    format!("loop \"{}\" is both current and queued", current.id),
                ));
            }
            // A completed attempt cannot be re-run: current.attempt must exceed
            // the highest completed attempt for the same ID (retry lean).
            let max_completed = self
                .history
                .iter()
                .filter(|h| h.id == current.id && h.outcome == Outcome::Completed)
                .map(|h| h.attempt)
                .max()
                .unwrap_or(0);
            if current.attempt <= max_completed {
                issues.push(ValidationIssue::new(
                    "duplicate_id",
                    format!(
                        "current loop \"{}\" attempt {} is not a new attempt \
                         (already completed at attempt {})",
                        current.id, current.attempt, max_completed
                    ),
                ));
            }
        }

        // History: no two records for the same (id, attempt) — one attempt
        // resolves once. The same ID with different attempts (a retry) is fine.
        let mut history_seen: HashSet<(&str, u32)> = HashSet::new();
        for h in &self.history {
            if !history_seen.insert((h.id.as_str(), h.attempt)) {
                issues.push(ValidationIssue::new(
                    "duplicate_id",
                    format!(
                        "history records loop \"{}\" attempt {} more than once",
                        h.id, h.attempt
                    ),
                ));
            }
            if h.outcome == Outcome::Abandoned
                && h.reason
                    .as_deref()
                    .map(str::trim)
                    .map(str::is_empty)
                    .unwrap_or(true)
            {
                issues.push(ValidationIssue::new(
                    "missing_reason",
                    format!("abandoned loop \"{}\" has no reason", h.id),
                ));
            }
        }

        issues
    }

    /// Validate for a write: the current schema version AND no invariant
    /// issues. Used by [`save_to_path`] and by the transition helpers before
    /// returning a new state.
    pub fn validate(&self) -> Result<(), AppError> {
        if self.schema_version != CURRENT_SCHEMA_VERSION {
            return Err(AppError::ExecutionState(format!(
                "refusing to write execution state with schema_version {} \
                 (current schema is {CURRENT_SCHEMA_VERSION})",
                self.schema_version
            )));
        }
        let issues = self.validate_invariants();
        if issues.is_empty() {
            return Ok(());
        }
        let detail = issues
            .iter()
            .map(|i| i.message.as_str())
            .collect::<Vec<_>>()
            .join("; ");
        Err(AppError::ExecutionState(format!(
            "execution state is invalid: {detail}"
        )))
    }

    // ── Pure transitions (Phase 3 wraps these in commands / the CLI) ──

    /// Atomically complete the current loop in one pure transition: append a
    /// `Completed` history record, clear `current`, and bump `revision`. When
    /// `promote_first_queued` is set and the queue is non-empty, the first
    /// queued loop is promoted into `current` in the *same* transition, so a
    /// crash can never leave `current` cleared but the next loop un-promoted
    /// (PRD acceptance criterion 2: one atomic write clears Current and
    /// appends History).
    ///
    /// Pure — does not touch disk; pair with [`save`] for durability. The
    /// returned state carries `revision + 1` and is re-validated before return.
    pub fn complete_current(
        &self,
        now: DateTime<Utc>,
        git: Option<GitEvidence>,
        promote_first_queued: bool,
    ) -> Result<ExecutionState, AppError> {
        let active = self.current.as_ref().ok_or_else(|| {
            AppError::Conflict("cannot complete a loop: no loop is in progress".into())
        })?;
        let record = HistoryLoop {
            id: active.id.clone(),
            title: active.title.clone(),
            origin: active.origin.clone(),
            outcome: Outcome::Completed,
            started_at: active.started_at,
            completed_at: now,
            attempt: active.attempt,
            git,
            // The in-flight links (branch, rubric) carry into the terminal
            // delivery record; the caller enriches them (commit, PR URL)
            // before completing whenever the delivery pipeline ran.
            delivery: active.delivery.clone(),
            reason: None,
        };

        let mut next = self.clone();
        next.history.push(record);
        next.current = None;
        if promote_first_queued {
            promote_first(&mut next, now);
        }
        bump_revision(&mut next)?;
        next.validate()?;
        Ok(next)
    }

    /// Abandon the current loop with a required reason. Records an `Abandoned`
    /// history entry, clears `current`, bumps `revision`. Optionally promotes
    /// the first queued loop (mirrors [`Self::complete_current`]).
    pub fn abandon_current(
        &self,
        reason: impl Into<String>,
        now: DateTime<Utc>,
        promote_first_queued: bool,
    ) -> Result<ExecutionState, AppError> {
        let reason = reason.into();
        if reason.trim().is_empty() {
            return Err(AppError::ExecutionState(
                "abandoning a loop requires a non-empty reason".into(),
            ));
        }
        let active = self.current.as_ref().ok_or_else(|| {
            AppError::Conflict("cannot abandon a loop: no loop is in progress".into())
        })?;
        let record = HistoryLoop {
            id: active.id.clone(),
            title: active.title.clone(),
            origin: active.origin.clone(),
            outcome: Outcome::Abandoned,
            started_at: active.started_at,
            completed_at: now,
            attempt: active.attempt,
            git: None,
            // Abandoned attempts keep any links they accumulated (a parked
            // delivery with a failing rubric stays explainable), but never
            // gain commit/PR enrichment.
            delivery: active.delivery.clone(),
            reason: Some(reason),
        };

        let mut next = self.clone();
        next.history.push(record);
        next.current = None;
        if promote_first_queued {
            promote_first(&mut next, now);
        }
        bump_revision(&mut next)?;
        next.validate()?;
        Ok(next)
    }

    /// Promote the first queued loop into `current`. Requires `current` to be
    /// empty — the caller must complete or abandon the in-progress loop first.
    pub fn promote_next_from_queue(&self, now: DateTime<Utc>) -> Result<ExecutionState, AppError> {
        if self.current.is_some() {
            return Err(AppError::Conflict(
                "cannot promote: a loop is already in progress; complete or abandon it first"
                    .into(),
            ));
        }
        if self.queue.is_empty() {
            return Err(AppError::Conflict(
                "cannot promote: the queue is empty".into(),
            ));
        }
        let mut next = self.clone();
        promote_first(&mut next, now);
        bump_revision(&mut next)?;
        next.validate()?;
        Ok(next)
    }

    /// Promote a spec-layer PRD loop (identified by its stable ID) into
    /// `current`. The title and origin come from the spec layer (`epic.rs`),
    /// so the ID is the only identity the caller needs. The attempt number is
    /// computed from history — a retry gets `max-attempt + 1`, including an
    /// abandoned attempt — and
    /// the result is re-validated, so a re-run of a non-retry cannot land.
    /// Refuses if a loop is already in progress.
    pub fn promote_loop_into_current(
        &self,
        id: &str,
        title: &str,
        origin: LoopOrigin,
        now: DateTime<Utc>,
    ) -> Result<ExecutionState, AppError> {
        if self.current.is_some() {
            return Err(AppError::Conflict(
                "cannot promote: a loop is already in progress; complete or abandon it first"
                    .into(),
            ));
        }
        let attempt = self
            .history
            .iter()
            .filter(|h| h.id == id)
            .map(|h| h.attempt)
            .max()
            .unwrap_or(0)
            + 1;
        let mut next = self.clone();
        next.current = Some(ActiveLoop {
            id: id.into(),
            title: title.into(),
            origin,
            started_at: now,
            attempt,
            delivery: None,
        });
        bump_revision(&mut next)?;
        next.validate()?;
        Ok(next)
    }

    /// Retry-recovery completion (`prd-verified-delivery-reconciliation`
    /// Phase 4, `retry-recovery`): a delivery that previously ended
    /// `Abandoned` finished after an idempotent retry once its PR existed.
    /// For each ID: a still-`current` loop gets its in-flight links enriched
    /// (the success-path shape); the newest matching history record flips
    /// `Abandoned` → `Completed` with its reason cleared and the full links
    /// attached; a record already `Completed` only gains missing links.
    /// Idempotent — a second call finds nothing left to change except the
    /// revision, which is only bumped on an actual mutation.
    pub fn record_recovered_delivery(
        &self,
        loop_ids: &[String],
        links: crate::delivery::DeliveryLinks,
        now: DateTime<Utc>,
    ) -> Result<ExecutionState, AppError> {
        let mut next = self.clone();
        let mut changed = false;
        for id in loop_ids {
            if let Some(active) = next.current.as_mut().filter(|current| current.id == *id) {
                active.delivery = Some(links.clone());
                changed = true;
                continue;
            }
            if let Some(record) = next
                .history
                .iter_mut()
                .rev()
                .find(|record| record.id == *id)
            {
                if record.outcome == Outcome::Abandoned {
                    record.outcome = Outcome::Completed;
                    record.reason = None;
                    record.completed_at = now;
                    changed = true;
                }
                if record.delivery.is_none() {
                    record.delivery = Some(links.clone());
                    changed = true;
                }
            }
        }
        if changed {
            bump_revision(&mut next)?;
            next.validate()?;
        }
        Ok(next)
    }
}

/// Move the first queued loop into `current` (attempt 1). No-op on an empty
/// queue. Does not bump the revision — the caller does, once, for the whole
/// transition.
fn promote_first(state: &mut ExecutionState, now: DateTime<Utc>) {
    if let Some(q) = state.queue.first().cloned() {
        state.queue.remove(0);
        state.current = Some(ActiveLoop {
            id: q.id,
            title: q.title,
            origin: q.origin,
            started_at: now,
            attempt: 1,
            delivery: None,
        });
    }
}

fn bump_revision(state: &mut ExecutionState) -> Result<(), AppError> {
    state.revision = state
        .revision
        .checked_add(1)
        .ok_or_else(|| AppError::ExecutionState("execution state revision overflowed".into()))?;
    Ok(())
}

// ── Paths ───────────────────────────────────────────────────────────

/// Path to a project's structured execution state.
pub fn execution_path(repo_path: &Path) -> PathBuf {
    repo_path.join(".loopdeck").join("execution.yaml")
}

/// Sibling last-known-good backup path (`execution.yaml.bak`), mirroring
/// [`crate::config`]'s registry backup.
fn backup_path(primary: &Path) -> PathBuf {
    let mut name = primary
        .file_name()
        .expect("execution-state path has a file name")
        .to_os_string();
    name.push(".bak");
    primary.with_file_name(name)
}

// ── Load (non-destructive recovery) ─────────────────────────────────

/// Where a loaded [`ExecutionState`] came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum LoadSource {
    /// No `execution.yaml` yet — a fresh default state (nothing on disk).
    Default,
    /// Read from the primary `execution.yaml`.
    Primary,
    /// The primary was malformed; the last-known-good `.bak` was loaded
    /// instead. The primary is preserved untouched for manual recovery.
    BackupRecovered,
}

/// A loaded execution state plus its provenance and any non-fatal warnings.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct LoadedExecution {
    pub state: ExecutionState,
    pub source: LoadSource,
    pub warnings: Vec<String>,
    /// Whether `execution.yaml` exists on disk (vs a fresh default synthesized
    /// for a missing file). Lets the UI branch on structured-vs-legacy mode
    /// without a separate probe (0.2.1 Phase 4 migration surface). Mirrors
    /// `source != LoadSource::Default`, exposed as a plain bool for the wire.
    pub file_present: bool,
}

/// Load `.loopdeck/execution.yaml` for `repo_path`. See [`load_from_path`].
pub fn load(repo_path: &Path) -> Result<LoadedExecution, AppError> {
    load_from_path(&execution_path(repo_path))
}

/// Load from an explicit path with non-destructive recovery:
///
/// 1. Missing primary → fresh default.
/// 2. Primary parses + version ok → load it; structural issues are warnings.
/// 3. Primary parses but the version is newer/older → read-only error; the
///    file is never rewritten.
/// 4. Primary malformed → try the `.bak`; if it parses, load it read-only and
///    warn. The malformed primary is preserved.
/// 5. Primary malformed and no usable backup → error; primary preserved.
pub fn load_from_path(path: &Path) -> Result<LoadedExecution, AppError> {
    if !path.exists() {
        return Ok(LoadedExecution {
            state: ExecutionState::default(),
            source: LoadSource::Default,
            warnings: Vec::new(),
            file_present: false,
        });
    }

    let contents = limits::read_bounded_to_string(path, limits::EXECUTION_MAX_BYTES)?;
    match serde_yaml::from_str::<ExecutionState>(&contents) {
        Ok(state) => {
            // Version compatibility is a hard, read-only event — distinct
            // from structural defects, which are non-fatal warnings.
            check_schema_version(state.schema_version, path)?;
            let warnings = state
                .validate_invariants()
                .iter()
                .map(|i| i.message.clone())
                .collect();
            Ok(LoadedExecution {
                state,
                source: LoadSource::Primary,
                warnings,
                file_present: true,
            })
        }
        Err(primary_err) => {
            tracing::warn!(
                "malformed execution state at {}: {primary_err}",
                path.display()
            );
            match recover_from_backup(path) {
                Ok(loaded) => Ok(loaded),
                Err(()) => Err(AppError::ExecutionState(format!(
                    "execution state at {} is malformed and no valid backup was found; \
                     the file has been preserved for manual recovery. Parse error: {primary_err}",
                    path.display()
                ))),
            }
        }
    }
}

/// Try the `.bak` for a malformed primary. On success returns a
/// [`LoadedExecution`] sourced `BackupRecovered` with a leading warning. On
/// any failure returns `Err(())` so the caller composes its own message and
/// preserves the primary.
fn recover_from_backup(path: &Path) -> Result<LoadedExecution, ()> {
    let backup = backup_path(path);
    let backup_contents = match limits::read_bounded_to_string(&backup, limits::EXECUTION_MAX_BYTES)
    {
        Ok(c) => c,
        Err(e) => {
            tracing::warn!("backup at {} unreadable: {e}", backup.display());
            return Err(());
        }
    };
    let state = match serde_yaml::from_str::<ExecutionState>(&backup_contents) {
        Ok(s) => s,
        Err(backup_err) => {
            tracing::warn!(
                "backup at {} also malformed: {backup_err}",
                backup.display()
            );
            return Err(());
        }
    };
    // A backup with an incompatible version is not a usable recovery.
    if let Err(e) = check_schema_version(state.schema_version, path) {
        tracing::warn!("backup at {} cannot be recovered: {e}", backup.display());
        return Err(());
    }
    let mut warnings: Vec<String> = state
        .validate_invariants()
        .iter()
        .map(|i| i.message.clone())
        .collect();
    warnings.insert(
        0,
        format!(
            "primary execution state at {} is malformed; loaded the last-known-good \
             backup instead. The malformed primary was preserved for manual recovery.",
            path.display()
        ),
    );
    Ok(LoadedExecution {
        state,
        source: LoadSource::BackupRecovered,
        warnings,
        file_present: true,
    })
}

/// Read-only schema-version gate. A newer schema must never be rewritten; an
/// older one is not auto-migrated in Phase 2 (migration from legacy
/// `loops.md` arrives in Phase 4).
fn check_schema_version(version: u32, path: &Path) -> Result<(), AppError> {
    if version == CURRENT_SCHEMA_VERSION {
        return Ok(());
    }
    let where_ = path.display();
    if version > CURRENT_SCHEMA_VERSION {
        Err(AppError::ExecutionState(format!(
            "execution state at {where_} uses schema_version {version}, newer than this \
             Selasar supports ({CURRENT_SCHEMA_VERSION}). The file was not modified — \
             upgrade Selasar to read it."
        )))
    } else {
        Err(AppError::ExecutionState(format!(
            "execution state at {where_} uses schema_version {version}, older than the \
             current schema ({CURRENT_SCHEMA_VERSION}). The file was not modified — \
             migration from a legacy schema is not supported in this version."
        )))
    }
}

// ── Save (validate + OCC + backup + atomic write) ───────────────────

/// Persist `new_state` to `.loopdeck/execution.yaml` for `repo_path`. See
/// [`save_to_path`].
pub fn save(
    repo_path: &Path,
    new_state: &ExecutionState,
    expected_revision: u64,
) -> Result<(), AppError> {
    save_to_path(&execution_path(repo_path), new_state, expected_revision)
}

/// Atomically write `new_state` after validating it and confirming the on-disk
/// revision still matches `expected_revision` (optimistic concurrency: a stale
/// UI action cannot silently overwrite a newer state — PRD acceptance
/// criterion 3). Before writing, the existing primary is copied to `.bak` as a
/// last-known-good recovery floor.
///
/// A malformed or version-incompatible primary is never overwritten here —
/// that is the load/recovery path's job. This refuses to write against a
/// primary it cannot read a revision from.
pub fn save_to_path(
    path: &Path,
    new_state: &ExecutionState,
    expected_revision: u64,
) -> Result<(), AppError> {
    // 1. Validate before any I/O — never persist an invalid state.
    new_state.validate()?;

    // 2. OCC: the on-disk revision must match what the caller read.
    let on_disk_revision = read_on_disk_revision(path)?;
    if on_disk_revision != expected_revision {
        return Err(AppError::Conflict(format!(
            "execution state changed before this write (expected revision \
             {expected_revision}, found {on_disk_revision} on disk); reload and retry."
        )));
    }

    // 3. Best-effort last-known-good backup of the current primary.
    if path.exists() {
        let backup = backup_path(path);
        if let Err(e) = std::fs::copy(path, &backup) {
            tracing::warn!(
                "failed to update execution-state backup at {}: {e}",
                backup.display()
            );
        }
    }

    // 4. Atomic write (temp + fsync + same-dir rename).
    let contents = serde_yaml::to_string(new_state)?;
    persist::atomic_write(path, &contents)?;
    Ok(())
}

/// The revision currently on disk, or `0` if there is no primary yet. A
/// primary that cannot be parsed is treated as unrecoverable-for-write: the
/// caller must repair it via the load path rather than overwrite it blind.
fn read_on_disk_revision(path: &Path) -> Result<u64, AppError> {
    if !path.exists() {
        return Ok(0);
    }
    let contents = limits::read_bounded_to_string(path, limits::EXECUTION_MAX_BYTES)?;
    #[derive(Deserialize)]
    struct RevisionProbe {
        #[serde(default)]
        revision: u64,
    }
    let probe: RevisionProbe = serde_yaml::from_str(&contents).map_err(|e| {
        AppError::ExecutionState(format!(
            "cannot verify revision — execution state at {} is malformed; \
             recover it before writing. Parse error: {e}",
            path.display()
        ))
    })?;
    Ok(probe.revision)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn ts(s: &str) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(s).unwrap().with_timezone(&Utc)
    }

    /// Unique temp dir per test so parallel runs never race on a shared parent.
    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "loopdeck-execution-{name}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn origin(prd: &str) -> LoopOrigin {
        LoopOrigin {
            epic: "support-project-management".into(),
            prd: prd.into(),
            phase: "phase-1".into(),
        }
    }

    fn active(id: &str, attempt: u32) -> ActiveLoop {
        ActiveLoop {
            id: id.into(),
            title: format!("title-{id}"),
            origin: origin("prd-x"),
            started_at: ts("2026-07-25T09:00:00+00:00"),
            attempt,
            delivery: None,
        }
    }

    fn queued(id: &str) -> QueuedLoop {
        QueuedLoop {
            id: id.into(),
            title: format!("title-{id}"),
            origin: origin("prd-x"),
            queued_at: ts("2026-07-25T08:00:00+00:00"),
        }
    }

    // ── types / default ──

    #[test]
    fn default_state_is_empty_v1() {
        let s = ExecutionState::default();
        assert_eq!(s.schema_version, CURRENT_SCHEMA_VERSION);
        assert_eq!(s.revision, 0);
        assert!(s.current.is_none());
        assert!(s.queue.is_empty());
        assert!(s.history.is_empty());
        assert!(s.validate_invariants().is_empty());
    }

    #[test]
    fn round_trip_preserves_state() {
        let dir = temp_dir("round_trip");
        let path = execution_path(&dir);
        let mut state = ExecutionState::default();
        state.current = Some(active("prd-x/loop-a", 1));
        state.queue = vec![queued("prd-x/loop-b"), queued("prd-x/loop-c")];
        state.revision = 3;

        save_to_path(&path, &state, 0).unwrap();
        // OCC: a second write must use the revision now on disk (3).
        let loaded = load_from_path(&path).unwrap();
        assert_eq!(loaded.source, LoadSource::Primary);
        assert!(loaded.warnings.is_empty());
        assert_eq!(loaded.state, state);
        assert_eq!(loaded.state.revision, 3);
        fs::remove_dir_all(&dir).unwrap();
    }

    // ── OCC ──

    #[test]
    fn stale_writer_is_rejected_by_revision_check() {
        let dir = temp_dir("occ");
        let path = execution_path(&dir);
        let mut s1 = ExecutionState::default();
        s1.current = Some(active("prd/loop", 1));
        save_to_path(&path, &s1, 0).unwrap();

        // A real transition bumps the on-disk revision 0 -> 1.
        let loaded = load_from_path(&path).unwrap();
        let completed = loaded
            .state
            .complete_current(ts("2026-07-25T12:00:00+00:00"), None, false)
            .unwrap();
        save_to_path(&path, &completed, loaded.state.revision).unwrap();

        // A stale writer that still expects revision 0 must be rejected.
        let stale = ExecutionState::default();
        let err = save_to_path(&path, &stale, 0).unwrap_err();
        assert!(
            matches!(err, AppError::Conflict(_)),
            "stale writer must conflict, got {err:?}"
        );
        fs::remove_dir_all(&dir).unwrap();
    }

    // ── validation ──

    #[test]
    fn duplicate_queue_id_is_invalid() {
        let mut s = ExecutionState::default();
        s.queue = vec![queued("prd/dup"), queued("prd/dup")];
        let issues = s.validate_invariants();
        assert!(issues
            .iter()
            .any(|i| i.kind == "duplicate_id" && i.message.contains("queue")));
        assert!(s.validate().is_err());
    }

    #[test]
    fn current_in_queue_is_invalid() {
        let mut s = ExecutionState::default();
        s.current = Some(active("prd/both", 1));
        s.queue = vec![queued("prd/both")];
        assert!(s.validate().is_err());
    }

    #[test]
    fn completed_attempt_cannot_rerun_but_higher_attempt_can() {
        let mut s = ExecutionState::default();
        s.history.push(HistoryLoop {
            id: "prd/retry".into(),
            title: "t".into(),
            origin: origin("prd-x"),
            outcome: Outcome::Completed,
            started_at: ts("2026-07-24T09:00:00+00:00"),
            completed_at: ts("2026-07-24T12:00:00+00:00"),
            attempt: 1,
            git: None,
            delivery: None,
            reason: None,
        });
        // Re-running attempt 1 is invalid.
        s.current = Some(active("prd/retry", 1));
        assert!(s.validate().is_err());
        // A fresh attempt (2) is a valid retry.
        s.current = Some(active("prd/retry", 2));
        assert!(s.validate().is_ok());
    }

    #[test]
    fn duplicate_history_attempt_is_invalid() {
        let rec = HistoryLoop {
            id: "prd/h".into(),
            title: "t".into(),
            origin: origin("prd-x"),
            outcome: Outcome::Completed,
            started_at: ts("2026-07-24T09:00:00+00:00"),
            completed_at: ts("2026-07-24T12:00:00+00:00"),
            attempt: 1,
            git: None,
            delivery: None,
            reason: None,
        };
        let mut s = ExecutionState::default();
        s.history.push(rec.clone());
        s.history.push(rec);
        assert!(s.validate().is_err());
    }

    #[test]
    fn abandoned_without_reason_is_invalid() {
        let mut s = ExecutionState::default();
        s.history.push(HistoryLoop {
            id: "prd/a".into(),
            title: "t".into(),
            origin: origin("prd-x"),
            outcome: Outcome::Abandoned,
            started_at: ts("2026-07-24T09:00:00+00:00"),
            completed_at: ts("2026-07-24T12:00:00+00:00"),
            attempt: 1,
            git: None,
            delivery: None,
            reason: None,
        });
        assert!(s.validate().is_err());
    }

    // ── version compatibility ──

    #[test]
    fn newer_schema_version_loads_read_only_and_preserves_file() {
        let dir = temp_dir("newer_version");
        let path = execution_path(&dir);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, "schema_version: 99\nrevision: 1\n").unwrap();
        let before = fs::read_to_string(&path).unwrap();

        let err = load_from_path(&path).unwrap_err();
        assert!(matches!(err, AppError::ExecutionState(_)));
        assert!(err.to_string().to_lowercase().contains("newer"));
        // The file is untouched — never rewritten.
        assert_eq!(fs::read_to_string(&path).unwrap(), before);
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn older_schema_version_loads_read_only() {
        let dir = temp_dir("older_version");
        let path = execution_path(&dir);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, "schema_version: 0\nrevision: 0\n").unwrap();

        let err = load_from_path(&path).unwrap_err();
        assert!(matches!(err, AppError::ExecutionState(_)));
        let msg = err.to_string().to_lowercase();
        assert!(msg.contains("older") || msg.contains("migration"));
        fs::remove_dir_all(&dir).unwrap();
    }

    // ── recovery ──

    #[test]
    fn malformed_primary_recovers_from_backup_and_preserves_primary() {
        let dir = temp_dir("recover_backup");
        let path = execution_path(&dir);
        let backup = backup_path(&path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();

        // A valid last-known-good backup on disk.
        let good = ExecutionState {
            schema_version: CURRENT_SCHEMA_VERSION,
            revision: 7,
            current: None,
            queue: vec![queued("prd/q")],
            history: vec![],
        };
        fs::write(&backup, serde_yaml::to_string(&good).unwrap()).unwrap();
        // A garbled primary.
        fs::write(&path, "schema_version: : :\n  not: [valid yaml").unwrap();
        let before = fs::read_to_string(&path).unwrap();

        let loaded = load_from_path(&path).unwrap();
        assert_eq!(loaded.source, LoadSource::BackupRecovered);
        assert!(loaded.warnings[0].to_lowercase().contains("malformed"));
        assert_eq!(loaded.state.revision, 7);
        // The malformed primary is preserved untouched.
        assert_eq!(fs::read_to_string(&path).unwrap(), before);
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn malformed_primary_without_backup_errors_and_preserves_file() {
        let dir = temp_dir("no_backup");
        let path = execution_path(&dir);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, "::: not yaml :::").unwrap();
        let before = fs::read_to_string(&path).unwrap();

        let err = load_from_path(&path).unwrap_err();
        assert!(matches!(err, AppError::ExecutionState(_)));
        assert_eq!(fs::read_to_string(&path).unwrap(), before);
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn missing_file_loads_default() {
        let dir = temp_dir("missing");
        let path = execution_path(&dir);
        let loaded = load_from_path(&path).unwrap();
        assert_eq!(loaded.source, LoadSource::Default);
        assert_eq!(loaded.state, ExecutionState::default());
        assert!(loaded.warnings.is_empty());
        fs::remove_dir_all(&dir).unwrap();
    }

    // ── transitions ──

    #[test]
    fn complete_current_is_atomic_clear_append_bump() {
        let mut s = ExecutionState::default();
        s.current = Some(active("prd/loop", 1));
        let now = ts("2026-07-25T12:00:00+00:00");

        let next = s.complete_current(now, None, false).unwrap();
        assert!(next.current.is_none());
        assert_eq!(next.history.len(), 1);
        assert_eq!(next.history[0].outcome, Outcome::Completed);
        assert_eq!(next.history[0].id, "prd/loop");
        assert_eq!(next.history[0].completed_at, now);
        assert_eq!(next.revision, s.revision + 1);
        // Pure transform — the input is untouched.
        assert!(s.current.is_some());
    }

    #[test]
    fn complete_current_can_promote_first_queued() {
        let mut s = ExecutionState::default();
        s.current = Some(active("prd/a", 1));
        s.queue = vec![queued("prd/b"), queued("prd/c")];
        let now = ts("2026-07-25T12:00:00+00:00");

        let next = s.complete_current(now, None, true).unwrap();
        assert_eq!(next.current.as_ref().unwrap().id, "prd/b");
        assert_eq!(next.queue.len(), 1);
        assert_eq!(next.queue[0].id, "prd/c");
        assert_eq!(next.history.len(), 1);
        assert_eq!(next.history[0].id, "prd/a");
    }

    #[test]
    fn complete_without_current_errors() {
        let s = ExecutionState::default();
        let err = s
            .complete_current(ts("2026-07-25T12:00:00+00:00"), None, false)
            .unwrap_err();
        assert!(matches!(err, AppError::Conflict(_)));
    }

    #[test]
    fn abandon_requires_reason_and_records_outcome() {
        let mut s = ExecutionState::default();
        s.current = Some(active("prd/loop", 1));
        let now = ts("2026-07-25T12:00:00+00:00");

        assert!(s.abandon_current("   ", now, false).is_err());
        let next = s
            .abandon_current("blocked on upstream api", now, false)
            .unwrap();
        assert!(next.current.is_none());
        assert_eq!(next.history[0].outcome, Outcome::Abandoned);
        assert_eq!(
            next.history[0].reason.as_deref(),
            Some("blocked on upstream api")
        );
        assert_eq!(next.revision, s.revision + 1);
    }

    #[test]
    fn promote_next_from_queue_moves_first_into_current() {
        let mut s = ExecutionState::default();
        s.queue = vec![queued("prd/a"), queued("prd/b")];
        let now = ts("2026-07-25T12:00:00+00:00");

        let next = s.promote_next_from_queue(now).unwrap();
        assert_eq!(next.current.as_ref().unwrap().id, "prd/a");
        assert_eq!(next.queue.len(), 1);
        assert_eq!(next.revision, s.revision + 1);
    }

    #[test]
    fn promote_blocked_when_current_set_or_queue_empty() {
        let now = ts("2026-07-25T12:00:00+00:00");
        let mut s = ExecutionState::default();
        s.current = Some(active("prd/a", 1));
        s.queue = vec![queued("prd/b")];
        assert!(matches!(
            s.promote_next_from_queue(now).unwrap_err(),
            AppError::Conflict(_)
        ));
        let empty = ExecutionState::default();
        assert!(matches!(
            empty.promote_next_from_queue(now).unwrap_err(),
            AppError::Conflict(_)
        ));
    }

    #[test]
    fn retry_after_abandonment_uses_the_next_attempt() {
        let mut s = ExecutionState::default();
        s.current = Some(active("prd/loop", 1));
        let abandoned = s
            .abandon_current(
                "agent session ended",
                ts("2026-07-25T12:00:00+00:00"),
                false,
            )
            .unwrap();

        let retried = abandoned
            .promote_loop_into_current(
                "prd/loop",
                "Retryable loop",
                origin("prd/loop"),
                ts("2026-07-25T13:00:00+00:00"),
            )
            .unwrap();

        assert_eq!(retried.current.as_ref().unwrap().attempt, 2);
        assert!(retried.validate().is_ok());
    }

    // ── save guards ──

    #[test]
    fn save_refuses_invalid_state_and_writes_nothing() {
        let dir = temp_dir("save_invalid");
        let path = execution_path(&dir);
        let mut s = ExecutionState::default();
        s.queue = vec![queued("prd/dup"), queued("prd/dup")];

        let err = save_to_path(&path, &s, 0).unwrap_err();
        assert!(matches!(err, AppError::ExecutionState(_)));
        assert!(!path.exists(), "an invalid state must not be written");
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn save_refuses_wrong_schema_version() {
        let dir = temp_dir("save_wrong_version");
        let path = execution_path(&dir);
        let mut s = ExecutionState::default();
        s.schema_version = 2;
        assert!(matches!(
            save_to_path(&path, &s, 0).unwrap_err(),
            AppError::ExecutionState(_)
        ));
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn save_against_malformed_primary_is_refused() {
        let dir = temp_dir("save_against_malformed");
        let path = execution_path(&dir);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, "::: garbage :::").unwrap();

        let s = ExecutionState::default();
        let err = save_to_path(&path, &s, 0).unwrap_err();
        assert!(matches!(err, AppError::ExecutionState(_)));
        // The malformed primary is preserved, not overwritten.
        assert_eq!(fs::read_to_string(&path).unwrap(), "::: garbage :::");
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn save_creates_backup_before_overwrite() {
        let dir = temp_dir("save_backup");
        let path = execution_path(&dir);
        let backup = backup_path(&path);

        let mut s = ExecutionState::default();
        s.current = Some(active("prd/a", 1));
        save_to_path(&path, &s, 0).unwrap();
        assert!(path.exists());

        // A transition + second save must leave a last-known-good .bak.
        let loaded = load_from_path(&path).unwrap();
        let next = loaded
            .state
            .complete_current(ts("2026-07-25T12:00:00+00:00"), None, false)
            .unwrap();
        save_to_path(&path, &next, loaded.state.revision).unwrap();
        assert!(
            backup.exists(),
            "a last-known-good backup must exist after overwrite"
        );
        fs::remove_dir_all(&dir).unwrap();
    }
}
