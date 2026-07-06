import { create } from "zustand";
import type { ClaudeEvent, ContentBlock, TaskRecord } from "../types";

/**
 * Per-project in-flight streaming state, lifted OUT of `AgentPanel`'s React
 * state into a navigation-stable Zustand store keyed by project path.
 *
 * WHY THIS STORE EXISTS:
 * `AgentPanel` unmounts when the user navigates away from `/agent` (or the
 * project-detail Agent tab). Previously that meant:
 * 1. `busy` / `streamingBlocks` / `streamingResult` were destroyed, so coming
 *    back showed an empty transcript mid-turn (the assistant reply only lands
 *    on disk when the turn completes — the in-progress text existed only in
 *    React state).
 * 2. The Tauri Channel `onmessage` callback bailed on `mountedRef.current =
 *    false`, so every streaming event after navigation was dropped on the
 *    floor — including the terminal `result` that should have triggered the
 *    transcript reload.
 *
 * By moving streaming state here, the Channel callback (owned by whoever kicked
 * off the turn) can keep writing to the store even with no panel mounted, and
 * a freshly-mounted panel picks up the in-flight bubble verbatim.
 *
 * `turns` is NOT stored here — that's the persisted transcript (re-loaded from
 * disk on mount). Only the ephemeral streaming accumulator lives here.
 *
 * Not persisted to localStorage: streaming state is, by definition, only
 * meaningful for a live process.
 */
export interface StreamingState {
  /** True while a turn is in flight. Source of truth for disabling the composer. */
  busy: boolean;
  /**
   * Currently accumulating content blocks for the in-flight turn, in arrival
   * order. `null` means "no turn streaming" — hides the streaming bubble.
   * Each text/thinking delta coalesces into the trailing same-type block; each
   * tool_use pushes a new block. This is what lets the bubble render a turn
   * exactly as it unfolded (thinking → text → tool_use → …).
   */
  streamingBlocks: ContentBlock[] | null;
  /**
   * The terminal Result event, if it has arrived while the streaming bubble is
   * still mounted. Carries usage, duration, and is_error for the meta row.
   */
  streamingResult: (ClaudeEvent & { type: "result" }) | null;
  /** Error banner text. */
  error: string | null;
  /**
   * Live task lifecycle state for the in-flight turn, keyed by task id.
   *
   * The agent emits a `task_update` event on every TodoWrite create / update /
   * complete / delete. The transcript renders each as an arrival-ordered
   * activity row (good for "what happened when"), but a turn's tasks also want
   * a *current state* view — "which tasks exist right now and what's their
   * status?" — which is what the floating TaskPanel shows. We keep the latest
   * record per id here (last write wins, exactly like the agent's own todo
   * store), so a created→completed→updated sequence collapses to a single
   * "completed" row keyed by id, instead of three rows.
   *
   * Cleared on turn begin (each turn gets a fresh todo set) and on `clear`.
   * Not persisted — like the rest of streaming state it's only meaningful for
   * a live process.
   */
  tasks: Record<string, TaskRecord>;
}

interface StreamingStateMap {
  /** Per-project streaming state, keyed by absolute project path. */
  byPath: Record<string, StreamingState>;

  // ── Reads ──
  get: (path: string) => StreamingState;

  // ── Mutations (per-path) ──
  /** Begin a new turn: busy=true, blocks=[], result=null, error=null, tasks={}. */
  beginTurn: (path: string) => void;
  /** Patch arbitrary fields (used for result, error, busy). */
  patch: (path: string, patch: Partial<StreamingState>) => void;
  /** Append-or-coalesce a streaming block. */
  appendBlock: (path: string, block: ContentBlock) => void;
  /** Coalesce a text delta into the trailing text block (or push a new one). */
  appendTextDelta: (path: string, text: string) => void;
  /** Coalesce a thinking delta into the trailing thinking block. */
  appendThinkingDelta: (path: string, thinking: string) => void;
  /**
   * Apply a task lifecycle event: insert-or-replace by id (last write wins).
   * Deletions remove the id so the panel reflects the agent's own todo state.
   */
  applyTask: (path: string, task: TaskRecord) => void;
  /** Clear all streaming state for a path (turn fully done + reloaded). */
  clear: (path: string) => void;
}

const EMPTY: StreamingState = {
  busy: false,
  streamingBlocks: null,
  streamingResult: null,
  error: null,
  tasks: {},
};

export const useStreamingState = create<StreamingStateMap>((set, get) => ({
  byPath: {},

  get: (path) => get().byPath[path] ?? EMPTY,

  beginTurn: (path) =>
    set((s) => ({
      byPath: {
        ...s.byPath,
        [path]: {
          busy: true,
          streamingBlocks: [],
          streamingResult: null,
          error: null,
          // Fresh todo set each turn — a new turn's TodoWrite starts clean.
          tasks: {},
        },
      },
    })),

  patch: (path, patch) =>
    set((s) => {
      const cur = s.byPath[path] ?? EMPTY;
      return {
        byPath: { ...s.byPath, [path]: { ...cur, ...patch } },
      };
    }),

  applyTask: (path, task) =>
    set((s) => {
      const cur = s.byPath[path] ?? EMPTY;
      const next = { ...cur.tasks };
      // `deleted` tasks drop out of the agent's todo list, so mirror that here
      // — the panel should reflect "what exists now," not "what ever existed."
      if (task.status === "deleted") {
        delete next[task.id];
      } else {
        next[task.id] = task;
      }
      return {
        byPath: { ...s.byPath, [path]: { ...cur, tasks: next } },
      };
    }),

  appendBlock: (path, block) =>
    set((s) => {
      const cur = s.byPath[path] ?? EMPTY;
      const next = cur.streamingBlocks ?? [];
      return {
        byPath: {
          ...s.byPath,
          [path]: { ...cur, streamingBlocks: [...next, block] },
        },
      };
    }),

  appendTextDelta: (path, text) =>
    set((s) => {
      const cur = s.byPath[path] ?? EMPTY;
      const prev = cur.streamingBlocks ?? [];
      const last = prev[prev.length - 1];
      const blocks =
        last && last.type === "text"
          ? [...prev.slice(0, -1), { type: "text" as const, text: last.text + text }]
          : [...prev, { type: "text" as const, text }];
      return {
        byPath: { ...s.byPath, [path]: { ...cur, streamingBlocks: blocks } },
      };
    }),

  appendThinkingDelta: (path, thinking) =>
    set((s) => {
      const cur = s.byPath[path] ?? EMPTY;
      const prev = cur.streamingBlocks ?? [];
      const last = prev[prev.length - 1];
      const blocks =
        last && last.type === "thinking"
          ? [
              ...prev.slice(0, -1),
              { type: "thinking" as const, thinking: last.thinking + thinking },
            ]
          : [...prev, { type: "thinking" as const, thinking }];
      return {
        byPath: { ...s.byPath, [path]: { ...cur, streamingBlocks: blocks } },
      };
    }),

  clear: (path) =>
    set((s) => {
      if (!s.byPath[path]) return s;
      const next = { ...s.byPath };
      delete next[path];
      return { byPath: next };
    }),
}));
