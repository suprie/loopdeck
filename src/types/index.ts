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

/** A tool call rendered in the streaming activity list. */
export interface ToolCall {
  /** Tool name, e.g. "Read", "Edit", "Bash". */
  name: string;
  /** Raw tool input as a JSON string (from the backend). */
  input: string;
}
export interface ConversationTurn {
  /** ISO-8601 timestamp. */
  ts: string;
  /** "user" for prompts, "assistant" for replies. */
  role: "user" | "assistant";
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
}
