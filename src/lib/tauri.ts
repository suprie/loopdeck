import { invoke, Channel } from "@tauri-apps/api/core";
import type {
  AgentConfig,
  DiscoveredRepo,
  ProjectEntry,
  ProjectMeta,
  Decision,
  LoopStatus,
  AgentResponse,
  ClaudeEvent,
  ConversationTurn,
} from "../types";

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

/**
 * Get the global agent configuration.
 * Rust: get_agent_config() -> Result<Option<AgentConfig>, AppError>
 */
export async function getAgentConfig(): Promise<AgentConfig | null> {
  return invoke<AgentConfig | null>("get_agent_config");
}

/**
 * Set (create or update) the global agent configuration.
 * Rust: set_agent_config(agent_config: AgentConfig) -> Result<AgentConfig, AppError>
 */
export async function setAgentConfig(config: AgentConfig): Promise<AgentConfig> {
  return invoke<AgentConfig>("set_agent_config", { agentConfig: config });
}

// ── Agent session commands ─────────────────────────────────────────────────

/**
 * Start the next development loop for a project.
 * Builds the next-loop prompt from `.loopdeck/loops.md` and sends it through
 * the shared agent pipeline (spawn/resume + record transcript).
 * Rust: agent_start_loop(path: String) -> Result<AgentResponse, AppError>
 */
export async function agentStartLoop(path: string): Promise<AgentResponse> {
  return invoke<AgentResponse>("agent_start_loop", { path });
}

/**
 * Send a free-form follow-up message to the project's agent session.
 * Rust: agent_send_message(path: String, prompt: String) -> Result<AgentResponse, AppError>
 */
export async function agentSendMessage(
  path: string,
  prompt: string,
): Promise<AgentResponse> {
  return invoke<AgentResponse>("agent_send_message", { path, prompt });
}

/**
 * Start the next development loop with streaming events via Tauri Channel.
 *
 * Builds the next-loop prompt from `.loopdeck/loops.md` and sends it through
 * the shared agent pipeline, emitting each assistant content block as a
 * `ClaudeEvent` on `onEvent` as it arrives. The terminal `ClaudeEvent::Result`
 * carries the full aggregated response.
 *
 * Rust: agent_start_loop_streaming(
 *   path: String, on_event: Channel<ClaudeEvent>
 * ) -> Result<(), AppError>
 */
export async function agentStartLoopStreaming(
  path: string,
  onEvent: Channel<ClaudeEvent>,
): Promise<void> {
  return invoke<void>("agent_start_loop_streaming", { path, onEvent });
}

/**
 * Send a free-form follow-up message with streaming events via Tauri Channel.
 *
 * Each assistant content block is emitted on `onEvent` as a `ClaudeEvent` as it
 * arrives, so the UI can render tokens immediately. The terminal
 * `ClaudeEvent::Result` carries the full aggregated response (usage, duration,
 * session_id, etc.), so the caller doesn't need to await a return value.
 *
 * Rust: agent_send_message_streaming(
 *   path: String, prompt: String, on_event: Channel<ClaudeEvent>
 * ) -> Result<(), AppError>
 */
export async function agentSendMessageStreaming(
  path: string,
  prompt: string,
  onEvent: Channel<ClaudeEvent>,
): Promise<void> {
  return invoke<void>("agent_send_message_streaming", {
    path,
    prompt,
    onEvent,
  });
}

/**
 * Load the persisted conversation transcript for the Agent tab.
 * Rust: agent_get_conversation(path: String) -> Result<Vec<ConversationTurn>, AppError>
 */
export async function agentGetConversation(path: string): Promise<ConversationTurn[]> {
  return invoke<ConversationTurn[]>("agent_get_conversation", { path });
}

/**
 * Reset the project's agent session: drop the live process and archive the
 * transcript so the next Start is a fresh conversation.
 * Rust: agent_reset_session(path: String) -> Result<(), AppError>
 */
export async function agentResetSession(path: string): Promise<void> {
  return invoke<void>("agent_reset_session", { path });
}
