import { invoke } from "@tauri-apps/api/core";
import type { DiscoveredRepo, ProjectEntry, ProjectMeta, Decision, LoopStatus } from "../types";

/**
 * Scan a directory for project repositories.
 * Rust: scan_directory(path: String) -> Result<Vec<DiscoveredRepo>, AppError>
 */
export async function scanDirectory(path: string): Promise<DiscoveredRepo[]> {
  return invoke<DiscoveredRepo[]>("scan_directory", { path });
}

/**
 * Import a repository: bootstrap .loopdeck/ and register in global config.
 * Rust: import_project(path: String) -> Result<ProjectEntry, AppError>
 */
export async function importProject(path: string): Promise<ProjectEntry> {
  return invoke<ProjectEntry>("import_project", { path });
}

/**
 * List all registered projects.
 * Rust: list_projects() -> Result<Vec<ProjectEntry>, AppError>
 */
export async function listProjects(): Promise<ProjectEntry[]> {
  return invoke<ProjectEntry[]>("list_projects");
}

/**
 * Get a single project by path.
 * Rust: get_project(path: String) -> Result<ProjectEntry, AppError>
 */
export async function getProject(path: string): Promise<ProjectEntry> {
  return invoke<ProjectEntry>("get_project", { path });
}

/**
 * Update the project description.
 * Rust: update_description(path: String, description: String) -> Result<ProjectMeta, AppError>
 */
export async function updateDescription(
  path: string,
  description: string,
): Promise<ProjectMeta> {
  return invoke<ProjectMeta>("update_description", { path, description });
}

/**
 * Remove a project from the registry (does NOT delete files).
 * Rust: remove_project(path: String) -> Result<(), AppError>
 */
export async function removeProject(path: string): Promise<void> {
  return invoke<void>("remove_project", { path });
}

/**
 * Open the repository path in the system file manager.
 * Rust: open_in_finder(path: String) -> Result<(), AppError>
 */
export async function openInFinder(path: string): Promise<void> {
  return invoke<void>("open_in_finder", { path });
}

/**
 * Open the repository path in the system terminal.
 * Rust: open_in_terminal(path: String) -> Result<(), AppError>
 */
export async function openInTerminal(path: string): Promise<void> {
  return invoke<void>("open_in_terminal", { path });
}

/**
 * Rescan a project to refresh git info (last commit, last modified).
 * Rust: rescan_project(path: String) -> Result<ProjectEntry, AppError>
 */
export async function rescanProject(path: string): Promise<ProjectEntry> {
  return invoke<ProjectEntry>("rescan_project", { path });
}

/**
 * Regenerate the project description by re-scanning README and structure.
 * Rust: regenerate_description(path: String) -> Result<String, AppError>
 */
export async function regenerateDescription(path: string): Promise<string> {
  return invoke<string>("regenerate_description", { path });
}

/**
 * Get all decisions from .loopdeck/decisions.md.
 * Rust: get_decisions(path: String) -> Result<Vec<Decision>, AppError>
 */
export async function getDecisions(path: string): Promise<Decision[]> {
  return invoke<Decision[]>("get_decisions", { path });
}

/**
 * Get loop status from .loopdeck/loops.md.
 * Rust: get_loops(path: String) -> Result<LoopStatus, AppError>
 */
export async function getLoops(path: string): Promise<LoopStatus> {
  return invoke<LoopStatus>("get_loops", { path });
}
