import { useRef, useEffect, useState } from "react";
import {
  Bot,
  User,
  AlertTriangle,
  Loader2,
  Send,
  Brain,
  ChevronDown,
  ChevronRight,
} from "lucide-react";
import type { ConversationTurn, ClaudeEvent, ToolCall } from "../../types";

// ── Types ────────────────────────────────────────────────────────────────────

export interface ChatProps {
  /** Completed conversation turns to display. */
  turns: ConversationTurn[];
  /** Currently accumulating streaming text (null when nothing is streaming). */
  streamingText: string | null;
  /** Currently accumulating streaming thinking (null when nothing is streaming). */
  streamingThinking: string | null;
  /** Tool calls accumulated so far this turn (null when nothing is streaming). */
  streamingTools: ToolCall[] | null;
  /**
   * The terminal Result event, if it has arrived while the streaming bubble is
   * still visible (arrives before the transcript reload replaces it). Carries
   * usage, duration, and is_error for the meta row. */
  streamingResult: (ClaudeEvent & { type: "result" }) | null;
  /** Whether a request is currently in flight (Start or Send). */
  busy: boolean;
  /** Error message to show as a banner above the transcript. */
  error: string | null;
  /** Called when the user sends a message from the composer. */
  onSend: (text: string) => void;
  /** Called when the user clears the error banner. */
  onClearError?: () => void;
  /** Whether the composer and Start button should be disabled. */
  disabled?: boolean;
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
  const candidate = input.file_path ?? input.path ?? input.command ?? input.pattern ?? input.query ?? input.url;
  const detail = candidate ? str(candidate) : (rawInput && rawInput !== "{}" ? rawInput : "");

  return detail ? `${name} · ${detail}` : name;
}

// ── Sub-components ───────────────────────────────────────────────────────────

/**
 * A single completed conversation turn from the persisted transcript.
 *
 * Renders user turns as right-aligned primary bubbles and assistant turns as
 * left-aligned card bubbles with optional error flag, duration, and token usage
 * meta.  This component is used for turns loaded from disk — it does not stream.
 */
function TurnBubble({ turn }: { turn: ConversationTurn }) {
  const isUser = turn.role === "user";
  const isError = turn.is_error ?? false;

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
            : isError
              ? "bg-[color-mix(in_oklab,var(--destructive)_10%,transparent)] border border-[color-mix(in_oklab,var(--destructive)_28%,transparent)] text-foreground"
              : "bg-card border border-border text-foreground"
        }`}
      >
        {/* Meta row for assistant turns: error flag / duration / usage. */}
        {!isUser && (
          <div className="flex items-center gap-2 mb-1 flex-wrap">
            {isError && (
              <span className="inline-flex items-center gap-1 text-[10px] font-semibold uppercase tracking-wider text-destructive">
                <AlertTriangle size={10} /> error
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

        <p className="text-sm leading-relaxed whitespace-pre-wrap break-words">
          {sanitise(turn.text)}
        </p>
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
        <span>
          {open ? "Hide thinking" : "Show thinking"}
        </span>
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
  if (tools.length === 0) return null;
  return (
    <ul className="mb-2 space-y-1">
      {tools.map((tool, i) => (
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
  text,
  thinking,
  tools,
  result,
}: {
  text: string;
  thinking: string;
  tools: ToolCall[];
  result: (ClaudeEvent & { type: "result" }) | null;
}) {
  const isComplete = result !== null;
  const isError = result?.is_error ?? false;

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
          isError
            ? "bg-[color-mix(in_oklab,var(--destructive)_10%,transparent)] border border-[color-mix(in_oklab,var(--destructive)_28%,transparent)] text-foreground"
            : "bg-card border border-border text-foreground"
        }`}
      >
        {/* Meta row — usage / duration appear once the Result arrives. */}
        {isComplete && (
          <div className="flex items-center gap-2 mb-1 flex-wrap">
            {isError && (
              <span className="inline-flex items-center gap-1 text-[10px] font-semibold uppercase tracking-wider text-destructive">
                <AlertTriangle size={10} /> error
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

        {/* Collapsible thinking block */}
        <ThinkingBlock thinking={thinking} />

        {/* Live tool calls — concrete activity while the agent works. */}
        {!isComplete && <ToolList tools={tools} />}

        {/* Streaming text body */}
        {text.length > 0 ? (
          <p className="text-sm leading-relaxed whitespace-pre-wrap break-words">
            {sanitise(text)}
            {/* Blinking cursor while still streaming — typewriter feel. */}
            {!isComplete && (
              <span className="inline-block w-1.5 h-4 ml-0.5 bg-primary animate-pulse align-middle rounded-sm" />
            )}
          </p>
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

// ── Main component ───────────────────────────────────────────────────────────

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
  turns,
  streamingText,
  streamingThinking,
  streamingTools,
  streamingResult,
  busy,
  error,
  onSend,
  onClearError,
  disabled = false,
}: ChatProps) {
  const [draft, setDraft] = useState("");
  const scrollRef = useRef<HTMLDivElement>(null);
  const isStreaming = streamingText !== null;

  // Auto-scroll to the bottom when turns change, streaming progresses, or
  // the busy flag toggles (covers the gap before the first token arrives).
  useEffect(() => {
    if (scrollRef.current) {
      scrollRef.current.scrollTop = scrollRef.current.scrollHeight;
    }
  }, [turns, streamingText, streamingThinking, streamingTools, busy]);

  /** Handle the composer send action: validate, clear, fire callback. */
  function handleSend() {
    const text = draft.trim();
    if (!text || disabled || busy) return;
    setDraft("");
    onSend(text);
  }

  const isEmpty = turns.length === 0 && !isStreaming && !busy;

  return (
    <div className="flex flex-col h-full min-h-0">
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
        className="flex-1 min-h-0 overflow-y-auto space-y-3 pr-1"
      >
        {/* Empty state */}
        {isEmpty && (
          <div className="flex flex-col items-center justify-center py-16 text-center">
            <Bot size={32} className="text-muted-foreground/30 mb-3" />
            <h3 className="text-sm font-semibold text-foreground mb-1.5">
              No conversation yet
            </h3>
            <p className="text-xs text-muted-foreground max-w-xs leading-relaxed">
              Press <strong>Start next loop</strong> to spawn the agent. It will
              read{" "}
              <code className="font-mono text-[11px] bg-muted px-1 py-0.5 rounded">
                .loopdeck/loops.md
              </code>
              , work the next unchecked step, and update the memory files.
            </p>
          </div>
        )}

        {/* Completed turns (persisted transcript) */}
        {turns.map((turn, i) => (
          <TurnBubble key={i} turn={turn} />
        ))}

        {/* Streaming bubble — live token accumulation via Channel */}
        {isStreaming && (
          <StreamingBubble
            text={streamingText ?? ""}
            thinking={streamingThinking ?? ""}
            tools={streamingTools ?? []}
            result={streamingResult}
          />
        )}

        {/* Busy indicator when awaiting the first token. */}
        {busy && !isStreaming && (
          <div className="flex items-center gap-2 text-xs text-muted-foreground pl-1">
            <Loader2 className="size-3.5 animate-spin" />
            <span>Agent is working&hellip;</span>
          </div>
        )}
      </div>

      {/* ── Composer ── */}
      <div className="pt-3 mt-3 border-t border-border shrink-0">
        <div className="flex items-end gap-2">
          <textarea
            value={draft}
            onChange={(e) => setDraft(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === "Enter" && !e.shiftKey) {
                e.preventDefault();
                handleSend();
              }
            }}
            placeholder="Send a follow-up message… (Enter to send, Shift+Enter for newline)"
            rows={2}
            disabled={disabled || busy}
            className="flex-1 resize-none rounded-md border border-border bg-input px-3 py-2 text-sm text-foreground placeholder:text-muted-foreground focus:outline-none focus:ring-1 focus:ring-primary disabled:opacity-50"
          />
          <button
            onClick={handleSend}
            disabled={disabled || busy || !draft.trim()}
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
      </div>
    </div>
  );
}
