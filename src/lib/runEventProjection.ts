/**
 * Provider-neutral run-event projection.
 *
 * Pure, dependency-free reducer used by the migrated run-queue consumer
 * (`src/hooks/useRunQueueEvents.ts`). Keeping it free of Zustand/React means
 * `node --experimental-strip-types --test` can exercise replay semantics
 * without a DOM, and `tsc` checks it against the generated Rust bindings.
 *
 * The reducer is intentionally idempotent by `event_id`: replaying the same
 * event twice produces the same state (compare by reference — the hook skips
 * unchanged projections). Distinct terminals are allowed across retry
 * segments; the backend's normalizer already guarantees one terminal per
 * segment, and a retry is a new segment.
 */
import type {
  AskUserQuestionSpec,
  ClaudeEvent,
  ContentBlock,
  TaskRecord,
  UsageInfo,
} from "../types";
import type { RunEvent, RunEventPayload } from "../types/runEvent";

export interface PendingPermissionProjection {
  requestId: string;
  toolName: string;
  input: string;
}

export interface PendingQuestionProjection {
  requestId: string;
  questions: AskUserQuestionSpec[];
}

export interface PendingPlanProjection {
  requestId: string;
  plan: string;
}

export interface RetryingProjection {
  attempt: number;
  maxAttempts: number;
  backoffMs: number;
  error: string;
}

export interface RunEventProjectionState {
  busy: boolean;
  blocks: ContentBlock[];
  tasks: Record<string, TaskRecord>;
  retrying: RetryingProjection | null;
  /** Synthesized legacy-shaped terminal, kept so existing render components work. */
  result: (ClaudeEvent & { type: "result" }) | null;
  error: string | null;
  permission: PendingPermissionProjection | null;
  question: PendingQuestionProjection | null;
  plan: PendingPlanProjection | null;
  usage: UsageInfo | null;
  /** Event IDs already applied; makes duplicate delivery a no-op. */
  applied: Record<string, true>;
}

export function emptyRunEventProjection(): RunEventProjectionState {
  return {
    busy: false,
    blocks: [],
    tasks: {},
    retrying: null,
    result: null,
    error: null,
    permission: null,
    question: null,
    plan: null,
    usage: null,
    applied: {},
  };
}

/** Start a fresh streaming turn while preserving applied-event dedupe. */
function begin(state: RunEventProjectionState): RunEventProjectionState {
  if (state.busy) return state;
  return {
    ...state,
    busy: true,
    blocks: [],
    tasks: {},
    retrying: null,
    result: null,
    error: null,
    permission: null,
    question: null,
    plan: null,
    usage: null,
  };
}

function appendText(blocks: ContentBlock[], text: string): ContentBlock[] {
  const last = blocks[blocks.length - 1];
  if (last && last.type === "text") {
    return [...blocks.slice(0, -1), { type: "text", text: last.text + text }];
  }
  return [...blocks, { type: "text", text }];
}

function appendThinking(blocks: ContentBlock[], thinking: string): ContentBlock[] {
  const last = blocks[blocks.length - 1];
  if (last && last.type === "thinking") {
    return [
      ...blocks.slice(0, -1),
      { type: "thinking", thinking: last.thinking + thinking },
    ];
  }
  return [...blocks, { type: "thinking", thinking }];
}

/** Tool rows that the legacy consumer suppressed because they have a dedicated surface. */
const SUPPRESSED_TOOL_ROWS = new Set(["AskUserQuestion", "TaskCreate", "TaskUpdate"]);

function parseJson<T>(text: string | null | undefined): T | null {
  if (!text) return null;
  try {
    return JSON.parse(text) as T;
  } catch {
    return null;
  }
}

function terminalState(
  state: RunEventProjectionState,
  payload: Extract<RunEventPayload, { type: "terminal" }>,
  event: RunEvent,
): RunEventProjectionState {
  const message = payload.message_redacted ?? "";
  const isError =
    payload.outcome === "error" ||
    payload.outcome === "cancelled" ||
    payload.outcome === "killed";
  const accumulatedText = state.blocks
    .filter((block): block is Extract<ContentBlock, { type: "text" }> => block.type === "text")
    .map((block) => block.text)
    .join("");
  // Always build the result from the newest terminal. A retried turn emits a
  // terminal per attempt; the final attempt's message/outcome must replace the
  // failed attempt's, not be hidden behind it.
  const streamedResult: ClaudeEvent & { type: "result" } = {
    type: "result",
    text: accumulatedText,
    thinking: null,
    result: message,
    usage: state.usage,
    is_error: isError,
    duration_ms: 0,
    session_id: "",
  };
  return {
    ...state,
    busy: false,
    retrying: null,
    result: streamedResult,
    error: isError ? message || "The unattended turn failed." : null,
    permission: null,
    question: null,
    plan: null,
    applied: { ...state.applied, [event.event_id]: true } as Record<string, true>,
  };
}

/**
 * Reduce one normalized event into the consumer's projection.
 *
 * Returns the same object reference when the event is a duplicate or not
 * meant for the current streaming surface (e.g. `unsupported`), letting the
 * hook skip a redundant store write.
 */
export function projectRunEvent(
  state: RunEventProjectionState,
  event: RunEvent,
): RunEventProjectionState {
  if (event.version !== 1) return state;
  if (state.applied[event.event_id]) return state;

  const applied: Record<string, true> = {
    ...state.applied,
    [event.event_id]: true,
  };
  const withApplied = (next: RunEventProjectionState) => ({ ...next, applied });
  const payload = event.payload;

  switch (payload.type) {
    case "started":
      return withApplied(begin(state));

    case "content": {
      const base = begin(state);
      const blocks =
        payload.kind === "text"
          ? appendText(base.blocks, payload.text)
          : appendThinking(base.blocks, payload.text);
      return withApplied({ ...base, blocks });
    }

    case "tool": {
      const base = begin(state);
      // The normalized contract has no separate task variant: a TaskUpdate is
      // tool activity whose input carries the task state. Keep the legacy
      // TaskPanel populated instead of silently dropping it.
      if (payload.name === "TaskUpdate") {
        const parsed = parseJson<{
          taskId?: string;
          subject?: string;
          status?: string;
        }>(payload.input_redacted);
        if (!parsed?.taskId) return withApplied(base);
        const tasks = { ...base.tasks };
        if (parsed.status === "deleted") {
          delete tasks[parsed.taskId];
        } else {
          tasks[parsed.taskId] = {
            id: parsed.taskId,
            subject: parsed.subject || tasks[parsed.taskId]?.subject || "",
            status: parsed.status ?? "updated",
          };
        }
        return withApplied({ ...base, tasks });
      }
      if (SUPPRESSED_TOOL_ROWS.has(payload.name)) {
        return withApplied(base);
      }
      return withApplied({
        ...base,
        blocks: [
          ...base.blocks,
          { type: "tool_use", name: payload.name, input: payload.input_redacted },
        ],
      });
    }

    case "decision": {
      const base = begin(state);
      const detail = payload.detail_redacted ?? null;
      if (payload.kind === "approval") {
        if (payload.state === "pending") {
          // Approval detail is `{ tool_name, input }` (input is redacted JSON
          // text), so the pending card keeps the exact payload the backend saw.
          const parsed = parseJson<{ tool_name?: string; input?: unknown }>(detail);
          const toolName =
            parsed?.tool_name ??
            (payload.summary?.split(":")[0]?.trim() || "Tool");
          const input =
            typeof parsed?.input === "string"
              ? parsed.input
              : parsed?.input !== undefined
                ? JSON.stringify(parsed.input)
                : "";
          return withApplied({
            ...base,
            permission: {
              requestId: payload.decision_id,
              toolName,
              input,
            },
          });
        }
        return withApplied({ ...base, permission: null });
      }

      if (payload.kind === "question") {
        if (payload.state === "pending") {
          const questions = parseJson<AskUserQuestionSpec[]>(detail) ?? [];
          return withApplied({
            ...base,
            question: { requestId: payload.decision_id, questions },
          });
        }
        return withApplied({ ...base, question: null });
      }

      // plan_approval
      if (payload.state === "pending") {
        return withApplied({
          ...base,
          plan: { requestId: payload.decision_id, plan: detail ?? "" },
        });
      }
      return withApplied({ ...base, plan: null });
    }

    case "usage":
      return withApplied({
        ...state,
        usage: {
          input_tokens: payload.input_tokens,
          output_tokens: payload.output_tokens,
          total_cost_usd: payload.total_cost_usd ?? 0,
        },
      });

    case "retrying":
      return withApplied({
        ...begin(state),
        retrying: {
          attempt: payload.attempt,
          maxAttempts: payload.max_attempts,
          // The normalized retry signal does not carry backoff duration (it
          // is a transient UI hint); renderers show the attempt count.
          backoffMs: 0,
          error: payload.error_redacted,
        },
      });

    case "terminal":
      return terminalState(state, payload, event);

    case "unsupported":
      // The legacy stores have no surface for an explicit capability gap; it
      // stays available to consumers that render it (morning report).
      return state;
  }
}
