// TypeScript types mirroring Rust structs from src-tauri/src/

/** A repository discovered during directory scanning. */
export interface DiscoveredRepo {
  path: string;
  name: string;
  markers: string[];
  has_readme: boolean;
  has_loopdeck: boolean;
  /** Human-readable technology stack, e.g. "Rust, JavaScript/TypeScript". */
  detected_stack: string;
  /** Lightweight description preview generated from the detected stack. */
  description_preview: string;
  last_commit: string | null;
  last_modified: string | null;
}

/** Project status from Rust ProjectStatus enum. */
export type ProjectStatus = "active" | "archived" | "nonactive" | "warning";

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
}

/** Content of .loopdeck/project.yaml. */
export interface ProjectMeta {
  name: string;
  description: string;
  status: string;
  created_at: string;
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
    | "agent";
}

/** Global agent configuration (mirrors Rust AgentConfig). */
export interface AgentConfig {
  auth_token?: string;
  base_url?: string;
  model?: string;
  effort?: string;
}

/** Internal app view routing. */
export type AppView = "dashboard" | "import" | "detail" | "settings" | "agent" | "activity" | "decisions" | "loops";

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

/** Full loop status from .loopdeck/loops.md. */
export interface LoopStatus {
  current: Loop | null;
  next_steps: string[];
  history: Loop[];
}

/** Tab navigation within ProjectDetail. */
export type DetailTab = "overview" | "decisions" | "loops" | "agent";

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
