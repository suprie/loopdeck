import { useState, useEffect, useRef, useCallback } from "react";
import { Channel } from "@tauri-apps/api/core";
import { Play, RotateCcw, Loader2 } from "lucide-react";
import type { ConversationTurn, ClaudeEvent, AppError, ToolCall } from "../../types";
import * as api from "../../lib/tauri";
import { useAppStore } from "../../store/appStore";
import { Chat } from "./Chat";

interface AgentPanelProps {
  projectPath: string;
}

// ── Helpers ──────────────────────────────────────────────────────────────────

/**
 * Coerce any rejection into a human-readable string.
 *
 * Tauri command rejections arrive in several shapes depending on where they
 * originate — this normalizes all of them so the error banner never shows
 * blank/"undefined":
 * - Rust `AppError` → `{ message, kind }` (our serialized form)
 * - plain `Error` (JS throw) → `.message`
 * - string → as-is
 * - anything else → `String(err)` fallback
 */
function describeError(err: unknown): string {
  if (err == null) return "Unknown error";
  if (typeof err === "string") return err;
  if (err instanceof Error) return err.message;
  const maybe = err as Partial<AppError> & { message?: unknown };
  if (typeof maybe.message === "string" && maybe.message.length > 0) {
    return maybe.message;
  }
  return String(err);
}

// ── Component ────────────────────────────────────────────────────────────────

export function AgentPanel({ projectPath }: AgentPanelProps) {
  // ── Transcript ──
  const [turns, setTurns] = useState<ConversationTurn[]>([]);
  const [loading, setLoading] = useState(true);

  // ── Streaming state ──
  const [busy, setBusy] = useState(false);
  const [streamingText, setStreamingText] = useState<string | null>(null);
  const [streamingThinking, setStreamingThinking] = useState<string | null>(null);
  const [streamingTools, setStreamingTools] = useState<ToolCall[] | null>(null);
  const [streamingResult, setStreamingResult] =
    useState<(ClaudeEvent & { type: "result" }) | null>(null);

  // ── Error ──
  const [error, setError] = useState<string | null>(null);

  // ── pendingAgentStart — auto-fire Start when landed from dashboard CTA ──
  const pendingAgentStart = useAppStore((s) => s.pendingAgentStart);
  const setPendingAgentStart = useAppStore((s) => s.setPendingAgentStart);

  // Track whether the component is mounted so channel events arriving after
  // unmount (user navigated away mid-turn) don't set state.
  const mountedRef = useRef(true);
  useEffect(() => {
    mountedRef.current = true;
    return () => {
      mountedRef.current = false;
    };
  }, []);

  // ── Transcript persistence ───────────────────────────────────────────────

  /** Load (or reload) the full transcript from disk. */
  const reload = useCallback(async () => {
    try {
      const data = await api.agentGetConversation(projectPath);
      if (mountedRef.current) {
        setTurns(data);
        setError(null);
      }
    } catch (err) {
      if (mountedRef.current) {
        setError(describeError(err));
      }
    }
  }, [projectPath]);

  // Load transcript on mount / project change.
  useEffect(() => {
    let cancelled = false;
    async function load() {
      setLoading(true);
      try {
        const data = await api.agentGetConversation(projectPath);
        if (!cancelled) {
          setTurns(data);
          setError(null);
        }
      } catch (err) {
        if (!cancelled) {
          setError(describeError(err));
        }
      } finally {
        if (!cancelled) setLoading(false);
      }
    }
    load();
    return () => {
      cancelled = true;
    };
  }, [projectPath]);

  // ── Auto-fire Start from dashboard CTA ──────────────────────────────────

  useEffect(() => {
    if (pendingAgentStart && pendingAgentStart === projectPath) {
      setPendingAgentStart(null);
      void runStartLoop();
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [pendingAgentStart, projectPath]);

  // ── Streaming orchestration ──────────────────────────────────────────────

  /**
   * Shared streaming turn runner.
   *
   * Creates a Tauri `Channel<ClaudeEvent>`, wires `onmessage` to accumulate
   * text/thinking deltas in real time, calls the appropriate streaming IPC,
   * and finalizes (reload transcript, clear streaming state) when the terminal
   * `Result` event arrives or the invoke Promise rejects.
   *
   * @param prompt  If `undefined`, starts the next loop (prompt built
   *                server-side from `.loopdeck/loops.md`).  If a string,
   *                sends it as a free-form follow-up.
   */
  async function runStreamingTurn(prompt?: string) {
    setBusy(true);
    setError(null);
    setStreamingText("");
    setStreamingThinking("");
    setStreamingTools([]);
    setStreamingResult(null);

    const channel = new Channel<ClaudeEvent>();

    // Guards against double-reload if the invoke Promise resolves before the
    // last channel event is delivered (shouldn't happen with Tauri v2's ordered
    // IPC, but defensive coding is cheap).
    let resultHandled = false;

    channel.onmessage = (event: ClaudeEvent) => {
      if (!mountedRef.current) return;

      switch (event.type) {
        case "text_delta":
          setStreamingText((prev) => (prev ?? "") + event.text);
          break;
        case "thinking_delta":
          setStreamingThinking((prev) => (prev ?? "") + event.thinking);
          break;
        case "tool_use":
          // Accumulate tool calls so the UI shows live activity during long
          // agentic turns (reading files, editing, running commands) where
          // text deltas are sparse.
          setStreamingTools((prev) => [
            ...(prev ?? []),
            { name: event.name, input: event.input },
          ]);
          break;
        case "result":
          resultHandled = true;
          setStreamingResult(event);
          setStreamingText(null); // signal Chat to remove the streaming bubble
          setStreamingThinking(null);
          setStreamingTools(null);
          setBusy(false);

          // Surface model-level errors (e.g. "Not logged in") in the banner.
          if (event.is_error) {
            setError(event.result || "The agent turn ended in an error.");
          }

          // Reload transcript so the persisted turn replaces the streaming
          // bubble with the canonical record.
          void reload();
          break;
      }
    };

    try {
      if (prompt !== undefined) {
        await api.agentSendMessageStreaming(projectPath, prompt, channel);
      } else {
        await api.agentStartLoopStreaming(projectPath, channel);
      }

      // Fallback: if the Result event hasn't fired yet (unlikely), reload.
      if (!resultHandled && mountedRef.current) {
        setStreamingText(null);
        setStreamingThinking(null);
        setStreamingTools(null);
        setStreamingResult(null);
        setBusy(false);
        void reload();
      }
    } catch (err) {
      // Infra-level error: timeout, no agent config, spawn failure, etc.
      if (mountedRef.current) {
        setError(describeError(err));
        setStreamingText(null);
        setStreamingThinking(null);
        setStreamingTools(null);
        setStreamingResult(null);
        setBusy(false);
        // Best-effort reload — a failed turn may still have been partially
        // recorded (user turn appended before send).
        void reload();
      }
    }
  }

  /** Start the next development loop (streaming). */
  async function runStartLoop() {
    await runStreamingTurn(undefined);
  }

  /** Send a free-form follow-up message (streaming). */
  async function runSendMessage(prompt: string) {
    if (!prompt.trim() || busy) return;
    await runStreamingTurn(prompt.trim());
  }

  /** Reset: drop the live process and archive the transcript. */
  async function runReset() {
    setBusy(true);
    setError(null);
    try {
      await api.agentResetSession(projectPath);
      await reload();
    } catch (err) {
      setError(describeError(err));
    } finally {
      if (mountedRef.current) setBusy(false);
    }
  }

  // ── Loading state ───────────────────────────────────────────────────────

  if (loading) {
    return (
      <div className="flex flex-col items-center justify-center py-12 gap-4 text-muted-foreground">
        <Loader2 className="size-8 animate-spin" />
        <span className="text-sm">Loading conversation...</span>
      </div>
    );
  }

  // ── Render ──────────────────────────────────────────────────────────────

  return (
    <div className="max-w-3xl flex flex-col h-full">
      {/* ── Toolbar ── */}
      <div className="flex items-center gap-2 pb-3 mb-3 border-b border-border shrink-0">
        <button
          onClick={runStartLoop}
          disabled={busy}
          className="inline-flex items-center gap-1.5 h-8 px-3 rounded-md bg-primary text-primary-foreground text-xs font-medium hover:opacity-90 transition disabled:opacity-50 disabled:cursor-not-allowed"
          title="Start / continue the next development loop"
        >
          {busy ? (
            <Loader2 className="size-3.5 animate-spin" />
          ) : (
            <Play className="size-3.5 fill-current" />
          )}
          Start next loop
        </button>
        <button
          onClick={runReset}
          disabled={busy || turns.length === 0}
          className="inline-flex items-center gap-1.5 h-8 px-3 rounded-md bg-muted text-muted-foreground text-xs font-medium hover:bg-accent hover:text-foreground transition disabled:opacity-50 disabled:cursor-not-allowed"
          title="Drop the live process and archive the transcript (next Start is fresh)"
        >
          <RotateCcw className="size-3.5" />
          New conversation
        </button>
      </div>

      {/* ── Chat (transcript + streaming bubble + composer) ── */}
      <div className="flex-1 min-h-0">
        <Chat
          turns={turns}
          streamingText={streamingText}
          streamingThinking={streamingThinking}
          streamingTools={streamingTools}
          streamingResult={streamingResult}
          busy={busy}
          error={error}
          onSend={runSendMessage}
          onClearError={() => setError(null)}
        />
      </div>
    </div>
  );
}
