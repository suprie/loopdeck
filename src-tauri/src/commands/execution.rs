//! Structured execution-state commands (milestone 0.2.1, Phase 3).
//!
//! Bridges the IPC layer to the `execution` data layer (Phase 2). These are
//! the validated write paths for `.loopdeck/execution.yaml` — every transition
//! goes through the Rust domain (validate → optimistic-concurrency check →
//! backup → atomic write), never a free-form file edit. The `loopdeck-state`
//! CLI (also Phase 3) wraps the same domain for bundled skills/hooks; the
//! derived-progress UI is Phase 5.
//!
//! Named `execution` to avoid clashing with the agent run-state `commands::state`.

use super::state::{resolve_root, AppState};
use crate::epic;
use crate::error::AppError;
use crate::execution::{self, ExecutionState, GitEvidence, LoadedExecution, LoopOrigin};
use crate::migration::{self, MigrationPreview};
use chrono::Utc;
use std::path::Path;
use tauri::State;
use tracing::debug;

/// Read `.loopdeck/execution.yaml` for a project. Non-destructive: a malformed
/// primary is recovered from the `.bak` read-only and surfaced as a warning
/// (`source: BackupRecovered`); a missing file returns a fresh default.
#[tauri::command]
pub async fn get_execution_state(
    path: String,
    state: State<'_, AppState>,
) -> Result<LoadedExecution, AppError> {
    debug!("get_execution_state called for path: {path}");
    let root = resolve_root(&state, &path)?;
    execution::load(&root)
}

/// Promote a PRD checklist loop (by stable ID) into `current`. The loop's title
/// and spec origin are read from `docs/epics/`, so the ID is the only identity
/// the caller supplies. Refuses if a loop is already in progress. A retry of a
/// previously-completed ID gets the next attempt number automatically.
#[tauri::command]
pub async fn promote_loop_by_id(
    path: String,
    loop_id: String,
    state: State<'_, AppState>,
) -> Result<ExecutionState, AppError> {
    debug!("promote_loop_by_id called for path: {path}, id: {loop_id}");
    let root = resolve_root(&state, &path)?;

    let found = find_prd_loop(&root, &loop_id)?;
    let loaded = execution::load(&root)?;
    let next =
        loaded
            .state
            .promote_loop_into_current(&loop_id, &found.title, found.origin, Utc::now())?;
    execution::save(&root, &next, loaded.state.revision)?;
    Ok(next)
}

/// Complete the current loop, optionally recording a verified local commit SHA
/// as Git evidence. Atomically clears `current`, appends a `Completed` history
/// record, and bumps the revision. When `promote_next` is set, the first queued
/// loop is promoted into `current` in the same atomic write.
#[tauri::command]
pub async fn complete_current_loop(
    path: String,
    commit: Option<String>,
    promote_next: bool,
    state: State<'_, AppState>,
) -> Result<ExecutionState, AppError> {
    debug!("complete_current_loop called for path: {path}, commit: {commit:?}");
    let root = resolve_root(&state, &path)?;
    let loaded = execution::load(&root)?;
    let git = commit.map(|sha| GitEvidence { commit: sha });
    let next = loaded
        .state
        .complete_current(Utc::now(), git, promote_next)?;
    execution::save(&root, &next, loaded.state.revision)?;
    Ok(next)
}

/// Abandon the current loop with a required reason. Atomically clears
/// `current`, appends an `Abandoned` history record (carrying the reason), and
/// bumps the revision.
#[tauri::command]
pub async fn abandon_current_loop(
    path: String,
    reason: String,
    promote_next: bool,
    state: State<'_, AppState>,
) -> Result<ExecutionState, AppError> {
    debug!("abandon_current_loop called for path: {path}");
    let root = resolve_root(&state, &path)?;
    let loaded = execution::load(&root)?;
    let next = loaded
        .state
        .abandon_current(reason, Utc::now(), promote_next)?;
    execution::save(&root, &next, loaded.state.revision)?;
    Ok(next)
}

/// Promote the first queued loop into `current` (requires `current` to be
/// empty — complete or abandon the in-progress loop first). Bumps the revision.
#[tauri::command]
pub async fn promote_next_queued_loop(
    path: String,
    state: State<'_, AppState>,
) -> Result<ExecutionState, AppError> {
    debug!("promote_next_queued_loop called for path: {path}");
    let root = resolve_root(&state, &path)?;
    let loaded = execution::load(&root)?;
    let next = loaded.state.promote_next_from_queue(Utc::now())?;
    execution::save(&root, &next, loaded.state.revision)?;
    Ok(next)
}

/// Compute a read-only migration preview for a project still on legacy
/// `.loopdeck/loops.md` (Phase 4). Returns the planned `execution.yaml`, the
/// records that map to stable IDs, and every unmatched/ambiguous record +
/// unconverted next-step (preserved verbatim — never fuzzy-matched). Writes
/// nothing. `available()` is false when `execution.yaml` already exists.
#[tauri::command]
pub async fn get_migration_preview(
    path: String,
    state: State<'_, AppState>,
) -> Result<MigrationPreview, AppError> {
    debug!("get_migration_preview called for path: {path}");
    let root = resolve_root(&state, &path)?;
    migration::preview(&root, Utc::now())
}

/// Perform the confirmed legacy→structured migration (Phase 4). Recomputes the
/// preview server-side, validates, atomically writes `execution.yaml`, then
/// renames `loops.md` → `loops.legacy.md` (preserving the original). Idempotent
/// against a crash between the two writes. Returns the new structured state.
#[tauri::command]
pub async fn apply_migration(
    path: String,
    state: State<'_, AppState>,
) -> Result<ExecutionState, AppError> {
    debug!("apply_migration called for path: {path}");
    let root = resolve_root(&state, &path)?;
    migration::apply(&root, Utc::now())
}

/// A resolved PRD loop: its display title + spec origin.
#[derive(Debug)]
struct PrdLoopRef {
    title: String,
    origin: LoopOrigin,
}

/// Find a PRD checklist loop by stable ID, returning its title and spec origin.
/// Thin wrapper over [`epic::find_loop_by_id`] (the single spec-layer lookup,
/// shared with the `loopdeck state` CLI) that maps the spec-native location to
/// the runtime `LoopOrigin`.
fn find_prd_loop(root: &Path, loop_id: &str) -> Result<PrdLoopRef, AppError> {
    let loc = epic::find_loop_by_id(root, loop_id).ok_or_else(|| {
        AppError::ProjectNotFound(format!(
            "no PRD checklist loop with id \"{loop_id}\" found under docs/epics/"
        ))
    })?;
    Ok(PrdLoopRef {
        title: loc.title,
        origin: LoopOrigin {
            epic: loc.epic,
            prd: loc.prd,
            phase: loc.phase,
        },
    })
}

#[cfg(test)]
mod tests {
    //! `find_prd_loop` is the one piece of command-layer logic not already
    //! covered by the `execution` module's tests. It is exercised against this
    //! repo's own spec layer — the same dogfood approach as
    //! `epic::tests::test_dogfood_parses_this_repos_epic`.
    use super::*;
    use std::path::Path;

    fn repo_root() -> &'static Path {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("CARGO_MANIFEST_DIR should have a parent (the repo root)")
    }

    #[test]
    fn find_prd_loop_resolves_real_spec_id() {
        // A stable ID authored by Phase 1 in the 0.2.1 PRD's Phase 1 checklist.
        let found = find_prd_loop(repo_root(), "structured-state/parse-loop-id")
            .expect("this repo's 0.2.1 PRD has the parse-loop-id checklist item");
        assert_eq!(found.origin.epic, "support-project-management");
        assert_eq!(found.origin.prd, "prd-structured-execution-state");
        assert!(!found.title.is_empty());
    }

    #[test]
    fn find_prd_loop_errors_on_unknown_id() {
        let err = find_prd_loop(repo_root(), "does/not-exist").unwrap_err();
        assert!(matches!(err, AppError::ProjectNotFound(_)));
    }
}
