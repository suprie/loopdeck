// TypeScript types mirroring Rust structs from src-tauri/src/

/** A repository discovered during directory scanning. */
export interface DiscoveredRepo {
  path: string;
  name: string;
  markers: string[];
  has_readme: boolean;
  has_loopdeck: boolean;
  /** Whether `graphify-out/graph.json` is present (Graphify already ran). */
  has_graphify: boolean;
  /** Human-readable technology stack, e.g. "Rust, JavaScript/TypeScript". */
  detected_stack: string;
  /** Lightweight description preview generated from the detected stack. */
  description_preview: string;
  last_commit: string | null;
  last_modified: string | null;
}

/** A child entry (file or folder) of a project directory, for the `@`-mention
 *  autocomplete in the chat composer. `path` is project-relative (forward
 *  slashes) so it can be inserted verbatim as `@<path>`. */
export interface DirEntry {
  /** Entry basename, e.g. `Chat.tsx`. */
  name: string;
  /** Whether the entry is a directory. */
  isDir: boolean;
  /** Project-relative path, e.g. `src/components/detail/Chat.tsx`. */
  path: string;
}

/** A skill installed for a project, surfaced by the composer's `/`-skill
 *  discovery menu. Read from `<repo>/.claude/skills/<dir>/SKILL.md`.
 *
 *  `name` is the frontmatter `name` (e.g. `loopdeck:rust-expert`) — the
 *  invocation token the `claude` CLI recognizes, so the menu inserts it
 *  verbatim as `/<name>`. It is distinct from `directory`, the on-disk folder
 *  name (`loopdeck-rust-expert`). */
export interface SkillEntry {
  /** Frontmatter `name` — the invocation token the `claude` CLI recognizes. */
  name: string;
  /** On-disk skill directory name, e.g. `loopdeck-rust-expert`. */
  directory: string;
  /** Frontmatter `description`, shown under the name in the menu. Empty string
   *  if the SKILL.md has no `description:` field. */
  description: string;
  /** Frontmatter `argument-hint`, shown as a dimmed placeholder next to the
   *  skill name (e.g. `<prd-file-path>`) to cue the user what to type after.
   *  Empty string when the skill takes no arguments. */
  argumentHint: string;
}

/** Project status from Rust ProjectStatus enum. */
export type ProjectStatus = "active" | "archived" | "nonactive" | "warning";

/**
 * Live agent run state for a project, derived per `list_projects` call from
 * the in-flight session + any pending manual approval / AskUserQuestion.
 * Mirrors Rust `RunState` (serde `rename_all = "lowercase"`).
 *
 * - `idle` — no live session, or session exists but no turn is in flight.
 * - `working` — a streaming agent turn is in flight right now.
 * - `waiting` — the in-flight turn is parked awaiting the user (a manual
 *   permission approval or an `AskUserQuestion` answer).
 * - `done` — surfaced by the frontend when a turn just finished (transient).
 */
export type RunState = "idle" | "working" | "waiting" | "done";

/** Aggregate diff stats for an uncommitted working tree. Mirrors Rust struct. */
export interface UncommittedStats {
  files: number;
  added: number;
  deleted: number;
}

/** A registered project entry from the global config. */
export interface ProjectEntry {
  path: string;
  name: string;
  description: string;
  status: ProjectStatus;
  /** Content of .loopdeck/current-loop.md, if present. */
  current_loop?: string;
  last_opened: string | null;
  created_at: string;
  /** ISO 8601 timestamp of the last git commit (refreshed on startup/rescan). */
  last_commit_date: string | null;
  /** Subject line of the last git commit. */
  last_commit_message: string | null;
  /** ISO 8601 timestamp of the most recently modified file. */
  last_modified: string | null;
  /** Uncommitted working-tree diff stats (refreshed on startup/rescan). Older
   *  configs without this field load as all-zero. */
  uncommitted: UncommittedStats;
  /** Live agent run state — derived at read time, not persisted. Older configs
   *  without this field load as `idle`. */
  run_state: RunState;
  /**
   * Per-project autonomous mode: when true, the agent self-approves
   * floor-clearing tool calls (Edit/Write, safe Bash, MCP, WebFetch) so loops
   * run unattended. The destructive floor still applies. Older configs without
   * this field load as `false` (confirm-changes). Optional on the wire because
   * the backend `skip_serializing_if`s it when false.
   */
  autonomous?: boolean;
}

/** Content of .loopdeck/project.yaml. */
export interface ProjectMeta {
  name: string;
  description: string;
  status: string;
  created_at: string;
}

/** Per-confidence link counts for a Graphify knowledge graph. */
export interface ConfidenceBreakdown {
  /** Links extracted directly from source (AST-level certainty). */
  extracted: number;
  /** Links inferred by an LLM backend (plausible but not provable). */
  inferred: number;
  /** Links the extractor flagged as ambiguous. */
  ambiguous: number;
}

/** Summary of a project's Graphify knowledge graph.
 *  `present: false` means no readable `graphify-out/graph.json` was found —
 *  the UI should hide the Graph tab rather than render empty stats. LoopDeck
 *  only reads Graphify's output and never runs it. */
export interface GraphifyStats {
  /** `false` when `graphify-out/graph.json` is missing or unparseable. */
  present: boolean;
  /** Number of nodes in the graph. */
  node_count: number;
  /** Number of links (edges) in the graph. */
  edge_count: number;
  /** Number of distinct community partitions. */
  community_count: number;
  /** Labels of the highest-degree nodes (most connected first), capped at 10. */
  god_nodes: string[];
  /** Distribution of link confidence values. */
  confidence: ConfidenceBreakdown;
  /** Build date parsed from `GRAPH_REPORT.md` (`YYYY-MM-DD`), if present. */
  built_at: string | null;
  /** Absolute path to `graphify-out/graph.json`. */
  graph_path: string;
  /** Absolute path to `graphify-out/GRAPH_REPORT.md`, if it exists. */
  report_path: string | null;
  /** Absolute path to `graphify-out/graph.html`, if it exists. */
  html_path: string | null;
}

/** Structured error returned from Rust AppError. */
export interface AppError {
  message: string;
  kind:
    | "io"
    | "yaml"
    | "walkdir"
    | "projectNotFound"
    | "invalidPath"
    | "scan"
    | "config"
    | "lockError"
    | "projectAlreadyExists"
    | "conflict"
    | "agent";
}

/** Global agent configuration (mirrors Rust AgentConfig). */
export interface AgentConfig {
  auth_token?: string;
  base_url?: string;
  model?: string;
  effort?: string;
  /**
   * Read-only signal from the backend: true when an auth token is stored in
   * the local secrets file. The plaintext token itself is never sent over IPC,
   * so the Settings UI uses this to show a "token stored" affordance.
   */
  has_auth_token?: boolean;
}

/** One retained log file, surfaced in Settings → Diagnostics. Mirrors Rust `LogFileInfo`. */
export interface LogFileInfo {
  /** File name, e.g. `loopdeck.log.2026-07-23`. */
  name: string;
  /** Size in bytes. */
  size_bytes: number;
}

/**
 * Snapshot of the log directory for Settings → Diagnostics. Mirrors Rust
 * `LogInfo`. The backend reads only file names/sizes — never contents — so
 * this surface can't exfiltrate whatever a log line might contain.
 */
export interface LogInfo {
  /** Absolute path to the log directory; `null` when logging is stderr-only. */
  dir: string | null;
  /** Retained `loopdeck.log*` files, newest-first. */
  files: LogFileInfo[];
  /** Total bytes across `files`. */
  total_bytes: number;
  /** Configured retention cap (max daily files kept on disk). */
  max_files: number;
}

/** A single architectural decision record from .loopdeck/decisions.md. */
export interface Decision {
  date: string;
  title: string;
  status: "proposed" | "accepted" | "superseded";
  context: string;
  consequences?: string;
}

/** A single development loop from .loopdeck/loops.md. */
export interface Loop {
  started: string;
  goal: string;
  status: "in_progress" | "completed" | "abandoned";
  completed?: string;
}

/** A single checklist item under `## Next Steps` in loops.md. Mirrors Rust `NextStep`. */
export interface NextStep {
  /** The step text (without the `- [ ]` / `- [x]` prefix). */
  text: string;
  /** Whether the box is checked (`- [x]`). */
  checked: boolean;
}

/** Full loop status from .loopdeck/loops.md. */
export interface LoopStatus {
  current: Loop | null;
  next_steps: NextStep[];
  history: Loop[];
}

// ── Structured execution state (.loopdeck/execution.yaml) — 0.2.1 ────
// Mirrors Rust `execution.rs` + `migration.rs`. Dates serialize as RFC3339
// strings (chrono). The ID is the join key; titles are presentation only.

/** A spec → runtime origin triple. Mirrors Rust `LoopOrigin`. */
export interface LoopOrigin {
  epic: string;
  prd: string;
  phase: string;
}

/** serde `rename_all = "lowercase"`. */
export type Outcome = "completed" | "abandoned";

/** Local Git delivery evidence (optional; Phase 6 enriches it). */
export interface GitEvidence {
  commit: string;
}

/** The loop currently in progress (`current:`). */
export interface ActiveLoop {
  id: string;
  title: string;
  origin: LoopOrigin;
  started_at: string;
  attempt: number;
}

/** A planned-but-not-started loop (`queue:`). */
export interface QueuedLoop {
  id: string;
  title: string;
  origin: LoopOrigin;
  queued_at: string;
}

/** A finished loop (`history:`), completed or abandoned. */
export interface HistoryLoop {
  id: string;
  title: string;
  origin: LoopOrigin;
  outcome: Outcome;
  started_at: string;
  completed_at: string;
  attempt: number;
  git?: GitEvidence;
  /** Present only for abandoned loops. */
  reason?: string;
}

/** The on-disk shape of `.loopdeck/execution.yaml`. */
export interface ExecutionState {
  schema_version: number;
  revision: number;
  current?: ActiveLoop;
  queue: QueuedLoop[];
  history: HistoryLoop[];
}

/** Where a loaded ExecutionState came from. Mirrors Rust `LoadSource` (PascalCase). */
export type LoadSource = "Default" | "Primary" | "BackupRecovered";

export interface LoadedExecution {
  state: ExecutionState;
  source: LoadSource;
  warnings: string[];
  /** Whether `execution.yaml` exists on disk (vs a fresh default). Lets the UI
   * branch on structured-vs-legacy mode (Phase 4 migration surface). */
  file_present: boolean;
}

/** A legacy record that could not be mapped to exactly one PRD loop. */
export interface UnmatchedRecord {
  label: string;
  title: string;
  section: "current" | "history";
  reason: string;
}

/** A Next Steps item that could not become a queue entry. */
export interface UnconvertedNextStep {
  text: string;
  checked: boolean;
}

/** The migration preview: planned state + everything that stayed unmatched. */
export interface MigrationPreview {
  planned: ExecutionState;
  current_matched: boolean;
  matched_history: number;
  unmatched: UnmatchedRecord[];
  unconverted_next_steps: UnconvertedNextStep[];
  execution_yaml_present: boolean;
  loops_md_present: boolean;
}

/**
 * A parsed epic from docs/epics/<slug>/README.md, with its PRDs attached.
 * Mirrors Rust `Epic` in epic.rs.
 */
export interface Epic {
  slug: string;
  title: string;
  milestone: string;
  status: "proposed" | "in_progress" | "completed" | "abandoned";
  description: string;
  started?: string;
  completed?: string;
  owner?: string;
  /** Path to the epic directory (back-reference for the promote action). */
  dir: string;
  prds: Prd[];
}

/** A parsed PRD, with its phase checklists attached. Mirrors Rust `Prd`. */
export interface Prd {
  /** PRD slug — the `prd:` frontmatter field (filename without `.md`). */
  slug: string;
  /** Parent epic slug. */
  epic: string;
  status: "proposed" | "accepted" | "completed";
  description: string;
  milestone?: string;
  /** Filename of the PRD file (back-reference for the promote action). */
  file: string;
  phases: PrdPhase[];
}

/** A `### Phase N — Name` section within a PRD's `## Phases`. */
export interface PrdPhase {
  /** Full heading text, e.g. "Phase 1 — Core structs and parser". */
  name: string;
  loops: PrdLoop[];
}

/**
 * A single checklist item inside a phase — the atomic unit the
 * promote-to-loop action acts on.
 */
export interface PrdLoop {
  title: string;
  checked: boolean;
  /** Read-only sync from loops.md History: true when a history goal matches. */
  done_in_history: boolean;
  /**
   * Stable, project-scoped loop ID `<prd-short-slug>/<loop-slug>`, parsed from
   * a leading backtick token. Undefined for legacy ID-less items — those cannot
   * be promoted. Titles are presentation; the ID is identity (Phase 3 join key).
   */
  id?: string;
}

/** Tab navigation within ProjectDetail. */
export type DetailTab = "overview" | "decisions" | "loops" | "epics" | "agent" | "graph";

/** Token usage + cost for an assistant turn (mirrors Rust UsageInfo). */
export interface UsageInfo {
  input_tokens: number;
  output_tokens: number;
  total_cost_usd: number;
}

/** Structured result from an agent turn (mirrors Rust AgentResponse). */
export interface AgentResponse {
  /** Concatenated assistant text deltas. */
  text: string;
  /** Raw thinking content, if the model returned any. */
  thinking: string | null;
  /** The complete final answer from the `result` event. */
  result: string;
  /** Token usage + cost, when reported. */
  usage: UsageInfo | null;
  /** Whether the turn ended in an error. */
  is_error: boolean;
  /** Wall-clock duration of the turn in milliseconds. */
  duration_ms: number;
  /** Claude session id (drives `--resume` across restarts). */
  session_id: string;
}

/** A streaming event emitted by `agent_send_message_streaming` via Tauri Channel.
 * Mirrors Rust `ClaudeEvent` enum (tagged union, discriminated by `type`). */
export type ClaudeEvent =
  | { type: "text_delta"; text: string }
  | { type: "thinking_delta"; thinking: string }
  | { type: "tool_use"; name: string; input: string }
  | { type: "task_update"; task: TaskRecord }
  | { type: "permission_request" } & PermissionDecision
  | { type: "ask_user_question"; request_id: string; tool_name: string; questions: AskUserQuestionSpec[] }
  | {
      type: "retrying";
      /** 1-based index of the attempt about to run (e.g. 2 for the first retry). */
      attempt: number;
      /** Configured maximum number of attempts (incl. the initial one). */
      max_attempts: number;
      /** Backoff in ms that was slept before this retry fires. */
      backoff_ms: number;
      /** The error text from the failed attempt that triggered the retry. */
      error: string;
    }
  | {
      type: "result";
      text: string;
      thinking: string | null;
      result: string;
      usage: UsageInfo | null;
      is_error: boolean;
      duration_ms: number;
      session_id: string;
    };

/**
 * One selectable option in an `AskUserQuestion` question.
 * Mirrors Rust `AskUserQuestionOption`.
 */
export interface AskUserQuestionOption {
  /** Short pickable text, e.g. `"suprie/ngopi-yuk"`. */
  label: string;
  /** Longer explanation of what the option entails. */
  description: string;
}

/**
 * One question surfaced to the user via the `AskUserQuestion` tool.
 * Mirrors Rust `AskUserQuestionSpec`. Note the snake_case `multiSelect` is
 * preserved verbatim from the wire (Claude's contract uses camelCase here).
 */
export interface AskUserQuestionSpec {
  /** The full question text. Also the key in the `answers` map sent back. */
  question: string;
  /** Short tag rendered as a chip above the question. */
  header: string;
  /** The selectable options. */
  options: AskUserQuestionOption[];
  /** Whether the user may select multiple options (checkbox vs radio). */
  multiSelect: boolean;
}

/**
 * The user's answer to a single `AskUserQuestion` question, as sent back to
 * the backend via `agent_answer_question`. `labels` holds the selected canned
 * option(s); `otherText` holds free-text when the "Other…" affordance is used.
 */
export interface AskUserQuestionAnswer {
  /** Selected option label(s). One for single-select; 0+ for multi-select. */
  labels: string[];
  /** Free-text from the "Other…" input, when used. */
  otherText?: string;
}

/** Answers keyed by question text — the shape `agent_answer_question` expects. */
export type AskUserQuestionAnswers = Record<string, AskUserQuestionAnswer>;

/**
 * One project's pending `AskUserQuestion`, surfaced across the whole registry
 * by `listPendingQuestions`. Mirrors Rust `PendingQuestionEntry`. Carries the
 * project `path` so the frontend can route the answer back to
 * `agentAnswerQuestion`.
 */
export interface PendingQuestionEntry {
  /** Canonical registered project path. */
  path: string;
  requestId: string;
  questions: AskUserQuestionSpec[];
}

/**
 * One project's pending manual-approval request, surfaced across the whole
 * registry by `listPendingPermissions`. Mirrors Rust
 * `PendingPermissionEntry`. Carries the project `path` so the frontend can
 * route the Allow/Deny verdict back to `agentAnswerPermission`.
 */
export interface PendingPermissionEntry {
  /** Canonical registered project path. */
  path: string;
  requestId: string;
  toolName: string;
  input: string;
}

/**
 * Narrowing helper: true if a `ClaudeEvent` is an `ask_user_question`.
 * (TypeScript can't narrow a union of object literals by a string `type` field
 * without a little help when the types come from a mapped union.)
 */
export function isAskUserQuestionEvent(
  e: ClaudeEvent,
): e is ClaudeEvent & { type: "ask_user_question" } {
  return e.type === "ask_user_question";
}

/** A tool call rendered in the streaming activity list. */
export interface ToolCall {
  /** Tool name, e.g. "Read", "Edit", "Bash". */
  name: string;
  /** Raw tool input as a JSON string (from the backend). */
  input: string;
}

/**
 * One task lifecycle event captured from a `tool_use_result.task` line.
 *
 * Claude's Task/TodoWrite tool returns structured task state as a tool result;
 * we persist one `TaskRecord` per event so the transcript records task creates
 * / updates alongside the tool calls that produced them. Mirrors Rust
 * `conversation::TaskRecord`. Foundation for a future live Tasks panel — kept
 * as a flat per-turn list for now.
 */
export interface TaskRecord {
  /** Claude's stable task identifier, e.g. `"10"`. Correlates updates to creates. */
  id: string;
  /** The task title / subject text. */
  subject: string;
  /** What happened — `"created"`, `"updated"`, `"completed"`, `"deleted"`, or a
   *  best-effort raw verb. Never empty. */
  status: string;
}

/**
 * A single assistant content block, recorded in **arrival order**.
 *
 * Mirrors Rust `ContentBlockRecord`. The ordered sequence is the canonical
 * view used for rendering — it preserves how a turn interleaved (e.g.
 * thinking → text → tool_use → thinking → text) instead of the fixed
 * grouping the flattened `text` / `thinking` / `tool_calls` fields force.
 */
export type ContentBlock =
  | { type: "text"; text: string }
  | { type: "thinking"; thinking: string }
  | { type: "tool_use"; name: string; input: string };

/**
 * A permission decision made by the LoopDeck policy layer in response to a
 * Claude `control_request`. Ephemeral narration — not part of the persisted
 * transcript.
 *
 * - `"pending"` — a mutating/executing tool (Bash/Edit/Write/…) needs manual
 *   approval; the agent turn is parked until the user resolves it. Emitted
 *   BEFORE the `control_response` is written.
 * - `"allow"` / `"deny"` — the resolved verdict. For manual-approval tools
 *   this is emitted a second time (after the user's choice); for auto-decided
 *   tools (read-only under allow-by-default, or destructive-floor denies) it's
 *   the only emission and serves as post-hoc narration.
 */
export interface PermissionDecision {
  /** Matches the originating control_request. */
  request_id: string;
  /** Tool name, e.g. "Bash", "Edit". */
  tool_name: string;
  /** Raw tool input as a JSON string. */
  input: string;
  /** `"pending"` (awaiting user), or `"allow"` / `"deny"` (resolved). */
  decision: "allow" | "deny" | "pending";
  /** Why LoopDeck allowed/denied. Empty for pending and plain allows. */
  reason: string;
}

/**
 * The user's verdict on a pending manual-approval request, sent back via
 * `agent_answer_permission`. `allow: false` denies the tool call; `reason`
 * is optional and only meaningful on a deny (surfaced to the model).
 */
export interface ApprovalDecision {
  allow: boolean;
  reason?: string;
}

/**
 * One row in the conversation history list (mirrors Rust `ConversationSummary`).
 *
 * `id` is `"active"` for the live transcript or an archive stem (e.g.
 * `"archive-20260703T101811Z"`); pass it to `agentGetConversationById` to load
 * the turns. Built by `agent_list_conversations`, sorted newest-first.
 */
export interface ConversationSummary {
  /** `"active"` or an archive stem — the handle for loading turns. */
  id: string;
  /** `"active"` for the live transcript, `"archived"` otherwise. */
  kind: "active" | "archived";
  /** ISO-8601 timestamp of the first turn. Empty when no turns survived. */
  started_ts: string;
  /** ISO-8601 timestamp of the last turn. Drives newest-first sort. */
  last_ts: string;
  /** Total turn count (user + assistant) for the row badge. */
  turn_count: number;
  /** First user prompt, truncated to one line — the row preview. */
  first_user_excerpt: string;
}

export interface ConversationTurn {
  /** ISO-8601 timestamp. */
  ts: string;
  /** "user" for prompts, "assistant" for replies. */
  role: "user" | "assistant";
  /**
   * Who originated a user turn:
   * - `"user"` (default, including old transcripts): the human typed it into
   *   the composer.
   * - `"loop"`: the backend auto-built it from `.loopdeck/loops.md` when the
   *   user clicked "Start next loop". Rendered as a compact system row, not a
   *   chat bubble, so the long boilerplate doesn't drown out real messages.
   * Empty string on assistant turns.
   */
  source?: "user" | "loop" | "";
  /** Turn body — user prompt text or assistant result text. */
  text: string;
  /** Claude session id (assistant turns only; absent on user turns). */
  session_id?: string;
  /** Whether the assistant turn was an error. Always false for user turns. */
  is_error?: boolean;
  /**
   * Sub-classifier for an interrupted assistant turn (an `is_error` turn that
   * never reached a terminal result). Tells the *reason* so the UI can render a
   * truthful message instead of blaming every interruption on a process crash:
   * - `"process_exited"`: the agent process exited before responding.
   * - `"approval_timeout"`: a parked manual approval expired and was auto-denied.
   * - `"question_timeout"`: a parked AskUserQuestion expired and was auto-denied.
   * Undefined on normal turns and on legacy transcripts (written before the
   * field existed) — the UI treats undefined as the historical generic
   * interruption when `is_error`.
   */
  interrupt_kind?: "process_exited" | "approval_timeout" | "question_timeout";
  /** Token usage + cost (assistant turns only, when reported). */
  usage?: UsageInfo;
  /** Wall-clock duration in milliseconds (assistant turns only). */
  duration_ms?: number;
  /** The model's extended-thinking chain (assistant turns only, when present). */
  thinking?: string;
  /** Tool calls the assistant made during the turn, in order. */
  tool_calls?: ToolCall[];
  /**
   * The assistant content blocks for the turn, in arrival order. The canonical
   * order-preserving view; `text` / `thinking` / `tool_calls` are the flattened
   * legacy view. Old transcripts without this key load as undefined — the UI
   * then falls back to the flattened fields.
   */
  blocks?: ContentBlock[];
  /**
   * Task lifecycle events (Task/TodoWrite creates / updates) observed during
   * the turn, in arrival order. Empty/absent on turns with no task activity;
   * old transcripts load as undefined.
   */
  tasks?: TaskRecord[];
}
