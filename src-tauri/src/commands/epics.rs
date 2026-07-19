//! Epic / spec / loop-status read commands. Bridges the IPC layer to the
//! `epic`, `memory`, and (for decisions/loops) the `.loopdeck/` parsers.
//! Named `epics` (plural) to avoid clashing with the top-level `epic` module.

use super::state::{resolve_root, AppState};
use crate::epic::{self, Epic};
use crate::error::AppError;
use crate::memory::{self, Decision, LoopStatus};
use tauri::State;
use tracing::debug;

/// Get all decisions from `.loopdeck/decisions.md`.
/// Returns an empty list if the file does not exist.
#[tauri::command]
pub async fn get_decisions(
    path: String,
    state: State<'_, AppState>,
) -> Result<Vec<Decision>, AppError> {
    debug!("get_decisions called for path: {path}");

    let root = resolve_root(&state, &path)?;
    Ok(memory::parse_decisions(&root))
}

/// Get loop status from `.loopdeck/loops.md`.
/// Returns an empty/default LoopStatus if the file does not exist.
#[tauri::command]
pub async fn get_loops(path: String, state: State<'_, AppState>) -> Result<LoopStatus, AppError> {
    debug!("get_loops called for path: {path}");

    let root = resolve_root(&state, &path)?;
    Ok(memory::parse_loops(&root))
}

/// Get all epics from `docs/epics/`, each with its PRDs and phase checklists.
/// Returns an empty list if `docs/epics/` does not exist.
#[tauri::command]
pub async fn get_epics(path: String, state: State<'_, AppState>) -> Result<Vec<Epic>, AppError> {
    debug!("get_epics called for path: {path}");

    let root = resolve_root(&state, &path)?;
    Ok(epic::parse_epics(&root))
}

/// Get epics grouped by milestone (ordered), for the cross-project `/epics` view.
/// Epics with no milestone land in an "Unmilestoned" bucket.
#[tauri::command]
pub async fn get_epics_by_milestone(
    path: String,
    state: State<'_, AppState>,
) -> Result<std::collections::BTreeMap<String, Vec<Epic>>, AppError> {
    debug!("get_epics_by_milestone called for path: {path}");

    let root = resolve_root(&state, &path)?;
    Ok(epic::epics_by_milestone(&root))
}

/// Promote a PRD checklist item into `.loopdeck/loops.md ## Current`.
///
/// Writes the loop with `**Epic**` / `**PRD**` back-reference bullets. Refuses
/// to clobber a non-empty `## Current` — the caller must complete or abandon
/// the current loop first.
#[tauri::command]
pub async fn promote_epic_loop(
    path: String,
    epic_slug: String,
    prd_filename: String,
    loop_title: String,
    state: State<'_, AppState>,
) -> Result<(), AppError> {
    debug!("promote_epic_loop called for path: {path}, epic: {epic_slug}, prd: {prd_filename}");

    // Resolve the canonical, registered root (PRD FR3) before rewriting
    // `.loopdeck/loops.md`.
    let root = resolve_root(&state, &path)?;
    epic::promote_epic_loop(&root, &epic_slug, &prd_filename, &loop_title)?;
    Ok(())
}

/// Toggle a `- [ ]` / `- [x]` next-step checklist item in `.loopdeck/loops.md`.
/// Returns the new checked state.
#[tauri::command]
pub async fn toggle_loop_step(
    path: String,
    step_text: String,
    state: State<'_, AppState>,
) -> Result<bool, AppError> {
    debug!("toggle_loop_step called for path: {path}");

    let root = resolve_root(&state, &path)?;
    let now_checked = memory::toggle_loop_step(&root, &step_text)?;
    Ok(now_checked)
}

/// Toggle a `- [ ]` / `- [x]` checklist item in a PRD file under docs/epics/.
/// Returns the new checked state.
#[tauri::command]
pub async fn toggle_prd_loop(
    path: String,
    epic_slug: String,
    prd_filename: String,
    loop_title: String,
    state: State<'_, AppState>,
) -> Result<bool, AppError> {
    debug!("toggle_prd_loop called for path: {path}, epic: {epic_slug}, prd: {prd_filename}");

    // Resolve the canonical, registered root (PRD FR3). `epic_slug` /
    // `prd_filename` are additionally sandboxed to `docs/epics/` inside
    // `epic::toggle_prd_loop` via the shared `paths::resolve_within` helper.
    let root = resolve_root(&state, &path)?;
    let now_checked = epic::toggle_prd_loop(&root, &epic_slug, &prd_filename, &loop_title)?;
    Ok(now_checked)
}

/// Read a spec file (epic README or PRD) under `docs/epics/`.
/// `rel_path` is relative to `docs/epics/` (e.g. `<slug>/prd-x.md`).
#[tauri::command]
pub async fn read_spec_file(
    path: String,
    rel_path: String,
    state: State<'_, AppState>,
) -> Result<String, AppError> {
    debug!("read_spec_file called for path: {path}, rel: {rel_path}");

    // Resolve the canonical, registered root (PRD FR3); `rel_path` is then
    // sandboxed to `docs/epics/` inside `epic::read_spec_file`.
    let root = resolve_root(&state, &path)?;
    epic::read_spec_file(&root, &rel_path)
}

/// Write (create or overwrite) a spec file under `docs/epics/`.
/// `rel_path` is relative to `docs/epics/`. Raw write — does not validate
/// frontmatter; a broken file will be logged and skipped by parse_epics.
#[tauri::command]
pub async fn write_spec_file(
    path: String,
    rel_path: String,
    content: String,
    state: State<'_, AppState>,
) -> Result<(), AppError> {
    debug!("write_spec_file called for path: {path}, rel: {rel_path}");

    // Resolve the canonical, registered root (PRD FR3); `rel_path` is then
    // sandboxed to `docs/epics/` inside `epic::write_spec_file`.
    let root = resolve_root(&state, &path)?;
    epic::write_spec_file(&root, &rel_path, &content)
}
