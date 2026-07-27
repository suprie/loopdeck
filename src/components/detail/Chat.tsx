import { useRef, useEffect, useState } from "react";
import {
  Bot,
  User,
  AlertTriangle,
  Clock,
  Loader2,
  Send,
  Brain,
  ChevronDown,
  ChevronRight,
  ShieldCheck,
  ShieldX,
  ShieldPlus,
  Repeat,
  RotateCw,
  ListChecks,
  CheckCircle2,
  XCircle,
} from "lucide-react";
import type {
  ConversationTurn,
  ClaudeEvent,
  ContentBlock,
  ToolCall,
  AskUserQuestionSpec,
  AskUserQuestionAnswers,
  ApprovalDecision,
  PlanApprovalDecision,
} from "../../types";
import { Markdown } from "../shared/Markdown";
import { FileMentionMenu, useFileMention } from "./FileMentionMenu";
import { SkillMenu, useSkillDiscovery } from "./SkillMenu";
import { AskUserQuestionCard } from "./AskUserQuestionCard";

// ── Helpers ──────────────────────────────────────────────────────────────────

/**
 * Detect a transient gateway-overload error in a result/turn body text.
 *
 * Mirrors the backend `retry::is_overloaded` substring match (529 / overloaded)
 * so the frontend can render the friendly amber bubble — with a Retry button —
 * instead of the raw `API Error: 529 [...]` text in a destructive-red bubble.
 * Client-side detection keeps the transcript truthful (the raw text is still
 * recorded) without threading a new `error_kind` field through `AgentResponse`.
 */
function isOverloadError(text: string | null | undefined): boolean {
  if (!text) return false;
  const lower = text.toLowerCase();
  return lower.includes("529") || lower.includes("overloaded");
}

// ── Types ────────────────────────────────────────────────────────────────────

export interface ChatProps {
  /** Canonical project root path — used to resolve `@`-mention file paths in
   *  the composer's autocomplete (relative to this directory). */
  projectPath: string;
  /** Completed conversation turns to display. */
  turns: ConversationTurn[];
  /**
   * Currently accumulating streaming content blocks, in arrival order
   * (null when nothing is streaming — hides the streaming bubble). Each
   * text/thinking delta coalesces into the trailing same-type block; each
   * tool_use is its own block. This is what lets the bubble render a turn
   * exactly as it unfolded instead of a fixed thinking→tools→text grouping.
   */
  streamingBlocks: ContentBlock[] | null;
  /**
   * The terminal Result event, if it has arrived while the streaming bubble is
   * still visible (arrives before the transcript reload replaces it). Carries
   * usage, duration, and is_error for the meta row. */
  streamingResult: (ClaudeEvent & { type: "result" }) | null;
  /**
   * The user's message for the in-flight turn, rendered as an ephemeral user
   * bubble above the streaming bubble. This is carried OUTSIDE `turns` (which
   * only holds canonical disk records) so the optimistic display can't race
   * with the transcript reload and double the bubble. Clears when the turn ends
   * and the canonical user turn lands in `turns`. `null` for loop starts.
   */
  pendingUserText: string | null;
  /** Whether a request is currently in flight (Start or Send). */
  busy: boolean;
  /**
   * Whether this project is in autonomous mode. When true, mutating tool calls
   * (Bash, Edit, Write, MCP) render an amber "auto-approved" tag — a per-turn
   * audit cue so the morning review can see at a glance what the agent did
   * without asking. Derived from the project flag (not per-event) so it's
   * stable across a turn; precise per-event attribution is a follow-up.
   */
  autonomous?: boolean;
  /**
   * Active transient-overload retry state for the in-flight turn, or `null`.
   * When set, renders an inline "Retrying 2/9 in 4s… (overloaded)" row so the
   * agent doesn't look frozen during a gateway 529 backoff window. Cleared by
   * the next terminal result.
   */
  retrying: {
    attempt: number;
    maxAttempts: number;
    backoffMs: number;
    error: string;
  } | null;
  /** Error message to show as a banner above the transcript. */
  error: string | null;
  /** Called when the user sends a message from the composer. */
  onSend: (text: string) => void;
  /** Called when the user clears the error banner. */
  onClearError?: () => void;
  /** Whether the composer and Start button should be disabled. */
  disabled?: boolean;
  /**
   * Bump this value to programmatically focus the composer (e.g. after the
   * parent archives the transcript for a fresh conversation). The effect keys
   * off this nonce, so re-focusing after back-to-back "New conversation"
   * presses still fires.
   */
  focusNonce?: number;
  /**
   * A pending `AskUserQuestion` from the agent — when non-null, the question
   * card is rendered above the composer and the composer is disabled (the
   * agent is parked waiting for the answer, not for free-form input).
   */
  pendingQuestion?: AskUserQuestionSpec[] | null;
  /**
   * Called when the user submits the question card. The parent forwards the
   * answers to the backend, which un-parks the agent turn.
   */
  onAnswerQuestion?: (answers: AskUserQuestionAnswers) => void;
  /**
   * Read-only mode for viewing archived conversations. Disables the composer
   * and swaps the empty-state copy — the user can read history but not send.
   * The parent sets this when a non-active conversation is selected.
   */
  readOnly?: boolean;
  /**
   * A pending manual-approval request from the agent — when non-null, the
   * approval card is rendered above the streaming bubble and the composer is
   * disabled. The agent turn is parked until the user picks Allow or Deny.
   */
  pendingPermission?: { toolName: string; input: string } | null;
  /**
   * Called when the user clicks Allow / Deny on the approval card. The parent
   * forwards the verdict to the backend, which writes the control_response
   * and resumes (or recovers from) the turn.
   */
  onAnswerPermission?: (decision: ApprovalDecision) => void;
  /**
   * "Always allow" callback — fires when the user clicks the Always-allow
   * button. The parent resolves the current approval (same as a plain Allow)
   * AND persists a permission rule (built from `toolName`/`input` via
   * `buildAllowRule`) so future calls of the same tool/command don't prompt
   * again. Optional — when omitted, the button is hidden.
   */
  onAlwaysAllow?: (toolName: string, input: string) => void;
  /**
   * A pending `ExitPlanMode` request from the agent — when non-null, the
   * plan-approval card is rendered above the composer (same pinned strip as
   * the question/permission cards) and the composer is disabled. The agent
   * turn is parked until the user Approves or Rejects the plan.
   */
  pendingPlan?: { plan: string } | null;
  /**
   * Called when the user clicks Approve / Reject on the plan card. The parent
   * forwards the verdict to the backend, which writes the control_response —
   * approve lets the agent start executing, reject (with optional feedback)
   * keeps it in plan mode to revise.
   */
  onAnswerPlan?: (decision: PlanApprovalDecision) => void;
  /**
   * Whether the NEXT message sent from the composer should run under the
   * CLI's `plan` permission mode (mirrors Claude Code's shift-tab toggle).
   * Omitted entirely (rather than defaulting) hides the toggle button — a
   * parent that doesn't wire plan mode gets the pre-existing composer as-is.
   */
  planMode?: boolean;
  /** Called when the user clicks the Plan-mode toggle button. */
  onTogglePlanMode?: () => void;
}

// ── Helpers ──────────────────────────────────────────────────────────────────

/** Format milliseconds to a human-readable duration string. */
function fmtDuration(ms: number): string {
  if (ms < 1000) return `${ms}ms`;
  if (ms < 60_000) return `${(ms / 1000).toFixed(1)}s`;
  const mins = Math.floor(ms / 60_000);
  const secs = Math.round((ms % 60_000) / 1000);
  return `${mins}m ${secs}s`;
}

/** Strip ANSI escape codes (claude sometimes emits them even in stream-json). */
function sanitise(text: string): string {
  return text.replace(/\x1b\[[0-9;]*[a-zA-Z]/g, "");
}

/**
 * Extract the human-readable subject from an auto-built loop prompt.
 *
 * `build_next_loop_prompt` (commands.rs) wraps the actual step in heavy
 * boilerplate: `You are working on this LoopDeck project. … The next unchecked
 * step is: "STEP HERE". Implement it. When done, update …`. Showing that
 * verbatim as a "system" row is no better than showing it as a user bubble.
 * This pulls out just the step text (the quoted span after `step is:`), or the
 * fallback "propose the next loop" phrasing, so the row reads like a label.
 */
function loopPromptSubject(text: string): string {
  const m = text.match(/next unchecked step is:\s*"([^"]+)"/i);
  if (m) return m[1];
  if (/propose and start the next loop/i.test(text))
    return "Propose & start next loop";
  // Fallback: trim and elide the long boilerplate to a reasonable preview.
  const trimmed = text.trim();
  return trimmed.length > 120 ? trimmed.slice(0, 117) + "…" : trimmed;
}

/**
 * Group consecutive auto-loop prompts into a single collapsed row.
 *
 * Clicking "Start next loop" multiple times before a turn completes produces
 * several identical auto-prompts in a row (the user re-clicked, or the click
 * fired twice). Each is a few hundred chars of boilerplate — rendering them as
 * separate rows recreates the noise we're trying to eliminate. This collapses
 * runs of consecutive `source: "loop"` turns into one row showing the count.
 *
 * Returns a new array where each element is either the original turn (for
 * user-typed and assistant turns) or a synthetic marker carrying the run.
 */
type TranscriptItem =
  | { kind: "turn"; turn: ConversationTurn }
  | { kind: "loop-run"; subject: string; count: number; ts: string };

function groupLoopRuns(turns: ConversationTurn[]): TranscriptItem[] {
  const out: TranscriptItem[] = [];
  let i = 0;
  while (i < turns.length) {
    const t = turns[i];
    if (t.role === "user" && t.source === "loop") {
      const subject = loopPromptSubject(t.text);
      // Collect consecutive loop turns with the SAME subject (dedup) — a
      // different subject means a different step, render separately.
      let count = 1;
      while (
        i + count < turns.length &&
        turns[i + count].role === "user" &&
        turns[i + count].source === "loop" &&
        loopPromptSubject(turns[i + count].text) === subject
      ) {
        count++;
      }
      out.push({ kind: "loop-run", subject, count, ts: t.ts });
      i += count;
    } else {
      out.push({ kind: "turn", turn: t });
      i++;
    }
  }
  return out;
}

/**
 * Render a tool call as a short human-readable summary.
 *
 * Pulls the most useful field out of the tool's input JSON (a file path for
 * Read/Edit/Write, a command for Bash, a query for WebSearch, etc.). Falls
 * back to the raw input string when there's nothing structured to show.
 */
function describeTool(name: string, rawInput: string): string {
  let input: Record<string, unknown> = {};
  try {
    const parsed = JSON.parse(rawInput);
    if (parsed && typeof parsed === "object" && !Array.isArray(parsed)) {
      input = parsed as Record<string, unknown>;
    }
  } catch {
    // Not JSON — fall through to the raw-input fallback below.
  }

  const str = (v: unknown): string =>
    typeof v === "string" ? v : JSON.stringify(v);

  // Order matters: check the most informative field for each tool shape.
  const candidate =
    input.file_path ??
    input.path ??
    input.command ??
    input.pattern ??
    input.query ??
    input.url;
  const detail = candidate
    ? str(candidate)
    : rawInput && rawInput !== "{}"
      ? rawInput
      : "";

  return detail ? `${name} · ${detail}` : name;
}

/**
 * Build a Claude Code permission allow-rule string for "always allow".
 *
 * Mirrors the `CURATED_ALLOW` format in `src-tauri/src/skills.rs` so the rule
 * matches Claude Code's own `--setting-sources` allow-list semantics:
 *
 * - `Bash` → `Bash(<first-command-token>:*)`. We take only the leading command
 *   word (not the full argv), so `git push --force origin main` becomes
 *   `Bash(git:*)` — same granularity as the curated `Bash(git status:*)` /
 *   `Bash(cargo:*)` entries. The `:*` is the wildcard terminator Claude Code's
 *   matcher expects (NOT a glob over the remainder).
 * - File tools (`Read`/`Edit`/`Write`/`Glob`/`Grep`) → `Tool(*)`. The path is
 *   irrelevant for "always allow this tool" intent, and a path-pinned rule
 *   would be too narrow to be useful.
 * - `mcp__server__tool` → the bare tool name (MCP convention — no parens).
 * - Anything else → `Tool(*)` as a safe permissive default.
 *
 * Used by the "Always allow" button on the approval card. The result is sent
 * to `agent_add_allow_rule`, which dedups it into `settings.local.json`.
 */
export function buildAllowRule(toolName: string, rawInput: string): string {
  if (toolName.startsWith("mcp__")) return toolName;
  if (toolName === "Bash") {
    // Extract the command field and take its first whitespace token.
    let command = "";
    try {
      const parsed = JSON.parse(rawInput);
      if (parsed && typeof parsed === "object" && !Array.isArray(parsed)) {
        const cmd = (parsed as Record<string, unknown>).command;
        if (typeof cmd === "string") command = cmd;
      }
    } catch {
      // Not JSON — fall back to the raw input trimmed.
      command = rawInput;
    }
    const firstToken = command.trim().split(/\s+/)[0];
    return firstToken ? `Bash(${firstToken}:*)` : "Bash(*)";
  }
  // Read/Edit/Write/Glob/Grep and anything else: allow the whole tool.
  return `${toolName}(*)`;
}

// ── Sub-components ───────────────────────────────────────────────────────────

/**
 * A collapsed row for one or more consecutive auto-built loop prompts.
 *
 * `build_next_loop_prompt` (Rust) generates long boilerplate user turns every
 * time the user clicks "Start next loop" — these aren't messages the human
 * typed, so rendering them as right-aligned user bubbles buries real input
 * under repetitive machine-generated text. Instead we render a compact,
 * left-aligned system row: a loop icon, the extracted step subject (e.g.
 * "Frontend chat UI"), and — when the click fired more than once before a turn
 * completed — an "× N" repeat badge.
 *
 * It reads as a labelled activity marker ("ran loop: …") rather than a
 * simulated user message, which is what makes the transcript scannable.
 */
function LoopStepRow({ subject, count }: { subject: string; count: number }) {
  return (
    <div className="flex justify-start pl-1">
      <div className="inline-flex items-center gap-2 rounded-md border border-border bg-muted/40 px-2.5 py-1 text-[11px] text-muted-foreground">
        <Repeat className="size-3 text-primary shrink-0" />
        <span className="font-medium">Ran loop</span>
        <span className="text-foreground/80 truncate max-w-md">{subject}</span>
        {count > 1 && (
          <span
            className="ml-1 inline-flex items-center justify-center min-w-[1.25rem] h-4 px-1 rounded-full bg-primary/15 text-primary text-[9px] font-semibold"
            title={`${count} consecutive auto-prompts collapsed`}
          >
            ×{count}
          </span>
        )}
      </div>
    </div>
  );
}

/**
 * Friendly overload-retry banner shown inside an error bubble when the turn
 * failed because the gateway (e.g. `api.z.ai`) was overloaded long enough to
 * exhaust all retries. Replaces the raw `API Error: 529 [...]` text with a
 * plain-language explanation + a **Retry** button that re-sends the last user
 * prompt (which itself runs the full retry loop again, giving the service
 * another window to recover).
 *
 * Amber (not destructive-red) to read as "transient, recoverable" rather than
 * a hard model error. The raw error text is still recorded in the transcript
 * for debugging; this banner just renders on top of it.
 */
function OverloadBanner({ onRetry }: { onRetry: () => void }) {
  return (
    <div className="mb-2 rounded-md border border-amber-500/40 bg-amber-500/10 p-2.5 text-xs">
      <div className="flex items-center gap-1.5 font-medium text-amber-600 dark:text-amber-400">
        <AlertTriangle size={12} />
        <span>Gateway was overloaded</span>
      </div>
      <p className="mt-1 leading-relaxed text-foreground/80">
        The service was overloaded long enough to exhaust automatic retries.
        It may have recovered — try again now.
      </p>
      <button
        type="button"
        onClick={onRetry}
        className="mt-2 inline-flex h-7 items-center gap-1.5 rounded-md bg-amber-500 px-3 text-[11px] font-medium text-amber-950 transition hover:bg-amber-400"
      >
        <RotateCw size={11} />
        Retry now
      </button>
    </div>
  );
}

/**
 * A single completed conversation turn from the persisted transcript.
 *
 * Renders user turns as right-aligned primary bubbles and assistant turns as
 * left-aligned card bubbles with optional error flag, duration, and token usage
 * meta.  This component is used for turns loaded from disk — it does not stream.
 */
function TurnBubble({ turn, onRetryOverload, autonomous = false }: {
  turn: ConversationTurn;
  /** Re-send the last user prompt when the user clicks Retry on an overload
   *  error bubble. Optional — only TurnBubble needs it; absent in read-only
   *  contexts (the button is hidden). */
  onRetryOverload?: () => void;
  /** When true, mutating tool calls render the "auto-approved" tag. Threaded
   *  through to BlockList → ToolUseBlock. */
  autonomous?: boolean;
}) {
  const isUser = turn.role === "user";
  const isError = turn.is_error ?? false;
  // Overload errors get the friendly amber treatment (banner + Retry button)
  // instead of destructive-red. Detected client-side from the result text so
  // the transcript stays truthful without a new error_kind field.
  const isOverload = isError && isOverloadError(turn.text);

  return (
    <div className={`flex gap-2.5 ${isUser ? "flex-row-reverse" : ""}`}>
      {/* Avatar */}
      <div
        className={`size-7 shrink-0 rounded-full grid place-items-center ${
          isUser
            ? "bg-muted text-muted-foreground"
            : "bg-[color-mix(in_oklab,var(--primary)_18%,transparent)] text-[var(--primary)]"
        }`}
      >
        {isUser ? <User className="size-3.5" /> : <Bot className="size-3.5" />}
      </div>

      {/* Bubble */}
      <div
        className={`min-w-0 max-w-[85%] rounded-lg px-3.5 py-2.5 ${
          isUser
            ? "bg-primary text-primary-foreground"
            : isOverload
              ? "bg-amber-500/5 border border-amber-500/30 text-foreground"
              : isError
                ? "bg-[color-mix(in_oklab,var(--destructive)_10%,transparent)] border border-[color-mix(in_oklab,var(--destructive)_28%,transparent)] text-foreground"
                : "bg-card border border-border text-foreground"
        }`}
      >
        {/* Meta row for assistant turns: error flag / duration / usage. */}
        {!isUser && (
          <div className="flex items-center gap-2 mb-1 flex-wrap">
            {isError && (
              <span
                className={`inline-flex items-center gap-1 text-[10px] font-semibold uppercase tracking-wider ${
                  isOverload ? "text-amber-600 dark:text-amber-400" : "text-destructive"
                }`}
              >
                {/* Truthful label per interrupt_kind: the recurring "error"
                    bubbles were mostly approval timeouts, not model errors or
                    process crashes. The body text already explains the cause;
                    this tag is the at-a-glance cue. Overload gets its own amber
                    tag so the bubble reads as transient, not a hard failure. */}
                {isOverload ? (
                  <>
                    <AlertTriangle size={10} /> overloaded
                  </>
                ) : turn.interrupt_kind === "approval_timeout" ||
                  turn.interrupt_kind === "question_timeout" ? (
                  <>
                    <Clock size={10} /> timed out
                  </>
                ) : turn.interrupt_kind === "process_exited" ? (
                  <>
                    <AlertTriangle size={10} /> interrupted
                  </>
                ) : (
                  <>
                    <AlertTriangle size={10} /> error
                  </>
                )}
              </span>
            )}
            {turn.duration_ms != null && turn.duration_ms > 0 && (
              <span className="text-[10px] text-muted-foreground">
                {fmtDuration(turn.duration_ms)}
              </span>
            )}
            {turn.usage && (
              <span className="text-[10px] text-muted-foreground">
                {turn.usage.input_tokens.toLocaleString()}&rarr;
                {turn.usage.output_tokens.toLocaleString()} tok
                {turn.usage.total_cost_usd > 0 &&
                  ` · $${turn.usage.total_cost_usd.toFixed(4)} total`}
              </span>
            )}
          </div>
        )}

        {/* Friendly overload banner: replaces the raw 529 text as the focal
            affordance when the turn died from an exhausted-overload retry.
            Rendered above the (still-truthful) body text. The Retry button
            re-sends the last user prompt, which runs the full retry loop
            again — giving the gateway another window to recover. */}
        {isOverload && onRetryOverload && <OverloadBanner onRetry={onRetryOverload} />}

        {/* Assistant body. Prefer the ordered `blocks` view (arrival-order
            rendering) when the turn recorded it; otherwise fall back to the
            legacy fixed grouping (thinking → tools → text) so old transcripts
            keep rendering exactly as before. User turns render their `text`
            directly via the `isUser` branch below. Task lifecycle rows are
            intentionally NOT rendered here — tasks have their own dedicated
            floating TaskPanel during a live turn, and a per-row list in the
            transcript just duplicated that surface as clutter. The `turn.tasks`
            array is still persisted on disk for a future Tasks panel. */}
        {isUser ? (
          // User turns: render the prompt text once. (Previously this was
          // duplicated — the fallback ternary below also rendered user text,
          // so every user bubble showed its message twice.)
          <p className="text-sm leading-relaxed whitespace-pre-wrap break-words">
            {sanitise(turn.text)}
          </p>
        ) : turn.blocks && turn.blocks.length > 0 ? (
          <BlockList blocks={turn.blocks} autonomous={autonomous} />
        ) : (
          <>
            <ThinkingBlock thinking={turn.thinking ?? ""} />
            <ToolList tools={turn.tool_calls ?? []} />
            <div className="text-sm leading-relaxed break-words">
              <Markdown>{sanitise(turn.text)}</Markdown>
            </div>
          </>
        )}
      </div>
    </div>
  );
}

/**
 * Collapsible thinking section.
 *
 * The model's reasoning chain (extended thinking) can be very long.  We collapse
 * it behind a toggle so it doesn't drown the assistant's actual answer.  The
 * character count gives a hint of how much content is hidden.
 */
function ThinkingBlock({ thinking }: { thinking: string }) {
  const [open, setOpen] = useState(false);

  if (!thinking) return null;

  return (
    <div className="mb-2">
      <button
        onClick={() => setOpen(!open)}
        className="inline-flex items-center gap-1 mb-2 text-[10px] text-muted-foreground hover:text-foreground transition-colors"
      >
        {open ? <ChevronDown size={12} /> : <ChevronRight size={12} />}
        <Brain size={11} />
        <span>{open ? "Hide thinking" : "Show thinking"}</span>
        {!open && (
          <span className="text-[9px] opacity-50">
            ({thinking.length.toLocaleString()} chars)
          </span>
        )}
      </button>
      {open && (
        <div className="p-2.5 rounded-md bg-muted/60 border border-border/60 text-xs text-muted-foreground italic leading-relaxed whitespace-pre-wrap break-words max-h-48 overflow-y-auto font-mono">
          {sanitise(thinking)}
        </div>
      )}
    </div>
  );
}

/**
 * Live activity list of tool calls during a streaming turn.
 *
 * During an agentic turn the model may make many tool calls (Read, Edit, Bash,
 * …) before emitting any text. Rendering them as they arrive gives the user
 * concrete feedback that the agent is working, not stuck. Each entry shows the
 * tool name and the most relevant input field (file path, command, etc.).
 */
function ToolList({ tools }: { tools: ToolCall[] }) {
  // `AskUserQuestion` is rendered as its own pinned question card, not as a
  // transcript activity row — filter it out so old transcripts (which still
  // carry the persisted tool_use entry) don't show a redundant
  // `› AskUserQuestion · {json}` line above where the card was.
  const visible = tools.filter((t) => t.name !== "AskUserQuestion");
  if (visible.length === 0) return null;
  return (
    <ul className="mb-2 space-y-1">
      {visible.map((tool, i) => (
        <li
          key={i}
          className="flex items-start gap-1.5 text-[11px] text-muted-foreground leading-relaxed"
        >
          <span className="text-[var(--primary)] mt-0.5">›</span>
          <span className="font-mono break-all">
            {sanitise(describeTool(tool.name, tool.input))}
          </span>
        </li>
      ))}
    </ul>
  );
}

/**
 * A single tool-use block, rendered as one indented activity row.
 *
 * Shared between `BlockList` (arrival-order rendering) and kept visually
 * identical to the legacy `ToolList` entries so live and persisted turns look
 * the same.
 */
/** Tools that require manual approval under `ConfirmChanges` mode (mirrors the
 *  backend `MANUAL_APPROVAL_TOOLS` + `mcp__*` prefix). Under autonomous mode
 *  these are the calls that were self-approved — tagged "auto-approved" in the
 *  transcript so the morning review can see what ran without asking. */
function wouldRequireApproval(name: string): boolean {
  const MANUAL = ["Bash", "Edit", "Write", "NotebookEdit", "WebFetch"];
  return MANUAL.includes(name) || name.startsWith("mcp__");
}

function ToolUseBlock({
  name,
  input,
  autoApproved = false,
}: {
  name: string;
  input: string;
  /** Show the amber "auto-approved" tag — set when the project is autonomous
   *  AND this tool would normally require manual approval. */
  autoApproved?: boolean;
}) {
  return (
    <div className="mb-2 flex items-start gap-1.5 text-[11px] text-muted-foreground leading-relaxed">
      <span className="text-[var(--primary)] mt-0.5">›</span>
      <span className="font-mono break-all">
        {sanitise(describeTool(name, input))}
      </span>
      {autoApproved && (
        <span
          title="Auto-approved by the autonomous-mode policy (no manual approval needed). The destructive floor still applied."
          className="ml-1 inline-flex shrink-0 items-center gap-0.5 rounded border border-amber-500/40 bg-amber-500/10 px-1 py-px text-[9px] font-medium uppercase tracking-wide text-amber-600 dark:text-amber-400"
        >
          auto
        </span>
      )}
    </div>
  );
}

/**
 * Normalize legacy/provider transcripts that persisted one content block per
 * streamed token. Only adjacent blocks of the same prose kind are merged;
 * tool calls remain hard ordering boundaries.
 */
function coalesceContentBlocks(blocks: ContentBlock[]): ContentBlock[] {
  const coalesced: ContentBlock[] = [];
  for (const block of blocks) {
    const last = coalesced[coalesced.length - 1];
    if (block.type === "text" && last?.type === "text") {
      coalesced[coalesced.length - 1] = {
        type: "text",
        text: last.text + block.text,
      };
    } else if (block.type === "thinking" && last?.type === "thinking") {
      coalesced[coalesced.length - 1] = {
        type: "thinking",
        thinking: last.thinking + block.thinking,
      };
    } else {
      coalesced.push(block);
    }
  }
  return coalesced;
}

/**
 * Render an ordered sequence of assistant content blocks in **arrival order**.
 *
 * This is the order-preserving counterpart to the legacy fixed grouping
 * (`ThinkingBlock` + `ToolList` + a single `<p>`). Each block is rendered in
 * sequence: thinking blocks reuse `ThinkingBlock` (each independently
 * collapsible), tool-use blocks reuse `ToolUseBlock`, text blocks render as
 * paragraphs. While streaming, a blinking cursor is shown after the trailing
 * text block for the typewriter feel.
 *
 * @param streaming  true while the turn is still arriving (controls the cursor).
 */
function BlockList({
  blocks,
  streaming = false,
  autonomous = false,
}: {
  blocks: ContentBlock[];
  streaming?: boolean;
  /** When true, mutating tool calls render the "auto-approved" tag. */
  autonomous?: boolean;
}) {
  const displayBlocks = coalesceContentBlocks(blocks);

  // Index of the last text block, so the cursor attaches only there.
  let lastTextIndex = -1;
  if (streaming) {
    for (let i = displayBlocks.length - 1; i >= 0; i--) {
      if (displayBlocks[i].type === "text") {
        lastTextIndex = i;
        break;
      }
    }
  }

  return (
    <>
      {displayBlocks.map((block, i) => {
        if (block.type === "thinking") {
          return <ThinkingBlock key={i} thinking={block.thinking} />;
        }
        if (block.type === "tool_use") {
          // `AskUserQuestion` is surfaced as its own pinned question card, not
          // as a transcript activity row — skip it so neither the live stream
          // nor a reloaded transcript shows the redundant
          // `› AskUserQuestion · {questions json}` line.
          if (block.name === "AskUserQuestion") return null;
          return (
            <ToolUseBlock
              key={i}
              name={block.name}
              input={block.input}
              autoApproved={autonomous && wouldRequireApproval(block.name)}
            />
          );
        }
        const isTrailing = i === lastTextIndex;
        return (
          <div key={i} className="text-sm leading-relaxed break-words">
            <Markdown>{sanitise(block.text)}</Markdown>
            {streaming && isTrailing && (
              <span className="inline-block w-1.5 h-4 ml-0.5 bg-primary animate-pulse align-middle rounded-sm" />
            )}
          </div>
        );
      })}
    </>
  );
}

/**
 * Live-updating streaming bubble — shows tokens as they arrive via the Tauri
 * Channel.
 *
 * Unlike `TurnBubble` (canonical persisted transcript), this component
 * accumulates `TextDelta` and `ThinkingDelta` events in real time so the user
 * sees the assistant's response as it is generated.  Once the terminal `Result`
 * event arrives, the bubble shows usage/duration meta and the spinner avatar
 * switches to a Bot icon.  The next transcript reload replaces this component
 * with a persisted `TurnBubble`.
 */
function StreamingBubble({
  blocks,
  result,
  autonomous = false,
}: {
  blocks: ContentBlock[];
  result: (ClaudeEvent & { type: "result" }) | null;
  /** When true, mutating tool calls render the "auto-approved" tag. */
  autonomous?: boolean;
}) {
  const isComplete = result !== null;
  const isError = result?.is_error ?? false;
  // Overload errors render amber + an "overloaded" tag here too, so there's no
  // red flash before the transcript reload swaps this for the canonical
  // TurnBubble (which carries the full friendly banner + Retry button).
  const isOverload = isError && isOverloadError(result?.result);
  const hasContent = blocks.length > 0;

  return (
    <div className="flex gap-2.5">
      {/* Avatar — spinner while streaming, Bot once the Result arrives. */}
      <div className="size-7 shrink-0 rounded-full grid place-items-center bg-[color-mix(in_oklab,var(--primary)_18%,transparent)] text-[var(--primary)]">
        {isComplete ? (
          <Bot className="size-3.5" />
        ) : (
          <Loader2 className="size-3.5 animate-spin" />
        )}
      </div>

      {/* Bubble */}
      <div
        className={`min-w-0 max-w-[85%] rounded-lg px-3.5 py-2.5 ${
          isOverload
            ? "bg-amber-500/5 border border-amber-500/30 text-foreground"
            : isError
              ? "bg-[color-mix(in_oklab,var(--destructive)_10%,transparent)] border border-[color-mix(in_oklab,var(--destructive)_28%,transparent)] text-foreground"
              : "bg-card border border-border text-foreground"
        }`}
      >
        {/* Meta row — usage / duration appear once the Result arrives. */}
        {isComplete && (
          <div className="flex items-center gap-2 mb-1 flex-wrap">
            {isError && (
              <span
                className={`inline-flex items-center gap-1 text-[10px] font-semibold uppercase tracking-wider ${
                  isOverload ? "text-amber-600 dark:text-amber-400" : "text-destructive"
                }`}
              >
                {isOverload ? (
                  <>
                    <AlertTriangle size={10} /> overloaded
                  </>
                ) : (
                  <>
                    <AlertTriangle size={10} /> error
                  </>
                )}
              </span>
            )}
            {result.duration_ms > 0 && (
              <span className="text-[10px] text-muted-foreground">
                {fmtDuration(result.duration_ms)}
              </span>
            )}
            {result.usage && (
              <span className="text-[10px] text-muted-foreground">
                {result.usage.input_tokens.toLocaleString()}&rarr;
                {result.usage.output_tokens.toLocaleString()} tok
                {result.usage.total_cost_usd > 0 &&
                  ` · $${result.usage.total_cost_usd.toFixed(4)} total`}
              </span>
            )}
          </div>
        )}

        {/* Ordered content blocks — rendered exactly as they arrived. */}
        {hasContent ? (
          <BlockList blocks={blocks} streaming={!isComplete} autonomous={autonomous} />
        ) : (
          <p className="text-sm text-muted-foreground italic">
            {isComplete ? "(empty response)" : "Waiting for response…"}
          </p>
        )}

        {/* Busy indicator — shown until the Result arrives. */}
        {!isComplete && (
          <div className="flex items-center gap-2 mt-2 text-[10px] text-muted-foreground">
            <Loader2 className="size-3 animate-spin" />
            <span>Agent is working&hellip;</span>
          </div>
        )}
      </div>
    </div>
  );
}

/**
 * Render a manual tool-approval prompt as an interactive Allow / Deny card.
 *
 * When the agent calls a mutating/executing tool (Bash, Edit, Write, …) under
 * the manual-approval policy, the read loop parks until the user decides. This
 * card is the focal affordance while parked: it shows what the agent wants to
 * do (tool name + a one-line summary of its input) and offers an Allow and a
 * Deny button. While visible the composer is disabled — the user must resolve
 * the prompt (or the turn ends) before sending more input.
 *
 * Optional reason text is forwarded on a deny (surfaced to the model as the
 * denial message); allows ignore it. Mirrors `AskUserQuestionCard`'s visual
 * language so the two parking states read consistently.
 */
export function PermissionApprovalCard({
  toolName,
  input,
  disabled,
  onDecide,
  onAlwaysAllow,
}: {
  toolName: string;
  input: string;
  disabled?: boolean;
  onDecide: (decision: ApprovalDecision) => void;
  /**
   * "Always allow": allow this call AND persist a rule so future calls of the
   * same tool/command auto-allow. The parent resolves the current approval
   * (same as Allow) and writes the rule to `.claude/settings.local.json`.
   * Optional — when omitted, the Always-allow button is hidden.
   */
  onAlwaysAllow?: () => void;
}) {
  const [reason, setReason] = useState("");
  const summary = describeTool(toolName, input);

  return (
    <div className="my-2 rounded-lg border border-primary/30 bg-[color-mix(in_oklab,var(--primary)_5%,transparent)] p-3 space-y-3">
      <div className="flex items-center gap-2 text-xs font-medium text-primary">
        <span className="inline-block size-1.5 rounded-full bg-primary animate-pulse" />
        <span>The agent needs your approval</span>
      </div>

      <div className="rounded-md border border-border bg-input/60 px-3 py-2">
        <div className="text-[10px] font-semibold uppercase tracking-wide text-muted-foreground mb-0.5">
          {toolName}
        </div>
        <div className="font-mono text-xs text-foreground/90 break-all leading-relaxed">
          {sanitise(summary)}
        </div>
      </div>

      <details className="text-xs text-muted-foreground">
        <summary className="cursor-pointer hover:text-foreground transition-colors select-none">
          Add a reason (optional, deny only)
        </summary>
        <textarea
          value={reason}
          onChange={(e) => setReason(e.target.value)}
          disabled={disabled}
          rows={2}
          placeholder="Why deny? (shown to the agent)"
          className="mt-2 w-full resize-none rounded-md border border-border bg-input px-2 py-1.5 text-xs text-foreground placeholder:text-muted-foreground focus:outline-none focus:ring-1 focus:ring-primary disabled:opacity-50"
        />
      </details>

      <div className="flex justify-end gap-2 pt-1">
        <button
          type="button"
          disabled={disabled}
          onClick={() =>
            onDecide({
              allow: false,
              reason: reason.trim() || undefined,
            })
          }
          className="inline-flex items-center gap-1.5 h-8 px-3 rounded-md border border-[color-mix(in_oklab,var(--destructive)_40%,transparent)] text-destructive text-xs font-medium hover:bg-[color-mix(in_oklab,var(--destructive)_10%,transparent)] transition disabled:opacity-50 disabled:cursor-not-allowed"
        >
          <ShieldX className="size-3.5" />
          Deny
        </button>
        <button
          type="button"
          disabled={disabled}
          onClick={() => onDecide({ allow: true })}
          className="inline-flex items-center gap-1.5 h-8 px-3 rounded-md border border-primary/40 text-primary text-xs font-medium hover:bg-primary/10 transition disabled:opacity-50 disabled:cursor-not-allowed"
        >
          <ShieldCheck className="size-3.5" />
          Allow once
        </button>
        {onAlwaysAllow && (
          <button
            type="button"
            disabled={disabled}
            onClick={onAlwaysAllow}
            title="Allow now + remember this tool/command for future sessions (writes a rule to .claude/settings.local.json, takes effect on the next agent session)"
            className="inline-flex items-center gap-1.5 h-8 px-4 rounded-md bg-primary text-primary-foreground text-xs font-medium hover:opacity-90 transition disabled:opacity-50 disabled:cursor-not-allowed"
          >
            <ShieldPlus className="size-3.5" />
            Always allow
          </button>
        )}
      </div>
    </div>
  );
}

/**
 * The plan-approval card — pinned above the composer when the agent has
 * finished planning (`ExitPlanMode`) and is waiting for the human to review
 * before it starts executing.
 *
 * Structurally mirrors `PermissionApprovalCard` (pending strip, optional
 * feedback-on-reject textarea, action row) but renders the plan as markdown
 * rather than a raw tool-input summary, and uses Approve/Reject language
 * instead of Allow/Deny to match the mental model (this is a go/no-go on a
 * multi-step plan, not a single tool call).
 */
export function PlanApprovalCard({
  plan,
  disabled,
  onDecide,
}: {
  plan: string;
  disabled?: boolean;
  onDecide: (decision: PlanApprovalDecision) => void;
}) {
  const [feedback, setFeedback] = useState("");

  return (
    <div className="my-2 rounded-lg border border-primary/30 bg-[color-mix(in_oklab,var(--primary)_5%,transparent)] p-3 space-y-3">
      <div className="flex items-center gap-2 text-xs font-medium text-primary">
        <span className="inline-block size-1.5 rounded-full bg-primary animate-pulse" />
        <span>The agent has a plan ready for review</span>
      </div>

      <div className="rounded-md border border-border bg-input/60 px-3 py-2 max-h-64 overflow-y-auto text-sm">
        <Markdown>{plan}</Markdown>
      </div>

      <details className="text-xs text-muted-foreground">
        <summary className="cursor-pointer hover:text-foreground transition-colors select-none">
          Add feedback (optional, reject only)
        </summary>
        <textarea
          value={feedback}
          onChange={(e) => setFeedback(e.target.value)}
          disabled={disabled}
          rows={2}
          placeholder="What should the agent change? (shown to the agent)"
          className="mt-2 w-full resize-none rounded-md border border-border bg-input px-2 py-1.5 text-xs text-foreground placeholder:text-muted-foreground focus:outline-none focus:ring-1 focus:ring-primary disabled:opacity-50"
        />
      </details>

      <div className="flex justify-end gap-2 pt-1">
        <button
          type="button"
          disabled={disabled}
          onClick={() =>
            onDecide({
              approve: false,
              feedback: feedback.trim() || undefined,
            })
          }
          className="inline-flex items-center gap-1.5 h-8 px-3 rounded-md border border-[color-mix(in_oklab,var(--destructive)_40%,transparent)] text-destructive text-xs font-medium hover:bg-[color-mix(in_oklab,var(--destructive)_10%,transparent)] transition disabled:opacity-50 disabled:cursor-not-allowed"
        >
          <XCircle className="size-3.5" />
          Reject
        </button>
        <button
          type="button"
          disabled={disabled}
          onClick={() => onDecide({ approve: true })}
          className="inline-flex items-center gap-1.5 h-8 px-4 rounded-md bg-primary text-primary-foreground text-xs font-medium hover:opacity-90 transition disabled:opacity-50 disabled:cursor-not-allowed"
        >
          <CheckCircle2 className="size-3.5" />
          Approve & execute
        </button>
      </div>
    </div>
  );
}

/**
 * Streaming-aware chat UI component.
 *
 * Renders a conversation transcript with:
 * - Completed turns as styled chat bubbles (user / assistant / error)
 * - A live-updating streaming bubble during active requests
 * - A collapsible thinking block for model reasoning
 * - Auto-scroll that sticks to the bottom
 * - Empty state when there are no turns and nothing is streaming
 * - Error banner at the top with an optional dismiss button
 * - Composer (textarea + send button) for follow-up messages
 *
 * This component is **presentational** — the parent manages the Tauri
 * `Channel<ClaudeEvent>` and passes down streaming state, leaving this
 * component to focus purely on rendering.  The composer fires `onSend` so
 * the parent can set up a new streaming Channel.
 */
export function Chat({
  projectPath,
  turns,
  streamingBlocks,
  streamingResult,
  pendingUserText,
  busy,
  autonomous = false,
  retrying,
  error,
  onSend,
  onClearError,
  disabled = false,
  focusNonce,
  pendingQuestion,
  onAnswerQuestion,
  readOnly = false,
  pendingPermission,
  onAnswerPermission,
  onAlwaysAllow,
  pendingPlan,
  onAnswerPlan,
  planMode,
  onTogglePlanMode,
}: ChatProps) {
  const [draft, setDraft] = useState("");
  const scrollRef = useRef<HTMLDivElement>(null);
  const inputRef = useRef<HTMLTextAreaElement>(null);
  const isStreaming = streamingBlocks !== null;

  // ── `@`-mention autocomplete ──
  // The caret index mirrors the textarea's selectionStart so the mention hook
  // can re-parse the active `@…` token as the user types/moves. Kept in state
  // (not just a ref) because the trigger-detection effect keys off it.
  const [caret, setCaret] = useState(0);
  const mention = useFileMention({
    projectPath,
    draft,
    caret,
    textareaRef: inputRef,
    setDraft,
    setCaret,
    // Disable while parked on an approval/question, busy, read-only, etc. —
    // mirrors the composer's own `composerDisabled` gate below.
    disabled:
      disabled || busy || readOnly || !!pendingQuestion || !!pendingPermission || !!pendingPlan,
  });

  // ── `/`-skill discovery ──
  // Same lifecycle pattern as the `@`-mention hook above (and shares the same
  // `draft`/`caret` state owners), but triggers on `/` and lists the project's
  // installed skills instead of files. The two are mutually exclusive at the
  // caret: a `/` span and an `@` span can't both be active at once.
  const skills = useSkillDiscovery({
    projectPath,
    draft,
    caret,
    setDraft,
    setCaret,
    disabled:
      disabled || busy || readOnly || !!pendingQuestion || !!pendingPermission || !!pendingPlan,
  });

  // Whether auto-scroll is armed — i.e. the user is parked at the bottom and
  // wants new content to scroll into view. Flips to false the moment the user
  // scrolls up (to read history or, importantly, to drag-select text). While
  // disarmed, per-token streaming scrolls are suppressed so the container
  // doesn't yank to the bottom and break an in-progress selection. Re-armed on
  // send (sending implies "show me the latest") and whenever the turn list is
  // replaced (initial load, conversation switch, post-turn reload).
  const stickRef = useRef(true);

  // Stick-to-bottom while streaming: scroll on every progress tick, but ONLY
  // if the user hasn't scrolled away. This is the per-token path, so gating it
  // on `stickRef` is what keeps a mid-stream drag-selection from being
  // annihilated by an auto-scroll on the next token.
  useEffect(() => {
    if (stickRef.current && scrollRef.current) {
      scrollRef.current.scrollTop = scrollRef.current.scrollHeight;
    }
  }, [streamingBlocks, streamingResult, busy]);

  // Force-to-bottom when the turn list changes — initial load, conversation
  // switch, and the canonical-record swap after a turn completes. These are
  // discrete navigational events where snapping to the freshest content is the
  // right call, and they re-arm stickiness so subsequent streaming tokens keep
  // the view pinned until the user scrolls away.
  useEffect(() => {
    stickRef.current = true;
    if (scrollRef.current) {
      scrollRef.current.scrollTop = scrollRef.current.scrollHeight;
    }
  }, [turns]);

  // Focus the composer when the parent bumps `focusNonce` (e.g. after the
  // "New conversation" button archives the transcript). Lets the user start
  // typing their first message immediately.
  useEffect(() => {
    if (focusNonce === undefined) return;
    if (inputRef.current) inputRef.current.focus();
  }, [focusNonce]);

  /** Handle the composer send action: validate, clear, fire callback. */
  function handleSend() {
    const text = draft.trim();
    if (!text || disabled || busy || readOnly) return;
    setDraft("");
    // Sending implies "show me what happens next" — re-arm stick-to-bottom so
    // the user's own bubble (rendered by the parent's optimistic insert) and
    // the upcoming assistant reply scroll into view, even if the user had
    // scrolled up to read history before hitting send.
    stickRef.current = true;
    if (scrollRef.current) {
      scrollRef.current.scrollTop = scrollRef.current.scrollHeight;
    }
    onSend(text);
  }

  // Whether any parking affordance is active — the composer is disabled
  // while the agent waits on the user (not free-form input).
  const hasPendingQuestion =
    pendingQuestion != null && pendingQuestion.length > 0;
  const hasPendingPermission = pendingPermission != null;
  const hasPendingPlan = pendingPlan != null;
  const composerDisabled =
    disabled ||
    busy ||
    readOnly ||
    hasPendingQuestion ||
    hasPendingPermission ||
    hasPendingPlan;

  const isEmpty = turns.length === 0 && !isStreaming && !busy;

  return (
    <div className="flex h-full min-h-0 flex-1 flex-col">
      {/* ── Error banner ── */}
      {error && (
        <div className="flex items-start gap-2 mb-3 p-3 rounded-lg bg-[color-mix(in_oklab,var(--destructive)_10%,transparent)] border border-[color-mix(in_oklab,var(--destructive)_28%,transparent)] text-destructive text-xs leading-relaxed shrink-0">
          <AlertTriangle className="size-4 shrink-0 mt-0.5" />
          <span className="break-words flex-1">{error}</span>
          {onClearError && (
            <button
              onClick={onClearError}
              className="shrink-0 text-[10px] font-medium opacity-60 hover:opacity-100 transition-opacity"
            >
              Dismiss
            </button>
          )}
        </div>
      )}

      {/* ── Transcript area ── */}
      <div
        ref={scrollRef}
        onScroll={(e) => {
          const el = e.currentTarget;
          // "Near the bottom" within a small threshold counts as pinned —
          // covers fractional/RTL rounding so normal streaming stays sticky.
          // Anything past the threshold disarms auto-scroll: the user is
          // reading history or selecting text, so don't fight them.
          const atBottom =
            el.scrollHeight - el.scrollTop - el.clientHeight < 24;
          stickRef.current = atBottom;
        }}
        className="flex-1 min-h-0 overflow-y-auto space-y-3 pr-1"
      >
        {/* Empty state */}
        {isEmpty && (
          <div className="flex flex-col items-center justify-center py-16 text-center">
            <Bot size={32} className="text-muted-foreground/30 mb-3" />
            {readOnly ? (
              <>
                <h3 className="text-sm font-semibold text-foreground mb-1.5">
                  This conversation is empty
                </h3>
                <p className="text-xs text-muted-foreground max-w-xs leading-relaxed">
                  Read-only view — pick another conversation from the history
                  list, or start a new one to send messages.
                </p>
              </>
            ) : (
              <>
                <h3 className="text-sm font-semibold text-foreground mb-1.5">
                  No conversation yet
                </h3>
                <p className="text-xs text-muted-foreground max-w-xs leading-relaxed">
                  Type a message below to start a conversation, or press{" "}
                  <strong>Start next loop</strong> to have the agent work the
                  next step from{" "}
                  <code className="font-mono text-[11px] bg-muted px-1 py-0.5 rounded">
                    .loopdeck/loops.md
                  </code>
                  .
                </p>
              </>
            )}
          </div>
        )}

        {/* Completed turns (persisted transcript).
            Auto-built loop prompts (source: "loop") are collapsed into compact
            LoopStepRow markers so consecutive "Start next loop" clicks don't
            pile up as identical user bubbles. Real user + assistant turns
            render as full bubbles. */}
        {groupLoopRuns(turns).map((item, i) =>
          item.kind === "loop-run" ? (
            <LoopStepRow key={i} subject={item.subject} count={item.count} />
          ) : (
            <TurnBubble
              key={i}
              turn={item.turn}
              autonomous={autonomous}
              onRetryOverload={
                // Re-send the most recent user prompt. Only meaningful for an
                // overload error bubble (the button is hidden otherwise), and
                // only when we have a send path + a prior user turn to recover.
                !disabled && onSend
                  ? () => {
                      const lastUser = [...turns]
                        .reverse()
                        .find((t) => t.role === "user" && (t.text ?? "").trim());
                      if (lastUser) onSend(lastUser.text.trim());
                    }
                  : undefined
              }
            />
          ),
        )}

        {/* Ephemeral user bubble for the in-flight turn. The backend writes the
            user turn to disk BEFORE streaming starts, so a mid-stream reload
            can pull it into `turns` while the turn is still running. To avoid
            showing two bubbles (the canonical one in `turns` + this one), we
            hide the ephemeral bubble as soon as `turns` already ends with the
            same user text. That gives exactly one bubble at all times:
            ephemeral in the brief pre-reload window, canonical thereafter. */}
        {pendingUserText &&
          !turns.some(
            (t) =>
              t.role === "user" &&
              (t.text ?? "").trim() === pendingUserText.trim(),
          ) && (
            <TurnBubble
              turn={{
                ts: new Date().toISOString(),
                role: "user",
                source: "user",
                text: pendingUserText,
              }}
            />
          )}

        {/* Streaming bubble — live token accumulation via Channel */}
        {isStreaming && (
          <StreamingBubble
            blocks={streamingBlocks ?? []}
            result={streamingResult}
            autonomous={autonomous}
          />
        )}

        {/* Busy indicator when awaiting the first token. */}
        {busy && !isStreaming && (
          <div className="flex items-center gap-2 text-xs text-muted-foreground pl-1">
            <Loader2 className="size-3.5 animate-spin" />
            <span>Agent is working&hellip;</span>
          </div>
        )}

        {/* Retry indicator during a transient gateway overload (e.g. 529).
            The backend emits a `retrying` event between a failed attempt and
            its backoff-delayed retry; without this row the agent looks frozen
            in that window (the failed attempt's Result already landed, the next
            hasn't started). Amber to read as "transient, recovering" rather
            than the red error banner. */}
        {retrying && (
          <div className="flex items-center gap-2 text-xs text-amber-600 dark:text-amber-400 pl-1">
            <Loader2 className="size-3.5 animate-spin" />
            <span>
              Gateway overloaded — retrying {retrying.attempt}/{retrying.maxAttempts}{" "}
              in {Math.round(retrying.backoffMs / 1000)}s&hellip;
            </span>
          </div>
        )}
      </div>

      {/* ── Pending-interaction strip ──
          Pinned OUT of the transcript scroll region, between the transcript
          and the composer. Both cards below represent a turn that is PARKED —
          the agent cannot proceed until the user answers — so they must be the
          most visible thing on screen, never buried below the fold. Keeping
          them in normal flow inside the scroller meant a long transcript
          pushed them out of view, forcing the user to scroll up to find the
          very thing blocking progress. This strip is non-scrolling
          (shrink-0, max-h with internal overflow only when a card is huge) so
          the card stays anchored at the bottom of the visible area, right
          above the composer where the user's attention already is. */}
      {(hasPendingQuestion || hasPendingPermission || hasPendingPlan) && (
        <div className="shrink-0 pt-2">
          {hasPendingQuestion && onAnswerQuestion && (
            <div className="max-h-[45vh] overflow-y-auto">
              <AskUserQuestionCard
                questions={pendingQuestion}
                disabled={disabled}
                onSubmit={onAnswerQuestion}
              />
            </div>
          )}
          {hasPendingPermission && pendingPermission && onAnswerPermission && (
            <PermissionApprovalCard
              toolName={pendingPermission.toolName}
              input={pendingPermission.input}
              disabled={disabled}
              onDecide={onAnswerPermission}
              onAlwaysAllow={
                onAlwaysAllow
                  ? () =>
                      onAlwaysAllow(
                        pendingPermission.toolName,
                        pendingPermission.input,
                      )
                  : undefined
              }
            />
          )}
          {hasPendingPlan && pendingPlan && onAnswerPlan && (
            <PlanApprovalCard
              plan={pendingPlan.plan}
              disabled={disabled}
              onDecide={onAnswerPlan}
            />
          )}
        </div>
      )}

      {/* ── Composer ── */}
      {/* Disabled while a question or approval is pending (the agent is parked
          waiting on the answer/verdict, not free-form input), or in read-only
          mode (viewing an archived conversation). */}
      <div className="pt-3 mt-3 border-t border-border shrink-0">
        {readOnly ? (
          <div className="text-[11px] text-muted-foreground italic text-center py-2">
            Read-only — this is a past conversation. Start a new one to send
            messages.
          </div>
        ) : (
          <div className="relative flex items-end gap-2">
            {/* `@`-mention popup — anchored to the textarea caret. Rendered as
                a child of this `relative` row so the caret-local coordinates
                from caretCoords (whose origin is the textarea's top-left, which
                coincides with this row's top-left) line up directly. The popup
                is absolute-positioned so it doesn't disturb the flex layout. */}
            <FileMentionMenu
              open={mention.open}
              items={mention.items}
              loading={mention.loading}
              selectedIndex={mention.selectedIndex}
              position={mention.position}
              onHover={mention.setSelectedIndex}
              onPick={mention.pick}
            />
            {/* `/`-skill discovery popup — same anchoring as the `@`-mention
                popup above. The two never open at the same caret at once. */}
            <SkillMenu
              open={skills.open}
              items={skills.items}
              loading={skills.loading}
              selectedIndex={skills.selectedIndex}
              onHover={skills.setSelectedIndex}
              onPick={skills.pick}
            />
            {onTogglePlanMode && (
              <button
                type="button"
                onClick={onTogglePlanMode}
                disabled={composerDisabled}
                title={
                  planMode
                    ? "Plan mode is ON — the next message runs read-only until you approve the agent's plan. Click to turn off."
                    : "Plan mode — have the agent propose a plan and wait for your approval before it edits anything."
                }
                className={`inline-flex items-center justify-center size-9 shrink-0 rounded-md border transition disabled:opacity-50 disabled:cursor-not-allowed ${
                  planMode
                    ? "bg-primary/15 border-primary/40 text-primary"
                    : "bg-muted border-border text-muted-foreground hover:bg-accent hover:text-foreground"
                }`}
              >
                <ListChecks className="size-4" />
              </button>
            )}
            <textarea
              ref={inputRef}
              value={draft}
              onChange={(e) => {
                setDraft(e.target.value);
                setCaret(e.target.selectionStart);
              }}
              onKeyUp={(e) => setCaret(e.currentTarget.selectionStart)}
              onClick={(e) => setCaret(e.currentTarget.selectionStart)}
              onSelect={(e) =>
                setCaret((e.target as HTMLTextAreaElement).selectionStart)
              }
              onKeyDown={(e) => {
                // Mention/skill nav consume the key first (arrows, Enter, Esc,
                // Tab). Only when neither does (returns false) do we fall
                // through to the normal Enter-to-send — so an open popup's
                // Enter inserts the selection instead of sending the message.
                if (mention.onKeyDown(e)) return;
                if (skills.onKeyDown(e)) return;
                if (e.key === "Enter" && !e.shiftKey) {
                  e.preventDefault();
                  handleSend();
                }
              }}
              placeholder={
                planMode
                  ? "Describe what you want planned… (the agent will propose a plan for your approval before editing anything)"
                  : turns.length === 0
                    ? "Start a new conversation… (Enter to send, Shift+Enter for newline, @ for files, / for skills)"
                    : "Send a follow-up message… (Enter to send, Shift+Enter for newline, @ for files, / for skills)"
              }
              rows={2}
              disabled={composerDisabled}
              onBlur={() => {
                mention.close();
                skills.close();
              }}
              className="flex-1 resize-none rounded-md border border-border bg-input px-3 py-2 text-sm text-foreground placeholder:text-muted-foreground focus:outline-none focus:ring-1 focus:ring-primary disabled:opacity-50"
            />
            <button
              onClick={handleSend}
              disabled={composerDisabled || !draft.trim()}
              className="inline-flex items-center justify-center size-9 shrink-0 rounded-md bg-primary text-primary-foreground hover:opacity-90 transition disabled:opacity-50 disabled:cursor-not-allowed"
              title="Send message"
            >
              {busy ? (
                <Loader2 className="size-4 animate-spin" />
              ) : (
                <Send className="size-4" />
              )}
            </button>
          </div>
        )}
      </div>
    </div>
  );
}
