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
    | "noProjectsFound";
}

/** Internal app view routing. */
export type AppView = "dashboard" | "import" | "detail";

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
export type DetailTab = "overview" | "decisions" | "loops";
