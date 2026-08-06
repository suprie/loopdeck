import { invoke, Channel } from "@tauri-apps/api/core";
import { createAgentRosterClient } from "./agentRosterClient";
import type {
  AgentConfig,
  NamedAgentConfig,
  NamedAgentConfigInput,
  MultiAgentRun,
  MultiAgentEvent,
  MultiAgentControlAction,
  Attachment,
  DirEntry,
  DiscoveredRepo,
  ProjectEntry,
  ProjectMeta,
  GraphifyStats,
  Decision,
  Epic,
  LoopStatus,
  ExecutionState,
  LoadedExecution,
  MigrationPreview,
  ProgressSnapshot,
  RunBudgets,
  RunPlan,
  RunReport,
  RunQueueStatus,
  StallPolicy,
  AgentResponse,
  ClaudeEvent,
  ConversationTurn,
  ConversationSummary,
  ApprovalDecision,
  AskUserQuestionAnswers,
  AskUserQuestionSpec,
  PendingQuestionEntry,
  PendingPermissionEntry,
  PendingPlanEntry,
  PlanApprovalDecision,
  SkillEntry,
  LogInfo,
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
 * Create a brand-new project from scratch under `parent`: fresh directory,
 * git init, .loopdeck/ bootstrap, register in global config.
 * Rust: create_project(parent: String, name: String) -> Result<ProjectEntry, AppError>
 */
export async function createProject(parent: string, name: string): Promise<ProjectEntry> {
  return invoke<ProjectEntry>("create_project", { parent, name });
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
 * Toggle per-project autonomous mode. When enabled, the project's agent
 * self-approves floor-clearing tool calls so loops run unattended. The
 * destructive floor still applies. Takes effect on the next spawned session.
 * Rust: set_project_autonomous(path: String, autonomous: bool) -> Result<(), AppError>
 */
export async function setProjectAutonomous(
  path: string,
  autonomous: boolean,
): Promise<void> {
  return invoke<void>("set_project_autonomous", { path, autonomous });
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
 * Refresh a project's managed skills to the current app version, re-deriving
 * stack markers so newly-added skills install too.
 * Rust: refresh_skills(path: String) -> Result<Vec<String>, AppError>
 */
export async function refreshSkills(path: string): Promise<string[]> {
  return invoke<string[]>("refresh_skills", { path });
}

/**
 * Regenerate the project description by re-scanning README and structure.
 * Rust: regenerate_description(path: String) -> Result<String, AppError>
 */
export async function regenerateDescription(path: string): Promise<string> {
  return invoke<string>("regenerate_description", { path });
}

/**
 * Summarize the Graphify knowledge graph for a project, if present.
 * Returns `present: false` when no readable `graphify-out/graph.json` exists.
 * Rust: get_graphify_stats(path: String) -> Result<GraphifyStats, AppError>
 */
export async function getGraphifyStats(path: string): Promise<GraphifyStats> {
  return invoke<GraphifyStats>("get_graphify_stats", { path });
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
 * Load .loopdeck/execution.yaml (structured state). `file_present` is false when
 * the file doesn't exist yet (fresh default) — use it to branch on structured vs
 * legacy mode. Rust: get_execution_state(path) -> Result<LoadedExecution, AppError>
 */
export async function getExecutionState(
  path: string,
): Promise<LoadedExecution> {
  return invoke<LoadedExecution>("get_execution_state", { path });
}

/**
 * Read-only migration preview for a project still on legacy .loopdeck/loops.md:
 * the planned execution.yaml + every unmatched/ambiguous record (preserved
 * verbatim, never fuzzy-matched). Writes nothing. Rust:
 * get_migration_preview(path) -> Result<MigrationPreview, AppError>
 */
export async function getMigrationPreview(
  path: string,
): Promise<MigrationPreview> {
  return invoke<MigrationPreview>("get_migration_preview", { path });
}

/**
 * Perform the confirmed legacy → structured migration: writes execution.yaml,
 * renames loops.md → loops.legacy.md (original preserved). Idempotent. Returns
 * the new structured state. Rust: apply_migration(path) -> Result<ExecutionState, AppError>
 */
export async function applyMigration(path: string): Promise<ExecutionState> {
  return invoke<ExecutionState>("apply_migration", { path });
}

/**
 * Derive loop/PRD/epic execution + delivery progress from stable IDs joined
 * against execution.yaml. Structured records are authoritative when present;
 * authored checkbox/history completion fills loops with no execution record.
 * `execution_file_present` is false in legacy/empty mode (no execution.yaml
 * yet), where the caller derives all progress from checkboxes.
 * Rust: get_progress_snapshot(path) -> Result<ProgressSnapshot, AppError>
 */
export async function getProgressSnapshot(path: string): Promise<ProgressSnapshot> {
  return invoke<ProgressSnapshot>("get_progress_snapshot", { path });
}

/**
 * Write a non-authoritative Markdown snapshot of derived execution/delivery
 * progress to .loopdeck/execution-summary.md. Never read back as state.
 * Returns the written file's absolute path.
 * Rust: export_execution_summary(path) -> Result<String, AppError>
 */
export async function exportExecutionSummary(path: string): Promise<string> {
  return invoke<string>("export_execution_summary", { path });
}

/**
 * Build and persist a new run plan from a phase-picker selection
 * (prd-run-queue Phase 5): the given execution IDs, in selection order,
 * under one queue-time stall policy and draft-PR consent. Every phase starts
 * `queued` / interview `pending`. Replaces any existing plan for the project
 * outright; rejects if a run is already in progress or an ID doesn't
 * resolve to a real PRD checklist loop.
 * Rust: create_run_plan(path, execution_ids, stall_policy, draft_pr_authorized, budgets) -> Result<RunPlan, AppError>
 */
export async function createRunPlan(
  path: string,
  executionIds: string[],
  stallPolicy: StallPolicy,
  draftPrAuthorized: boolean,
  budgets: RunBudgets,
): Promise<RunPlan> {
  return invoke<RunPlan>("create_run_plan", {
    path,
    executionIds,
    stallPolicy,
    draftPrAuthorized,
    budgets,
  });
}

/**
 * Start executing a project's queued .loopdeck/run-plan.yaml in the
 * background (prd-run-queue Phase 2). Returns immediately — a run is
 * typically hours long; poll `getRunStatus` rather than awaiting completion.
 * Rejects if no plan is queued, a run is already active, no phase is
 * `queued`, or a loop is already `current` in execution.yaml.
 * Rust: queue_run(path) -> Result<(), AppError>
 */
export async function queueRun(path: string): Promise<void> {
  return invoke<void>("queue_run", { path });
}

/**
 * Cancel the in-progress run for a project: fires the run's cancel flag and
 * interrupts any in-flight turn, so cancellation takes effect immediately
 * rather than waiting for the current phase to finish. Errors if no run is
 * active. Rust: cancel_run(path) -> Result<(), AppError>
 */
export async function cancelRun(path: string): Promise<void> {
  return invoke<void>("cancel_run", { path });
}

/**
 * Read the current run plan (and its live per-phase status) for a project.
 * `null` when no plan has ever been queued.
 * Rust: get_run_status(path) -> Result<RunQueueStatus, AppError>
 */
export async function getRunStatus(path: string): Promise<RunQueueStatus> {
  return invoke<RunQueueStatus>("get_run_status", { path });
}

/**
 * Morning report read model — joins the on-disk run plan with derived
 * per-phase verdict labels and the overnight audit slice (prd-wake-up Phase 2).
 * Rust: get_run_report(path) -> Result<RunReport, AppError>
 */
export async function getRunReport(path: string): Promise<RunReport> {
  return invoke<RunReport>("get_run_report", { path });
}

/**
 * Answer a parked phase's AskUserQuestion, pin the answers into its interview,
 * and requeue it (prd-wake-up Phase 2). Returns the updated plan.
 * Rust: answer_parked_question(path, execution_id, answers) -> Result<RunPlan, AppError>
 */
export async function answerParkedQuestion(
  path: string,
  executionId: string,
  answers: AskUserQuestionAnswers,
): Promise<RunPlan> {
  // Convert camelCase `otherText` to snake_case `other_text`, same as agentAnswerQuestion.
  const wire: Record<string, { labels: string[]; other_text?: string }> = {};
  for (const [q, a] of Object.entries(answers)) {
    wire[q] = { labels: a.labels ?? [] };
    if (a.otherText != null && a.otherText.trim() !== "") {
      wire[q].other_text = a.otherText;
    }
  }
  return invoke<RunPlan>("answer_parked_question", {
    path,
    executionId,
    answers: wire,
  });
}

/** Requeue a retryable terminal phase (plus dependents parked solely because of it). */
export async function requeueRunPhase(
  path: string,
  executionId: string,
): Promise<RunPlan> {
  return invoke<RunPlan>("requeue_run_phase", { path, executionId });
}

/**
 * Run one queued phase's pre-flight interview turn (prd-run-queue Phase 3):
 * a bounded session whose `AskUserQuestion` calls render as the same
 * question cards chat already shows. Awaits the whole turn, including any
 * parked question, so it only resolves once the user has answered (or the
 * agent decided nothing was ambiguous) — call this from an active UI
 * session, not a background poll. Pins the answers into the phase's
 * `interview` and marks `interview_status` "answered".
 * Rust: run_phase_interview(path, execution_id) -> Result<RunPlan, AppError>
 */
export async function runPhaseInterview(
  path: string,
  executionId: string,
): Promise<RunPlan> {
  return invoke<RunPlan>("run_phase_interview", { path, executionId });
}

/**
 * Explicitly skip a queued phase's pre-flight interview — no session is
 * run; `interview_status` becomes "skipped", unblocking `queueRun` for a
 * phase judged unambiguous.
 * Rust: skip_phase_interview(path, execution_id) -> Result<RunPlan, AppError>
 */
export async function skipPhaseInterview(
  path: string,
  executionId: string,
): Promise<RunPlan> {
  return invoke<RunPlan>("skip_phase_interview", { path, executionId });
}

/**
 * Get all epics from docs/epics/, each with its PRDs and phase checklists.
 * Returns an empty list if docs/epics/ does not exist.
 * Rust: get_epics(path: String) -> Result<Vec<Epic>, AppError>
 */
export async function getEpics(path: string): Promise<Epic[]> {
  return invoke<Epic[]>("get_epics", { path });
}

/**
 * Get epics grouped by milestone (ordered), for the cross-project /epics view.
 * Epics with no milestone land in an "Unmilestoned" bucket.
 * Rust: get_epics_by_milestone(path: String) -> Result<BTreeMap<String, Vec<Epic>>, AppError>
 */
export async function getEpicsByMilestone(
  path: string,
): Promise<Record<string, Epic[]>> {
  return invoke<Record<string, Epic[]>>("get_epics_by_milestone", { path });
}

/**
 * Promote a PRD checklist item into .loopdeck/loops.md ## Current.
 * Refuses (AppError kind "conflict") if a loop is already in progress.
 * Rust: promote_epic_loop(path, epic_slug, prd_filename, loop_title) -> Result<(), AppError>
 */
export async function promoteEpicLoop(
  path: string,
  epicSlug: string,
  prdFilename: string,
  loopTitle: string,
): Promise<void> {
  return invoke<void>("promote_epic_loop", {
    path,
    epicSlug,
    prdFilename,
    loopTitle,
  });
}

/**
 * Toggle a - [ ] / - [x] next-step checklist item in .loopdeck/loops.md.
 * Returns the new checked state.
 * Rust: toggle_loop_step(path, step_text) -> Result<bool, AppError>
 */
export async function toggleLoopStep(
  path: string,
  stepText: string,
): Promise<boolean> {
  return invoke<boolean>("toggle_loop_step", { path, stepText });
}

/**
 * Toggle a - [ ] / - [x] checklist item in a PRD file under docs/epics/.
 * Returns the new checked state.
 * Rust: toggle_prd_loop(path, epic_slug, prd_filename, loop_title) -> Result<bool, AppError>
 */
export async function togglePrdLoop(
  path: string,
  epicSlug: string,
  prdFilename: string,
  loopTitle: string,
): Promise<boolean> {
  return invoke<boolean>("toggle_prd_loop", {
    path,
    epicSlug,
    prdFilename,
    loopTitle,
  });
}

/**
 * Generate a collision-free stable `epic-slug/title-slug` id for an id-less
 * loop and rewrite its checklist line in the PRD file in place. Returns the
 * newly assigned id. Rejects if the loop already has an id.
 * Rust: assign_loop_id(path, epic_slug, prd_filename, loop_title) -> Result<String, AppError>
 */
export async function assignLoopId(
  path: string,
  epicSlug: string,
  prdFilename: string,
  loopTitle: string,
): Promise<string> {
  return invoke<string>("assign_loop_id", {
    path,
    epicSlug,
    prdFilename,
    loopTitle,
  });
}

/**
 * Set a PRD's status frontmatter field ("proposed" / "accepted" / "completed").
 * Rust: set_prd_status(path, epic_slug, prd_filename, status) -> Result<(), AppError>
 */
export async function setPrdStatus(
  path: string,
  epicSlug: string,
  prdFilename: string,
  status: string,
): Promise<void> {
  return invoke<void>("set_prd_status", {
    path,
    epicSlug,
    prdFilename,
    status,
  });
}

/**
 * Backfill `order:` frontmatter onto any PRD in epicSlug missing one,
 * derived from the current README/filename-fallback order. Idempotent —
 * safe to call repeatedly. Returns the count of PRDs updated.
 * Rust: migrate_prd_order(path, epic_slug) -> Result<usize, AppError>
 */
export async function migratePrdOrder(
  path: string,
  epicSlug: string,
): Promise<number> {
  return invoke<number>("migrate_prd_order", { path, epicSlug });
}

/**
 * Explicitly reorder every PRD in an epic (drag-to-reorder). orderedFiles
 * must be exactly the epic's current PRD filenames, in the desired order.
 * Rust: set_prd_order(path, epic_slug, ordered_files) -> Result<(), AppError>
 */
export async function setPrdOrder(
  path: string,
  epicSlug: string,
  orderedFiles: string[],
): Promise<void> {
  return invoke<void>("set_prd_order", { path, epicSlug, orderedFiles });
}

/**
 * Read a spec file (epic README or PRD) under docs/epics/.
 * relPath is relative to docs/epics/ (e.g. "<slug>/prd-x.md").
 * Rust: read_spec_file(path, rel_path) -> Result<String, AppError>
 */
export async function readSpecFile(
  path: string,
  relPath: string,
): Promise<string> {
  return invoke<string>("read_spec_file", { path, relPath });
}

/**
 * Write (create or overwrite) a spec file under docs/epics/.
 * relPath is relative to docs/epics/. Raw write — does not validate frontmatter.
 * Rust: write_spec_file(path, rel_path, content) -> Result<(), AppError>
 */
export async function writeSpecFile(
  path: string,
  relPath: string,
  content: string,
): Promise<void> {
  return invoke<void>("write_spec_file", { path, relPath, content });
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

/**
 * Remove the stored auth token from the local secrets file.
 * Rust: clear_auth_token() -> Result<(), AppError>
 */
export async function clearAuthToken(): Promise<void> {
  return invoke<void>("clear_auth_token");
}

// ── Named agent roster ────────────────────────────────────────────────────

const agentRosterClient = createAgentRosterClient((command, args) =>
  invoke(command, args),
);

/** List the reusable global agent profiles (secrets are never returned). */
export async function listAgentConfigs(): Promise<NamedAgentConfig[]> {
  return agentRosterClient.list();
}

/** Create one reusable global agent profile. */
export async function createAgentConfig(
  config: NamedAgentConfigInput,
): Promise<NamedAgentConfig> {
  return agentRosterClient.create(config);
}

/** Update one profile by its immutable UUID. */
export async function updateAgentConfig(
  id: string,
  config: NamedAgentConfigInput,
): Promise<NamedAgentConfig> {
  return agentRosterClient.update(id, config);
}

/** Delete one profile by UUID. The backend rejects deleting the final profile. */
export async function deleteAgentConfig(id: string): Promise<void> {
  return agentRosterClient.delete(id);
}

/** Get the default profile's stable ID, or null when no profile exists. */
export async function getDefaultAgentConfig(): Promise<NamedAgentConfig | null> {
  return agentRosterClient.getDefault();
}

/** Select the default profile used to prefill a new loop assignment. */
export async function setDefaultAgentConfig(id: string): Promise<NamedAgentConfig> {
  return agentRosterClient.setDefault(id);
}

/**
 * Snapshot of the log directory (path, retained files + sizes, total, cap) for
 * the Settings → Diagnostics panel. Reads names/sizes only — never contents.
 * Rust: get_log_info() -> Result<LogInfo, AppError>
 */
export async function getLogInfo(): Promise<LogInfo> {
  return invoke<LogInfo>("get_log_info");
}

/**
 * Open the log directory in the OS file manager (Finder on macOS) so the user
 * can inspect or share diagnostics.
 * Rust: reveal_log_dir() -> Result<(), AppError>
 */
export async function revealLogDir(): Promise<void> {
  return invoke<void>("reveal_log_dir");
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
 * `attachments` are images pasted/dropped into the composer, sent as inline
 * base64 content blocks alongside the text.
 *
 * Rust: agent_send_message(
 *   path: String, prompt: String, attachments: Option<Vec<Attachment>>
 * ) -> Result<AgentResponse, AppError>
 */
export async function agentSendMessage(
  path: string,
  prompt: string,
  attachments: Attachment[] = [],
): Promise<AgentResponse> {
  return invoke<AgentResponse>("agent_send_message", {
    path,
    prompt,
    attachments,
  });
}

/**
 * Read an image file from disk into an inline-base64 attachment.
 *
 * Only needed for the composer's drag-and-drop path: Tauri delivers a drop as
 * a filesystem path rather than as bytes, so the file has to be read by the
 * backend. Paste and the file picker hand the webview a `File` directly and
 * skip this entirely. Returned at original size — the caller downscales.
 *
 * Rust: agent_read_image_attachment(path: String) -> Result<Attachment, AppError>
 */
export async function agentReadImageAttachment(
  path: string,
): Promise<Attachment> {
  return invoke<Attachment>("agent_read_image_attachment", { path });
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

// ── Multi-agent loop commands ─────────────────────────────────────────────

/**
 * Start one logical loop as independently isolated sub-runs, one per assigned
 * profile. The backend emits `MultiAgentEvent`s tagged with both run and agent
 * IDs through `onEvent` while the returned snapshot establishes the run card.
 */
export async function agentStartMultiLoopStreaming(
  path: string,
  agentIds: string[],
  onEvent: Channel<MultiAgentEvent>,
): Promise<MultiAgentRun> {
  return invoke<MultiAgentRun>("agent_start_multi_loop_streaming", {
    path,
    agentIds,
    onEvent,
  });
}

/** Read a persisted multi-agent run snapshot (current or historical). */
export async function agentGetMultiAgentRun(
  path: string,
  runId: string,
): Promise<MultiAgentRun> {
  return invoke<MultiAgentRun>("agent_get_multi_agent_run", { path, runId });
}

/** List the newest multi-agent runs for a project. */
export async function agentListMultiAgentRuns(path: string): Promise<MultiAgentRun[]> {
  return invoke<MultiAgentRun[]>("agent_list_multi_agent_runs", { path });
}

/** Interrupt or retry exactly one assigned profile's sub-run. */
export async function agentControlMultiAgentRun(
  path: string,
  runId: string,
  agentId: string,
  action: MultiAgentControlAction,
): Promise<MultiAgentRun> {
  return invoke<MultiAgentRun>("agent_control_multi_agent_run", {
    path,
    runId,
    agentId,
    action,
  });
}

/**
 * Send a free-form follow-up message with streaming events via Tauri Channel.
 *
 * Each assistant content block is emitted on `onEvent` as a `ClaudeEvent` as it
 * arrives, so the UI can render tokens immediately. The terminal
 * `ClaudeEvent::Result` carries the full aggregated response (usage, duration,
 * session_id, etc.), so the caller doesn't need to await a return value.
 *
 * `planMode`: when true, the turn runs under the CLI's `plan` permission mode
 * (mirrors Claude Code's shift-tab toggle) — the agent is restricted to
 * read-only tools plus `ExitPlanMode`, which surfaces a `plan_approval`
 * channel event instead of letting the agent edit anything.
 *
 * `attachments` are images pasted/dropped into the composer, sent as inline
 * base64 content blocks ahead of the text block.
 *
 * Rust: agent_send_message_streaming(
 *   path: String, prompt: String, attachments: Option<Vec<Attachment>>,
 *   on_event: Channel<ClaudeEvent>, plan_mode: bool
 * ) -> Result<(), AppError>
 */
export async function agentSendMessageStreaming(
  path: string,
  prompt: string,
  onEvent: Channel<ClaudeEvent>,
  planMode: boolean = false,
  attachments: Attachment[] = [],
): Promise<void> {
  return invoke<void>("agent_send_message_streaming", {
    path,
    prompt,
    attachments,
    onEvent,
    planMode,
  });
}

/**
 * Answer a pending `AskUserQuestion` for the given project.
 *
 * Called by the frontend when the user submits the question card. The answers
 * are delivered to the backend's parked read loop (keyed by `request_id` and
 * the project path), which writes the `control_response` carrying
 * `updatedInput.answers` and the agent turn resumes.
 *
 * Rust: agent_answer_question(
 *   path: String, request_id: String, answers: HashMap<String, AnswerWire>
 * ) -> Result<(), AppError>
 *
 * The wire shape of `AnswerWire` is `{ labels: string[], other_text?: string }`
 * (snake_case on the Rust side); we send `otherText` and rely on serde's
 * `#[serde(rename)]`-friendly defaults — actually the struct uses
 * `#[serde(default)]` without a rename, so we must send `other_text`.
 */
export async function agentAnswerQuestion(
  path: string,
  requestId: string,
  answers: AskUserQuestionAnswers,
): Promise<void> {
  // Convert the frontend's camelCase `otherText` to the snake_case the Rust
  // `AnswerWire` struct expects (`other_text`). `labels` is already lowercase.
  const wire: Record<string, { labels: string[]; other_text?: string }> = {};
  for (const [q, a] of Object.entries(answers)) {
    wire[q] = { labels: a.labels ?? [] };
    if (a.otherText != null && a.otherText.trim() !== "") {
      wire[q].other_text = a.otherText;
    }
  }
  return invoke<void>("agent_answer_question", {
    path,
    requestId,
    answers: wire,
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
 * List all conversations (active + archived) for the history UI.
 *
 * Returns one `ConversationSummary` per transcript file, sorted newest-first.
 * Each row's `id` is the handle to pass to `agentGetConversationById`.
 *
 * Rust: agent_list_conversations(path: String) -> Result<Vec<ConversationSummary>, AppError>
 */
export async function agentListConversations(
  path: string,
): Promise<ConversationSummary[]> {
  return invoke<ConversationSummary[]>("agent_list_conversations", { path });
}

/**
 * Load a specific conversation by id (`"active"` or an archive stem).
 *
 * Used by the history viewer to open a past conversation read-only. Returns
 * an empty vec for an unknown id (e.g. an archive deleted out of band).
 *
 * Rust: agent_get_conversation_by_id(path: String, id: String) -> Result<Vec<ConversationTurn>, AppError>
 */
export async function agentGetConversationById(
  path: string,
  id: string,
): Promise<ConversationTurn[]> {
  return invoke<ConversationTurn[]>("agent_get_conversation_by_id", { path, id });
}

/**
 * Promote an archived conversation to active so the user can continue it.
 *
 * Archives the current active transcript aside (it survives in history) and
 * seeds a fresh active transcript with the named conversation's turns. Returns
 * the conversation's `session_id` (so the agent can `--resume` its context), or
 * `null` when the source has no session_id (empty / no assistant turns).
 *
 * Called by the frontend when the user sends a follow-up while viewing an
 * archived conversation — at that moment we make the archive "live" so the new
 * turn appends to it naturally.
 *
 * Rust: agent_promote_to_active(
 *   path: String, id: String
 * ) -> Result<Option<String>, AppError>
 */
export async function agentPromoteToActive(
  path: string,
  id: string,
): Promise<string | null> {
  return invoke<string | null>("agent_promote_to_active", { path, id });
}

/**
 * Resolve a pending manual-approval request (Allow / Deny).
 *
 * Called when the user clicks Allow or Deny on the approval card. Delivers the
 * verdict to the backend's parked read loop, which writes the `control_response`
 * and the agent turn resumes (or, on deny, recovers).
 *
 * Rust: agent_answer_permission(
 *   path: String, request_id: String, decision: ApprovalWire
 * ) -> Result<(), AppError>
 */
export async function agentAnswerPermission(
  path: string,
  requestId: string,
  decision: ApprovalDecision,
): Promise<void> {
  return invoke<void>("agent_answer_permission", {
    path,
    requestId,
    decision,
  });
}

/**
 * Resolve a pending `ExitPlanMode` request (Approve / Reject).
 *
 * Called when the user clicks Approve or Reject on the plan card. Delivers
 * the verdict to the backend's parked read loop, which writes the matching
 * `control_response`. On approve, the CLI reverts out of plan mode and the
 * agent starts executing; on reject, the model is expected to revise the
 * plan and call `ExitPlanMode` again.
 *
 * Rust: agent_answer_plan(
 *   path: String, request_id: String, decision: PlanApprovalWire
 * ) -> Result<(), AppError>
 */
export async function agentAnswerPlan(
  path: string,
  requestId: string,
  decision: PlanApprovalDecision,
): Promise<void> {
  return invoke<void>("agent_answer_plan", {
    path,
    requestId,
    decision,
  });
}

/**
 * Persist an "always allow" permission rule into `.claude/settings.local.json`.
 *
 * The "Always allow" button on the approval card: in addition to resolving the
 * current approval (done via `agentAnswerPermission`), the rule is written so
 * future calls of the same tool/command short-circuit via Claude Code's own
 * allow-list instead of prompting again. The rule string is built in the
 * frontend in the canonical Claude Code format (`Bash(cmd:*)`, `Read(*)`, …).
 *
 * Takes effect on the NEXT spawned session (settings are loaded at spawn); the
 * current approval still needs its normal verdict, which the frontend fires
 * alongside this call.
 *
 * Rust: agent_add_allow_rule(path: String, rule: String) -> Result<(), AppError>
 */
export async function agentAddAllowRule(path: string, rule: string): Promise<void> {
  return invoke<void>("agent_add_allow_rule", { path, rule });
}

/**
 * Gracefully interrupt the in-flight agent turn (Stop button).
 *
 * Ends the current turn gracefully — the live process keeps its conversation
 * context, so the next send resumes the same conversation (unlike
 * `agentResetSession`, which kills both). No-op when no turn is in flight, so
 * the Stop button stays responsive either way.
 *
 * Rust: agent_interrupt(path: String) -> Result<(), AppError>
 */
export async function agentInterrupt(path: string): Promise<void> {
  return invoke<void>("agent_interrupt", { path });
}

/**
 * Reset the project's agent session: drop the live process and archive the
 * transcript so the next Start is a fresh conversation.
 * Rust: agent_reset_session(path: String) -> Result<(), AppError>
 */
export async function agentResetSession(path: string): Promise<void> {
  return invoke<void>("agent_reset_session", { path });
}

/**
 * Report whether an agent turn is currently in flight for the project.
 *
 * Used by AgentPanel to reconcile `busy` state after the component unmounts
 * (user navigated away mid-turn) and remounts. The Tauri Channel the previous
 * mount subscribed to is gone, so streaming events from the still-running
 * backend can no longer reach the UI — this command lets a fresh mount detect
 * the in-flight turn, show an honest "Agent is working…" state, and poll the
 * transcript until the persisted assistant turn lands.
 * Rust: agent_is_busy(path: String) -> Result<bool, AppError>
 */
export async function agentIsBusy(path: string): Promise<boolean> {
  return invoke<boolean>("agent_is_busy", { path });
}

/**
 * Read the pending manual-approval payload for a project, if any.
 *
 * Does NOT consume the sender — only the payload. Used by AgentPanel to
 * re-render the Allow/Deny card after navigating away and back: the original
 * permission_request event arrived on a Tauri Channel whose subscriber
 * unmounted, so this is the only source of truth for the parked prompt.
 * Rust: agent_pending_permission(path: String) -> Result<Option<PendingPermissionInfo>, AppError>
 */
export async function agentPendingPermission(
  path: string,
): Promise<{ requestId: string; toolName: string; input: string } | null> {
  return invoke<{ requestId: string; toolName: string; input: string } | null>(
    "agent_pending_permission",
    { path },
  );
}

/**
 * Read the pending AskUserQuestion payload for a project, if any.
 *
 * Counterpart of `agentPendingPermission` for the question card. Does NOT
 * consume the sender.
 * Rust: agent_pending_question(path: String) -> Result<Option<PendingQuestionInfo>, AppError>
 */
export async function agentPendingQuestion(
  path: string,
): Promise<{ requestId: string; questions: AskUserQuestionSpec[] } | null> {
  return invoke<{ requestId: string; questions: AskUserQuestionSpec[] } | null>(
    "agent_pending_question",
    { path },
  );
}

/**
 * List every project with a pending `AskUserQuestion`, across the whole
 * registry. The cross-project aggregate of `agentPendingQuestion`.
 *
 * The frontend calls this on app launch, on window focus, and on manual
 * refresh to surface "stuck" prompts — ones parked while the user was on
 * another view or had the Mac locked, so the per-project question card never
 * rendered.
 *
 * Rust: list_pending_questions() -> Result<Vec<PendingQuestionEntry>, AppError>
 */
export async function listPendingQuestions(): Promise<PendingQuestionEntry[]> {
  return invoke<PendingQuestionEntry[]>("list_pending_questions");
}

/**
 * List every project with a pending manual-approval request, across the whole
 * registry. The cross-project aggregate of `agentPendingPermission`, and the
 * permission-side mirror of `listPendingQuestions`.
 *
 * The frontend calls this on app launch, on window focus, and on manual
 * refresh to surface "stuck" approvals — ones parked while the user was on
 * another view or had the Mac locked, so the per-project approval card never
 * rendered (and would otherwise auto-deny on the 10-min timeout).
 *
 * Rust: list_pending_permissions() -> Result<Vec<PendingPermissionEntry>, AppError>
 */
export async function listPendingPermissions(): Promise<PendingPermissionEntry[]> {
  return invoke<PendingPermissionEntry[]>("list_pending_permissions");
}

/**
 * Read the pending `ExitPlanMode` payload for a project, if any.
 *
 * Counterpart of `agentPendingPermission` for the plan-approval card. Does
 * NOT consume the sender.
 * Rust: agent_pending_plan(path: String) -> Result<Option<PendingPlanInfo>, AppError>
 */
export async function agentPendingPlan(
  path: string,
): Promise<{ requestId: string; plan: string } | null> {
  return invoke<{ requestId: string; plan: string } | null>("agent_pending_plan", { path });
}

/**
 * List every project with a pending `ExitPlanMode` request, across the whole
 * registry. The cross-project aggregate of `agentPendingPlan`, and the
 * plan-side mirror of `listPendingPermissions`.
 *
 * Rust: list_pending_plans() -> Result<Vec<PendingPlanEntry>, AppError>
 */
export async function listPendingPlans(): Promise<PendingPlanEntry[]> {
  return invoke<PendingPlanEntry[]>("list_pending_plans");
}

/**
 * List the direct children of a directory inside a project, for the chat
 * composer's `@`-mention autocomplete.
 *
 * `subdir` is project-relative (`""` = project root); the user navigates into
 * subfolders by selecting folders, which the frontend turns into successive
 * calls with deeper `subdir` values. Hidden entries and build/dependency
 * directories are filtered out server-side; the result is sorted
 * directories-first then alphabetical.
 *
 * Rust: list_dir_entries(path: String, subdir: String) -> Result<Vec<DirEntry>, AppError>
 */
export async function listDirEntries(
  path: string,
  subdir: string,
): Promise<DirEntry[]> {
  return invoke<DirEntry[]>("list_dir_entries", { path, subdir });
}

/**
 * Recursively search the project tree for files/folders matching `query`
 * (substring, case-insensitive), ranked by match quality (exact basename >
 * basename prefix > basename contains > path contains) then by path depth.
 *
 * Used by the `@`-mention autocomplete once the user types a filter after the
 * `@` — it searches the whole project at once instead of listing one folder.
 * `node_modules`, `target`, dotfiles etc. are pruned server-side. Capped at
 * `maxResults` (default 50).
 *
 * Rust: search_project_files(
 *   path: String, query: String, max_results: Option<usize>
 * ) -> Result<Vec<DirEntry>, AppError>
 */
export async function searchProjectFiles(
  path: string,
  query: string,
  maxResults?: number,
): Promise<DirEntry[]> {
  return invoke<DirEntry[]>("search_project_files", {
    path,
    query,
    maxResults,
  });
}

/**
 * List the skills installed for a project, for the composer's `/`-skill
 * discovery menu.
 *
 * Reads `<repo>/.claude/skills/<dir>/SKILL.md` (the files `copy_skills` writes
 * during project bootstrap) and parses each frontmatter for `name` (the
 * invocation token) and `description`. A project that hasn't been bootstrapped
 * yet — no `.claude/skills/` — returns an empty array, not an error; the menu
 * shows "no skills" in that case. Results are sorted by name.
 *
 * Rust: list_skills(path: String) -> Result<Vec<SkillEntry>, AppError>
 */
export async function listSkills(path: string): Promise<SkillEntry[]> {
  return invoke<SkillEntry[]>("list_skills", { path });
}
