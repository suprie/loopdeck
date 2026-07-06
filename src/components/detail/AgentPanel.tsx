import { useState, useEffect, useRef, useCallback } from "react";
import { Channel } from "@tauri-apps/api/core";
import { Play, RotateCcw, Loader2, History, ChevronDown, Square } from "lucide-react";
import type {
  ConversationTurn,
  ClaudeEvent,
  AppError,
  ContentBlock,
  AskUserQuestionAnswers,
  ConversationSummary,
  ApprovalDecision,
} from "../../types";
import * as api from "../../lib/tauri";
import { useAppStore } from "../../store/appStore";
import { usePendingInteractions } from "../../store/pendingInteractions";
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

/**
 * Format an ISO timestamp as a short relative-ish string for the history rows.
 *
 * Falls back to a trimmed absolute date when the timestamp is missing or
 * unparseable, so the row never shows a blank or `Invalid Date`.
 */
function fmtWhen(iso: string): string {
  if (!iso) return "—";
  const then = Date.parse(iso);
  if (Number.isNaN(then)) return iso.replace("T", " ").replace(/Z$/, "");
  const diffMs = Date.now() - then;
  const mins = Math.floor(diffMs / 60_000);
  if (mins < 1) return "just now";
  if (mins < 60) return `${mins}m ago`;
  const hrs = Math.floor(mins / 60);
  if (hrs < 24) return `${hrs}h ago`;
  const days = Math.floor(hrs / 24);
  if (days < 7) return `${days}d ago`;
  // Beyond a week, show the absolute date — relative gets meaningless.
  return new Date(then).toLocaleDateString(undefined, {
    month: "short",
    day: "numeric",
  });
}

// ── Component ────────────────────────────────────────────────────────────────

export function AgentPanel({ projectPath }: AgentPanelProps) {
  // ── Transcript ──
  const [turns, setTurns] = useState<ConversationTurn[]>([]);
  const [loading, setLoading] = useState(true);

  // ── Conversation history ──
  // `selectedId` is the conversation currently in view (`"active"` or an
  // archive stem). The active one is live (streaming + composer); archives are
  // read-only. Kept in sync with the on-disk list via `refreshConversations`.
  const [conversations, setConversations] = useState<ConversationSummary[]>([]);
  const [selectedId, setSelectedId] = useState<string>("active");
  const [historyOpen, setHistoryOpen] = useState(false);

  // ── Streaming state ──
  // `streamingBlocks` is a single ordered accumulator — each text/thinking
  // delta appends to the tail (coalescing consecutive same-type deltas) and
  // each tool_use pushes a new block. Preserving arrival order across block
  // types is the whole point: the bubble renders exactly how the turn unfolded
  // (e.g. thinking → text → tool_use → thinking → text) rather than a fixed
  // grouping. `null` means "no turn streaming" and hides the streaming bubble.
  const [busy, setBusy] = useState(false);
  const [streamingBlocks, setStreamingBlocks] = useState<ContentBlock[] | null>(null);
  const [streamingResult, setStreamingResult] =
    useState<(ClaudeEvent & { type: "result" }) | null>(null);

  // ── Error ──
  const [error, setError] = useState<string | null>(null);

  // ── Composer focus nonce — bumped to ask Chat to focus its composer. ──
  // Used by "New conversation": after archiving the transcript we want the
  // user to be able to start typing their first message immediately.
  const [focusNonce, setFocusNonce] = useState(0);

  // ── Pending AskUserQuestion + manual approval ──
  // These live in a navigation-stable Zustand store (keyed by project path),
  // NOT in component state: navigating away from the Agent tab unmounts this
  // component, and the backend stays parked on the answer — so the card must
  // reappear on remount. The store is the single source of truth for the
  // display payload; the backend's per-project oneshot slots are the source of
  // truth for "is anything actually waiting" (see `agent_answer_*` commands).
  const pendingQuestion = usePendingInteractions((s) => s.questions[projectPath] ?? null);
  const pendingPermission = usePendingInteractions((s) => s.permissions[projectPath] ?? null);
  const setPendingQuestion = usePendingInteractions((s) => s.setQuestion);
  const clearPendingQuestion = usePendingInteractions((s) => s.clearQuestion);
  const setPendingPermission = usePendingInteractions((s) => s.setPermission);
  const clearPendingPermission = usePendingInteractions((s) => s.clearPermission);

  // ── pendingAgentStart — auto-fire Start when landed from dashboard CTA ──
  const pendingAgentStart = useAppStore((s) => s.pendingAgentStart);
  const setPendingAgentStart = useAppStore((s) => s.setPendingAgentStart);

  // Synchronous busy flag — the source of truth for "is a turn in flight".
  // React state (`busy`) is stale within the render cycle, so a second rapid
  // send (double-click, double Enter) would see `busy === false` and start a
  // second concurrent turn — both writing into the single `streamingBlocks`
  // accumulator, piling their text into one garbled bubble. The ref flips
  // synchronously, closing that race. Stays true through end-of-turn cleanup
  // (the reload swap) so a new turn can't preempt the persisted write.
  const busyRef = useRef(false);

  // Track whether the component is mounted so channel events arriving after
  // unmount (user navigated away mid-turn) don't set state.
  const mountedRef = useRef(true);
  useEffect(() => {
    mountedRef.current = true;
    return () => {
      mountedRef.current = false;
    };
  }, []);

  // Ref version of `selectedId` so the streaming channel callback (created once
  // per turn) can read the current selection without being a stale closure.
  // This matters because the callback decides whether streaming blocks should
  // render — only the active conversation is live.
  const selectedIdRef = useRef(selectedId);
  useEffect(() => {
    selectedIdRef.current = selectedId;
  }, [selectedId]);

  // Whether the view is the live conversation. Archives are still continuable
  // — sending a follow-up promotes them to active on demand (see
  // `runSendMessage`) — but while *viewing* an archive we hide any stale
  // streaming bubble from a prior active turn so the read is clean.
  const isActiveView = selectedId === "active";

  // ── Conversation loading ────────────────────────────────────────────────

  /** Load a specific conversation's turns by id into state. */
  const loadConversation = useCallback(
    async (id: string) => {
      try {
        const data = await api.agentGetConversationById(projectPath, id);
        if (mountedRef.current) {
          setTurns(data);
          setError(null);
        }
      } catch (err) {
        if (mountedRef.current) {
          setError(describeError(err));
        }
      }
    },
    [projectPath],
  );

  /** Refresh the on-disk history list (active + archives). */
  const refreshConversations = useCallback(async () => {
    try {
      const list = await api.agentListConversations(projectPath);
      if (mountedRef.current) setConversations(list);
    } catch {
      // Non-fatal — the list is a convenience; the panel still works without it.
    }
  }, [projectPath]);

  /**
   * Reload the active conversation's turns + the history list.
   *
   * Called after a turn completes (the canonical record replaces the streaming
   * bubble) and after reset. Always targets `"active"` regardless of the
   * currently selected view — a completed/ reset turn only affects the live
   * transcript, so that's what we refresh.
   */
  const reload = useCallback(async () => {
    await Promise.all([loadConversation("active"), refreshConversations()]);
  }, [loadConversation, refreshConversations]);

  // Load transcript + history on mount / project change.
  useEffect(() => {
    let cancelled = false;
    let pollTimer: ReturnType<typeof setTimeout> | null = null;
    async function load() {
      setLoading(true);
      // Default to the active conversation on fresh mount of this project.
      setSelectedId("active");
      // Reconcile `busy` with any pending interaction in the navigation-stable
      // store: if a prior mount left the agent parked on an approval/question,
      // the backend turn is still in flight, so we must enter the busy state to
      // block a new Start from racing it. Without this, returning to the Agent
      // tab after navigating away mid-approval would show the card but enable
      // Start (which the backend would reject with "agent is busy" anyway —
      // this just makes the UI honest about it up front).
      const pending = usePendingInteractions.getState();
      const hasPendingParking =
        !!pending.permissions[projectPath] || !!pending.questions[projectPath];
      if (hasPendingParking) {
        setBusy(true);
        busyRef.current = true;
      }
      try {
        const [turnData, convList] = await Promise.all([
          api.agentGetConversationById(projectPath, "active"),
          api.agentListConversations(projectPath),
        ]);
        if (!cancelled) {
          setTurns(turnData);
          setConversations(convList);
          setError(null);
        }
      } catch (err) {
        if (!cancelled) {
          setError(describeError(err));
        }
      } finally {
        if (!cancelled) setLoading(false);
      }

      // Reconcile a turn that's in flight but NOT parked on the user. This
      // happens when the previous mount unmounted mid-streaming: the Tauri
      // Channel it subscribed to is gone, so streaming events from the
      // still-running backend can no longer reach the UI. We ask the backend
      // whether a turn is in flight; if so, show an honest "Agent is working…"
      // state and poll the transcript until the persisted turn lands or the
      // backend reports idle. We snapshot the turn count to detect landing
      // (the assistant turn only appears in `active.jsonl` once the backend
      // finishes writing it).
      if (cancelled || hasPendingParking) return;
      let busy = false;
      try {
        busy = await api.agentIsBusy(projectPath);
      } catch {
        // If the probe fails, assume idle — don't block the UI.
      }
      if (!busy || cancelled) return;

      // Snapshot the turn count we just loaded; the assistant turn lands as a
      // new entry once the backend finishes writing active.jsonl. We use this
      // to detect "the in-flight turn just landed" as the polling stop signal.
      let lastTurnCount = 0;
      try {
        lastTurnCount = (
          await api.agentGetConversationById(projectPath, "active")
        ).length;
      } catch {
        // ignore — we'll fall back to time-based polling
      }

      if (!mountedRef.current || cancelled) return;
      setBusy(true);
      busyRef.current = true;

      const poll = async () => {
        if (cancelled || !mountedRef.current) return;
        try {
          const [stillBusy, freshTurns] = await Promise.all([
            api.agentIsBusy(projectPath).catch(() => false),
            api.agentGetConversationById(projectPath, "active").catch(() => null),
          ]);
          if (cancelled || !mountedRef.current) return;

          // Turn landed (assistant turn appeared in the transcript) or the
          // backend reports idle. Either way, swap in the canonical record and
          // drop busy. Also refresh the history list so a fresh archive (if the
          // turn triggered a reset) shows up.
          const landed = freshTurns !== null && freshTurns.length > lastTurnCount;
          if (landed || !stillBusy) {
            if (freshTurns) setTurns(freshTurns);
            try {
              const convList = await api.agentListConversations(projectPath);
              if (!cancelled && mountedRef.current) setConversations(convList);
            } catch {
              // non-fatal
            }
            setBusy(false);
            busyRef.current = false;
            return;
          }
          // Still in flight — keep the transcript fresh and poll again.
          if (freshTurns) setTurns(freshTurns);
          lastTurnCount = freshTurns?.length ?? lastTurnCount;
          pollTimer = setTimeout(poll, 1500);
        } catch {
          // Network/IPC hiccup — retry rather than tear down.
          pollTimer = setTimeout(poll, 2000);
        }
      };
      pollTimer = setTimeout(poll, 1500);
    }
    load();
    return () => {
      cancelled = true;
      if (pollTimer) clearTimeout(pollTimer);
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
    // Synchronous busy guard — closes the double-send race that React state
    // can't (two rapid sends both see `busy === false` before re-render).
    if (busyRef.current) return;
    busyRef.current = true;

    setBusy(true);
    setError(null);
    setStreamingBlocks([]);
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
          // Coalesce consecutive text deltas into the trailing text block so
          // the bubble doesn't fragment the reply into one block per delta.
          // A type change (e.g. thinking→text) starts a fresh block.
          setStreamingBlocks((prev) => {
            const next = prev ?? [];
            const last = next[next.length - 1];
            if (last && last.type === "text") {
              return [...next.slice(0, -1), { type: "text", text: last.text + event.text }];
            }
            return [...next, { type: "text", text: event.text }];
          });
          break;
        case "thinking_delta":
          setStreamingBlocks((prev) => {
            const next = prev ?? [];
            const last = next[next.length - 1];
            if (last && last.type === "thinking") {
              return [
                ...next.slice(0, -1),
                { type: "thinking", thinking: last.thinking + event.thinking },
              ];
            }
            return [...next, { type: "thinking", thinking: event.thinking }];
          });
          break;
        case "tool_use":
          // Always a fresh block — a tool call is a discrete event, never
          // coalesced. Keeping it in-order with the surrounding text/thinking
          // is what surfaces live activity mid-turn (reading files, etc.).
          setStreamingBlocks((prev) => [
            ...(prev ?? []),
            { type: "tool_use", name: event.name, input: event.input },
          ]);
          break;
        case "task_update": {
          // A task create/update from the agent. Rendered as a tool_use-style
          // activity row so it reads naturally mid-turn ("✓ Task #10 created:
          // …"), in arrival order with the surrounding tool calls. Foundation
          // for a future Tasks panel — for now it's transcript decoration.
          const tag =
            event.task.status === "created"
              ? "✚"
              : event.task.status === "completed"
                ? "✓"
                : event.task.status === "deleted"
                  ? "✗"
                  : "✎";
          setStreamingBlocks((prev) => [
            ...(prev ?? []),
            {
              type: "tool_use",
              name: `${tag} Task #${event.task.id} ${event.task.status}`,
              input: event.task.subject,
            },
          ]);
          break;
        }
        case "permission_request": {
          // Two distinct meanings depending on `decision`:
          // - "pending": a mutating tool needs the user's Allow/Deny. We park
          //   the turn and surface the approval card; render a ⏳ marker in the
          //   streaming bubble so the user sees what's being asked.
          // - "allow"/"deny": the resolved verdict (either the user's choice on
          //   a pending request, or the synchronous policy for auto-decided
          //   tools). Narrated as a ✓/✗ tool_use-style activity row.
          if (event.decision === "pending") {
            setPendingPermission(projectPath, {
              requestId: event.request_id,
              toolName: event.tool_name,
              input: event.input,
            });
            setStreamingBlocks((prev) => [
              ...(prev ?? []),
              {
                type: "tool_use",
                name: `⏳ ${event.tool_name}`,
                input: event.input,
              },
            ]);
          } else {
            // Resolved — if it matches the pending request, clear the card.
            // (Reads the current store value rather than a functional updater —
            // Zustand actions don't take updaters, and the projectPath keying
            // means we only clear our own entry.)
            const cur = usePendingInteractions.getState().permissions[projectPath];
            if (cur && cur.requestId === event.request_id) {
              clearPendingPermission(projectPath);
            }
            const tag = event.decision === "allow" ? "✓" : "✗";
            const suffix = event.reason ? ` — ${event.reason}` : "";
            setStreamingBlocks((prev) => [
              ...(prev ?? []),
              {
                type: "tool_use",
                name: `${tag} ${event.tool_name}`,
                input: event.input + suffix,
              },
            ]);
          }
          break;
        }
        case "ask_user_question": {
          // Claude is asking the user a clarifying question. The backend read
          // loop is parked until we resolve it via agent_answer_question, so
          // surface the question card and keep busy=true (the turn isn't done).
          setPendingQuestion(projectPath, {
            requestId: event.request_id,
            questions: event.questions,
          });
          break;
        }
        case "result":
          resultHandled = true;
          setStreamingResult(event);
          // NOTE: do NOT null `streamingBlocks` yet. Clearing it here creates a
          // visible gap — the streaming bubble disappears immediately, but the
          // persisted turn only lands in `turns` after `reload()` round-trips
          // through Rust + re-parses active.jsonl. During that window the
          // assistant's reply exists nowhere on screen ("it went missing").
          // Instead we keep the bubble mounted (now showing the Result meta)
          // and clear it only once reload() resolves, then drop busyRef.
          setBusy(false);
          // Turn ended — any pending question/approval is moot (e.g. the turn
          // errored or timed out while parked). Clear so the cards disappear.
          clearPendingQuestion(projectPath);
          clearPendingPermission(projectPath);

          // Surface model-level errors (e.g. "Not logged in") in the banner.
          if (event.is_error) {
            setError(event.result || "The agent turn ended in an error.");
          }

          // Reload transcript so the persisted turn replaces the streaming
          // bubble with the canonical record — then drop the streaming bubble
          // and the busy flag together. busyRef stays true across the reload so
          // a new turn can't preempt the persisted write.
          void reload().finally(() => {
            if (!mountedRef.current) return;
            setStreamingBlocks(null);
            setStreamingResult(null);
            busyRef.current = false;
          });
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
        setStreamingBlocks(null);
        setStreamingResult(null);
        setBusy(false);
        clearPendingQuestion(projectPath);
        clearPendingPermission(projectPath);
        busyRef.current = false;
        void reload();
      }
    } catch (err) {
      // Infra-level error: timeout, no agent config, spawn failure, etc.
      if (mountedRef.current) {
        setError(describeError(err));
        setStreamingBlocks(null);
        setStreamingResult(null);
        setBusy(false);
        clearPendingQuestion(projectPath);
        clearPendingPermission(projectPath);
        busyRef.current = false;
        // Best-effort reload — a failed turn may still have been partially
        // recorded (user turn appended before send).
        void reload();
      }
    }
  }

  /** Start the next development loop (streaming). */
  async function runStartLoop() {
    // Start always begins a fresh (active) conversation server-side, so make
    // sure the view reflects it — otherwise the streaming bubble would render
    // into a read-only archive view.
    if (selectedIdRef.current !== "active") {
      setSelectedId("active");
      const data = await api.agentGetConversationById(projectPath, "active");
      if (mountedRef.current) setTurns(data);
    }
    await runStreamingTurn(undefined);
  }

  /**
   * Send a free-form follow-up message (streaming).
   *
   * If the user is viewing the active conversation, this is a normal send. If
   * they're viewing an ARCHIVE, we first promote it to active (the current
   * active is archived aside, preserving it as history) and switch the view to
   * it — so the follow-up appends to that conversation and the agent resumes
   * its context. Browsing is free; promotion is lazy and happens only on send.
   */
  async function runSendMessage(prompt: string) {
    if (!prompt.trim() || busy) return;
    const text = prompt.trim();

    // Promote-on-send: viewing an archive → make it active before sending.
    if (selectedIdRef.current !== "active") {
      const viewedId = selectedIdRef.current;
      try {
        await api.agentPromoteToActive(projectPath, viewedId);
        // The promoted conversation IS the new active; switch view + reload
        // turns from active so the streaming bubble appends to the right place.
        setSelectedId("active");
        const data = await api.agentGetConversationById(projectPath, "active");
        if (mountedRef.current) setTurns(data);
        await refreshConversations();
      } catch (err) {
        if (mountedRef.current) setError(describeError(err));
        return;
      }
    }

    await runStreamingTurn(text);
  }

  /**
   * Answer a pending AskUserQuestion. Delivers the user's answers to the
   * backend's parked read loop; the turn then resumes with the chosen answers.
   * Keeps `busy=true` — the turn isn't done until the terminal `result` event
   * fires. Clears the question card on success; surfaces an error if the
   * backend has no pending question (turn ended/timed out).
   */
  async function handleAnswerQuestion(answers: AskUserQuestionAnswers) {
    if (!pendingQuestion) return;
    const requestId = pendingQuestion.requestId;
    try {
      await api.agentAnswerQuestion(projectPath, requestId, answers);
      if (mountedRef.current) clearPendingQuestion(projectPath);
    } catch (err) {
      if (mountedRef.current) {
        setError(describeError(err));
        clearPendingQuestion(projectPath);
      }
    }
  }

  /**
   * Resolve a pending manual approval (Allow / Deny). Delivers the verdict to
   * the backend's parked read loop; the turn resumes (allow) or recovers
   * (deny). Keeps `busy=true` — the resolved permission_request event (or the
   * terminal `result`) clears the streaming bubble. Clears the card on
   * success; surfaces an error if the backend has no pending approval.
   */
  async function handleAnswerPermission(decision: ApprovalDecision) {
    if (!pendingPermission) return;
    const requestId = pendingPermission.requestId;
    try {
      await api.agentAnswerPermission(projectPath, requestId, decision);
      // Don't clear `pendingPermission` here — wait for the resolved
      // permission_request event to arrive so the ⏳ marker becomes ✓/✗ in
      // the same flow as auto-decided tools. If the IPC errored, clear + banner.
    } catch (err) {
      if (mountedRef.current) {
        setError(describeError(err));
        clearPendingPermission(projectPath);
      }
    }
  }

  /**
   * Reset: drop the live process and archive the transcript, then focus the
   * composer so the user can type the first message of a fresh conversation.
   * Always available (even on an empty transcript) — that's the point of the
   * button: it's the entry point for starting a new conversation by hand,
   * without having to run a loop first.
   */
  async function runReset() {
    setBusy(true);
    setError(null);
    try {
      await api.agentResetSession(projectPath);
      setSelectedId("active");
      await reload();
      if (mountedRef.current) setFocusNonce((n) => n + 1);
    } catch (err) {
      setError(describeError(err));
    } finally {
      if (mountedRef.current) setBusy(false);
    }
  }

  /**
   * Gracefully interrupt the in-flight turn (Stop button).
   *
   * Asks the backend to fire the interrupt oneshot; the read loop then writes
   * the graceful `interrupt` control_request, ends the turn, and emits a
   * synthesized `Result` event — which our channel handler turns into the
   * normal end-of-turn cleanup (clears streaming state, reloads transcript).
   * We DON'T optimistically flip `busy=false` here: the synthesized Result
   * arrives within a moment and does it, and optimistically clearing would
   * let the user fire a new turn before the process has actually stopped.
   * The live session + its context survive — the next send resumes.
   */
  async function runInterrupt() {
    if (!busy) return;
    try {
      await api.agentInterrupt(projectPath);
    } catch (err) {
      // Surface the error but let the turn keep running — the user can retry.
      setError(describeError(err));
    }
  }

  /** Open a past conversation read-only. */
  async function selectConversation(id: string) {
    setHistoryOpen(false);
    setSelectedId(id);
    await loadConversation(id);
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
        {busy && (
          <button
            onClick={runInterrupt}
            className="inline-flex items-center gap-1.5 h-8 px-3 rounded-md bg-[color-mix(in_oklab,var(--destructive)_12%,transparent)] border border-[color-mix(in_oklab,var(--destructive)_40%,transparent)] text-destructive text-xs font-medium hover:bg-[color-mix(in_oklab,var(--destructive)_20%,transparent)] transition"
            title="Stop the current turn gracefully (the conversation context is kept)"
          >
            <Square className="size-3 fill-current" />
            Stop
          </button>
        )}
        <button
          onClick={runReset}
          disabled={busy}
          className="inline-flex items-center gap-1.5 h-8 px-3 rounded-md bg-muted text-muted-foreground text-xs font-medium hover:bg-accent hover:text-foreground transition disabled:opacity-50 disabled:cursor-not-allowed"
          title="Archive the transcript and start a fresh conversation (type your first message below)"
        >
          <RotateCcw className="size-3.5" />
          New conversation
        </button>

        {/* ── History dropdown ── */}
        <div className="relative ml-auto">
          <button
            onClick={() => setHistoryOpen((v) => !v)}
            className={`inline-flex items-center gap-1.5 h-8 px-3 rounded-md text-xs font-medium transition ${
              selectedId !== "active"
                ? "bg-primary/10 text-primary border border-primary/30"
                : "bg-muted text-muted-foreground hover:bg-accent hover:text-foreground"
            }`}
            title="Browse past conversations"
          >
            <History className="size-3.5" />
            <span>History</span>
            {conversations.length > 0 && (
              <span className="text-[10px] opacity-70">({conversations.length})</span>
            )}
            <ChevronDown className="size-3 opacity-70" />
          </button>

          {historyOpen && (
            <>
              {/* Click-away backdrop */}
              <div
                className="fixed inset-0 z-10"
                onClick={() => setHistoryOpen(false)}
              />
              <div className="absolute right-0 mt-1 w-80 max-h-96 overflow-y-auto rounded-md border border-border bg-popover shadow-lg z-20 py-1 text-foreground">
                {conversations.length === 0 ? (
                  <div className="px-3 py-4 text-xs text-muted-foreground text-center">
                    No conversations yet.
                  </div>
                ) : (
                  conversations.map((c) => {
                    const isSel = c.id === selectedId;
                    return (
                      <button
                        key={c.id}
                        onClick={() => void selectConversation(c.id)}
                        className={`w-full text-left px-3 py-2 flex flex-col gap-0.5 transition ${
                          isSel ? "bg-accent" : "hover:bg-accent/60"
                        }`}
                      >
                        <div className="flex items-center gap-2">
                          <span
                            className={`text-[9px] font-semibold uppercase tracking-wide px-1.5 py-0.5 rounded ${
                              c.kind === "active"
                                ? "bg-primary/15 text-primary"
                                : "bg-muted text-muted-foreground"
                            }`}
                          >
                            {c.kind === "active" ? "Active" : "Archived"}
                          </span>
                          <span className="text-[11px] text-muted-foreground ml-auto">
                            {fmtWhen(c.last_ts)}
                          </span>
                        </div>
                        <div className="text-xs text-foreground/90 truncate">
                          {c.first_user_excerpt || (
                            <span className="italic text-muted-foreground">
                              (no prompts)
                            </span>
                          )}
                        </div>
                        <div className="text-[10px] text-muted-foreground">
                          {c.turn_count} {c.turn_count === 1 ? "turn" : "turns"}
                        </div>
                      </button>
                    );
                  })
                )}
              </div>
            </>
          )}
        </div>
      </div>

      {/* ── Chat (transcript + streaming bubble + composer) ── */}
      <div className="flex-1 min-h-0">
        <Chat
          turns={turns}
          streamingBlocks={isActiveView ? streamingBlocks : null}
          streamingResult={isActiveView ? streamingResult : null}
          busy={busy}
          error={error}
          onSend={runSendMessage}
          onClearError={() => setError(null)}
          focusNonce={focusNonce}
          pendingQuestion={pendingQuestion?.questions ?? null}
          onAnswerQuestion={handleAnswerQuestion}
          pendingPermission={pendingPermission ? {
            toolName: pendingPermission.toolName,
            input: pendingPermission.input,
          } : null}
          onAnswerPermission={handleAnswerPermission}
        />
      </div>
    </div>
  );
}
