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
  PlanApprovalDecision,
} from "../../types";
import * as api from "../../lib/tauri";
import { useAppStore } from "../../store/appStore";
import { usePendingInteractions } from "../../store/pendingInteractions";
import { useStreamingState } from "../../store/streamingState";
import { Chat, buildAllowRule } from "./Chat";
import { TaskPanel } from "./TaskPanel";
import { PermissionModeBadge } from "../shared/PermissionModeBadge";

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

  // ── Streaming state (navigation-stable) ──
  // Lifted OUT of React state into a Zustand store keyed by project path.
  // `AgentPanel` unmounts when the user navigates away from the agent surface;
  // the in-flight streaming bubble + busy flag must survive that unmount so
  // (a) coming back shows the partial assistant reply, and (b) the Tauri
  // Channel callback (owned by whoever kicked off the turn) can keep writing
  // events to the store even with no panel mounted — including the terminal
  // `result` that triggers the transcript reload.
  const busy = useStreamingState((s) => (s.byPath[projectPath]?.busy) ?? false);
  const streamingBlocks = useStreamingState(
    (s) => s.byPath[projectPath]?.streamingBlocks ?? null,
  );
  const streamingResult = useStreamingState(
    (s) => s.byPath[projectPath]?.streamingResult ?? null,
  );
  const pendingUserText = useStreamingState(
    (s) => s.byPath[projectPath]?.pendingUserText ?? null,
  );
  const retrying = useStreamingState(
    (s) => s.byPath[projectPath]?.retrying ?? null,
  );
  const error = useStreamingState((s) => s.byPath[projectPath]?.error ?? null);
  // Per-project autonomous flag — drives the PermissionModeBadge label so the
  // user sees at a glance whether this project's agent self-approves tool calls.
  const autonomous = useAppStore(
    (s) => s.projects.find((p) => p.path === projectPath)?.autonomous ?? false,
  );

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
  const pendingPlan = usePendingInteractions((s) => s.plans[projectPath] ?? null);
  const setPendingQuestion = usePendingInteractions((s) => s.setQuestion);
  const clearPendingQuestion = usePendingInteractions((s) => s.clearQuestion);
  const setPendingPermission = usePendingInteractions((s) => s.setPermission);
  const clearPendingPermission = usePendingInteractions((s) => s.clearPermission);
  const setPendingPlan = usePendingInteractions((s) => s.setPlan);
  const clearPendingPlan = usePendingInteractions((s) => s.clearPlan);

  // ── Plan-mode toggle ──
  // Local UI intent: "should the NEXT sent message run under the CLI's `plan`
  // permission mode". Not navigation-stable (unlike the streaming/pending
  // state above) — it's a one-shot compose-time choice, not something a
  // parked backend turn needs to survive a remount. Reset to false right
  // after firing a send so a subsequent follow-up defaults back to normal
  // mode, mirroring Claude Code's own shift-tab behavior (plan mode reverts
  // once you leave it).
  const [planMode, setPlanMode] = useState(false);

  // Plan mode is a Claude-CLI-specific concept (`set_permission_mode` +
  // `ExitPlanMode`) — Codex has no equivalent and always runs with
  // `workspace-write` (see the harness-boundary decision in
  // `.loopdeck/decisions.md`), so it would edit immediately despite the UI
  // promising a plan-first review. `AgentConfig.harness` is a single global
  // setting (not per-project), fetched once here so the toggle can be hidden
  // entirely for Codex rather than silently accepted and ignored.
  const [harness, setHarness] = useState<"claude" | "codex">("claude");
  useEffect(() => {
    let cancelled = false;
    api
      .getAgentConfig()
      .then((cfg) => {
        if (!cancelled) setHarness(cfg?.harness ?? "claude");
      })
      .catch(() => {
        // Best-effort — default to "claude" so the toggle degrades to its
        // pre-Codex behavior on a probe failure rather than disappearing.
      });
    return () => {
      cancelled = true;
    };
  }, []);
  // Defense in depth alongside hiding the toggle below: even if `planMode`
  // somehow ended up true while Codex is active (e.g. a harness switch
  // landing between toggling on and sending), never actually request plan
  // mode for a harness that can't honor it.
  const planModeUsable = harness !== "codex";

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

  // ── Streaming-state mutators (write through to the navigation-stable store). ──
  // Thin wrappers so the orchestration code below keeps reading like the old
  // useState version — every setX here is a store patch keyed by projectPath.
  const setBusy = useCallback(
    (b: boolean) => useStreamingState.getState().patch(projectPath, { busy: b }),
    [projectPath],
  );
  const setError = useCallback(
    (e: string | null) => useStreamingState.getState().patch(projectPath, { error: e }),
    [projectPath],
  );
  const setStreamingResult = useCallback(
    (r: (ClaudeEvent & { type: "result" }) | null) =>
      useStreamingState.getState().patch(projectPath, { streamingResult: r }),
    [projectPath],
  );
  /** Replace the streaming blocks. Pass null to clear (hide the bubble). */
  const setStreamingBlocks = useCallback(
    (
      next:
        | ContentBlock[]
        | null
        | ((prev: ContentBlock[] | null) => ContentBlock[] | null),
    ) => {
      const store = useStreamingState.getState();
      if (typeof next === "function") {
        const prev = store.byPath[projectPath]?.streamingBlocks ?? null;
        store.patch(projectPath, { streamingBlocks: next(prev) });
      } else {
        store.patch(projectPath, { streamingBlocks: next });
      }
    },
    [projectPath],
  );

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
        !!pending.permissions[projectPath] ||
        !!pending.questions[projectPath] ||
        !!pending.plans[projectPath];
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

      // Reconcile parked prompts (manual approval / AskUserQuestion). The
      // backend stores the parking payload in AppState.pending_* alongside
      // the oneshot sender (not just as a ClaudeEvent on the streaming
      // channel), so we can re-materialize the cards here even when the
      // previous mount's channel subscriber is gone. Only seed the store
      // when it doesn't already hold the entry — the channel-event path
      // remains the source of truth when it's working.
      if (!cancelled) {
        try {
          const pendingStore = usePendingInteractions.getState();
          const [perm, q, plan] = await Promise.all([
            api.agentPendingPermission(projectPath),
            api.agentPendingQuestion(projectPath),
            api.agentPendingPlan(projectPath),
          ]);
          if (perm && !pendingStore.permissions[projectPath]) {
            setPendingPermission(projectPath, {
              requestId: perm.requestId,
              toolName: perm.toolName,
              input: perm.input,
            });
          } else if (!perm && pendingStore.permissions[projectPath]) {
            // Backend says nothing pending but the store does — clear the
            // stale entry (turn ended while the user was away).
            clearPendingPermission(projectPath);
          }
          if (q && !pendingStore.questions[projectPath]) {
            setPendingQuestion(projectPath, {
              requestId: q.requestId,
              questions: q.questions,
            });
          } else if (!q && pendingStore.questions[projectPath]) {
            clearPendingQuestion(projectPath);
          }
          // Unlike the permission/question checks above, also refresh when
          // the backend's pending request_id has moved on — the model can
          // revise and re-call ExitPlanMode mid-turn, so a stored plan A
          // whose request_id no longer matches the backend's snapshot is
          // stale, not merely "already seeded". Without this, a missed
          // `plan_approval` channel event would leave the superseded plan A
          // on screen while the backend is actually parked on plan B.
          const storedPlan = pendingStore.plans[projectPath];
          if (plan && (!storedPlan || storedPlan.requestId !== plan.requestId)) {
            setPendingPlan(projectPath, {
              requestId: plan.requestId,
              plan: plan.plan,
            });
          } else if (!plan && storedPlan) {
            clearPendingPlan(projectPath);
          }
        } catch {
          // Best-effort reconciliation — don't fail the mount on a probe error.
        }
      }

      // Recompute parking state after the reconciliation above may have
      // populated the store. Used below to decide whether to also poll for
      // a non-parked in-flight turn.
      const hasPendingParkingAfter =
        !!usePendingInteractions.getState().permissions[projectPath] ||
        !!usePendingInteractions.getState().questions[projectPath] ||
        !!usePendingInteractions.getState().plans[projectPath];

      // Reconcile a turn that's in flight but NOT parked on the user. This
      // happens when the previous mount unmounted mid-streaming: the Tauri
      // Channel it subscribed to is gone, so streaming events from the
      // still-running backend can no longer reach the UI. We ask the backend
      // whether a turn is in flight; if so, show an honest "Agent is working…"
      // state and poll the transcript until the persisted turn lands or the
      // backend reports idle. We snapshot the turn count to detect landing
      // (the assistant turn only appears in `active.jsonl` once the backend
      // finishes writing it).
      if (cancelled || hasPendingParkingAfter) return;
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
   * @param usePlanMode  Only meaningful when `prompt` is set — runs the turn
   *                under the CLI's `plan` permission mode. Loop starts never
   *                use this (no prompt ⇒ always a normal turn).
   */
  async function runStreamingTurn(prompt?: string, usePlanMode = false) {
    // Synchronous busy guard — closes the double-send race that React state
    // can't (two rapid sends both see `busy === false` before re-render).
    if (busyRef.current) return;
    busyRef.current = true;

    setBusy(true);
    setError(null);
    setStreamingBlocks([]);
    setStreamingResult(null);
    // Reset the live-task state for this turn so the floating TaskPanel starts
    // clean — the agent's TodoWrite always begins a fresh todo set per turn.
    useStreamingState.getState().patch(projectPath, { tasks: {} });

    const channel = new Channel<ClaudeEvent>();

    // Guards against double-reload if the invoke Promise resolves before the
    // last channel event is delivered (shouldn't happen with Tauri v2's ordered
    // IPC, but defensive coding is cheap).
    let resultHandled = false;

    channel.onmessage = (event: ClaudeEvent ) => {
      // NOTE: we deliberately do NOT bail on `!mountedRef.current` here.
      // Streaming state lives in the navigation-stable store, so we keep
      // accumulating blocks / busy / result even when no panel is mounted —
      // that's the whole point of lifting it out of React state. A freshly-
      // mounted panel picks up the in-flight bubble verbatim, and the
      // terminal `result` triggers the transcript reload regardless of whether
      // the originating panel is still on screen. React-state writes (turns,
      // conversations) below are guarded individually.

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
          //
          // EXCEPTION: the `AskUserQuestion` tool call. Its tool_use block
          // arrives just before the dedicated `ask_user_question` channel
          // event, and rendering it here would show a noisy
          // `› AskUserQuestion · {questions json}` row right above the
          // question card itself — pure duplication. The question card (the
          // pinned AskUserQuestionCard) is the user-facing surface for this
          // interaction, so the raw tool-call row is dropped everywhere.
          if (event.name === "AskUserQuestion") break;
          // Task lifecycle tools are surfaced via the dedicated `task_update`
          // channel event (→ TaskPanel), not as transcript activity rows. The
          // backend suppresses `TaskUpdate` tool_use blocks at the source too;
          // this guard is belt-and-suspenders against drift between the two.
          if (event.name === "TaskCreate" || event.name === "TaskUpdate") break;
          setStreamingBlocks((prev) => [
            ...(prev ?? []),
            { type: "tool_use", name: event.name, input: event.input },
          ]);
          break;
        case "task_update": {
          // A task create/update from the agent. Folded into the navigation-
          // stable `tasks` map (latest-wins-by-id) which drives the floating
          // TaskPanel — that's now the single, dedicated surface for task
          // state. No transcript row: rendering it here too (as a "✚ Task #N
          // created" activity line) duplicated the panel and cluttered the
          // message flow.
          useStreamingState.getState().applyTask(projectPath, event.task);
          break;
        }
        case "permission_request": {
          // Two distinct meanings depending on `decision`:
          // - "pending": a mutating tool needs the user's Allow/Deny. We park
          //   the turn and surface the pinned approval card.
          // - "allow"/"deny": the resolved verdict (the user's choice on a
          //   pending request, or the synchronous policy for auto-decided
          //   tools). If it matches the pending request, the card clears.
          //
          // Neither decision is rendered as a transcript activity row anymore:
          // the approval card is the dedicated surface while pending, and a
          // ✓/✗ marker line in the message flow just duplicated that (and
          // vanished on reload anyway, since permission events aren't
          // persisted as turn content).
          if (event.decision === "pending") {
            setPendingPermission(projectPath, {
              requestId: event.request_id,
              toolName: event.tool_name,
              input: event.input,
            });
          } else {
            // Resolved — if it matches the pending request, clear the card.
            // (Reads the current store value rather than a functional updater —
            // Zustand actions don't take updaters, and the projectPath keying
            // means we only clear our own entry.)
            const cur = usePendingInteractions.getState().permissions[projectPath];
            if (cur && cur.requestId === event.request_id) {
              clearPendingPermission(projectPath);
            }
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
        case "plan_approval": {
          // Same pending/resolved shape as permission_request, but for
          // ExitPlanMode: "pending" parks the turn and surfaces the plan
          // card; "allow"/"deny"/"auto-allow" is the resolved verdict — if it
          // matches the pending request, the card clears.
          if (event.decision === "pending") {
            setPendingPlan(projectPath, {
              requestId: event.request_id,
              plan: event.plan,
            });
          } else {
            const cur = usePendingInteractions.getState().plans[projectPath];
            if (cur && cur.requestId === event.request_id) {
              clearPendingPlan(projectPath);
            }
          }
          break;
        }
        case "retrying": {
          // A transient gateway overload (e.g. 529) was hit and the backend is
          // retrying after a backoff. Without surfacing this the UI looks frozen
          // — the failed attempt's terminal Result has landed and the next
          // attempt hasn't started. Stash the payload so Chat can render an
          // honest "Retrying 2/9 in 4s…" row. Cleared by the next non-retry
          // `result` (success, non-transient error, or exhausted retries).
          useStreamingState.getState().patch(projectPath, {
            retrying: {
              attempt: event.attempt,
              maxAttempts: event.max_attempts,
              backoffMs: event.backoff_ms,
              error: event.error,
            },
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
          // A terminal result supersedes any retry indicator — the turn is over
          // (success, non-transient error, or retries exhausted). Drop the
          // payload so the "Retrying…" row disappears.
          useStreamingState.getState().patch(projectPath, { retrying: null });
          // Turn ended — any pending question/approval is moot (e.g. the turn
          // errored or timed out while parked). Clear so the cards disappear.
          clearPendingQuestion(projectPath);
          clearPendingPermission(projectPath);
          clearPendingPlan(projectPath);

          // Surface model-level errors (e.g. "Not logged in") in the banner.
          if (event.is_error) {
            setError(event.result || "The agent turn ended in an error.");
          }

          // Reload transcript so the persisted turn replaces the streaming
          // bubble with the canonical record — then drop the streaming bubble
          // and the busy flag together. busyRef stays true across the reload so
          // a new turn can't preempt the persisted write.
          //
          // We clear the streaming state unconditionally (even if this panel
          // unmounted) — the store is navigation-stable, so leaving the bubble
          // mounted forever would resurface a stale reply on next visit. The
          // transcript reload inside `reload()` guards its own React-state
          // writes against unmounted components.
          void reload().finally(() => {
            setStreamingBlocks(null);
            setStreamingResult(null);
            // Clear the ephemeral user bubble — the canonical user turn is now
            // in `turns` (via reload), so this field must drop to avoid a flash
            // of two user bubbles during the swap.
            useStreamingState.getState().patch(projectPath, { pendingUserText: null });
            // Keep the TaskPanel visible a beat longer than the streaming
            // bubble — the turn's completed tasks are the most useful thing to
            // glance at right as the turn lands, so collapsing them instantly
            // (the moment the streaming bubble goes) would flash the final
            // state away before the user registers it. 3.5s lets the user see
            // "5/5 done," then the panel auto-hides.
            setTimeout(() => {
              useStreamingState.getState().patch(projectPath, { tasks: {} });
            }, 3500);
            busyRef.current = false;
          });
          break;
      }
    };

    try {
      if (prompt !== undefined) {
        await api.agentSendMessageStreaming(projectPath, prompt, channel, usePlanMode);
      } else {
        await api.agentStartLoopStreaming(projectPath, channel);
      }

      // Fallback: if the Result event hasn't fired yet (unlikely), reload.
      // Unconditional on streaming state — see the result arm above.
      if (!resultHandled) {
        setStreamingBlocks(null);
        setStreamingResult(null);
        useStreamingState.getState().patch(projectPath, { tasks: {}, pendingUserText: null });
        setBusy(false);
        clearPendingQuestion(projectPath);
        clearPendingPermission(projectPath);
        clearPendingPlan(projectPath);
        busyRef.current = false;
        void reload();
      }
    } catch (err) {
      // Infra-level error: timeout, no agent config, spawn failure, etc.
      // Streaming-state cleanup is unconditional so a stale error / bubble
      // doesn't resurface on next visit; React-state writes inside reload()
      // guard themselves against unmounted components.
      setError(describeError(err));
      setStreamingBlocks(null);
      setStreamingResult(null);
      useStreamingState.getState().patch(projectPath, { tasks: {}, pendingUserText: null });
      setBusy(false);
      clearPendingQuestion(projectPath);
      clearPendingPermission(projectPath);
      clearPendingPlan(projectPath);
      busyRef.current = false;
      // Best-effort reload — a failed turn may still have been partially
      // recorded (user turn appended before send).
      void reload();
    }
  }

  /** Start the next development loop (streaming). */
  async function runStartLoop() {
    // Start always begins a FRESH conversation server-side: `spawn_fresh`
    // archives the current `active.jsonl` (it becomes history) and spawns a
    // new claude process without `--resume`. So the on-disk `active` transcript
    // is now empty — we must reload from it unconditionally, even when already
    // viewing `"active"`. The old code only reloaded when switching from an
    // archive view, which left the previous loop's turns rendered while the
    // new loop prompt streamed in on top of them.
    setSelectedId("active");
    const data = await api.agentGetConversationById(projectPath, "active");
    if (mountedRef.current) setTurns(data);
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
    if (!prompt.trim()) return;
    // Synchronous busy guard — same race `runStreamingTurn` closes. React's
    // `busy` state is stale within the render cycle, so a rapid double-send
    // (double-click, double Enter before re-render) would pass a `!busy` check
    // twice. Both calls would then append an optimistic user turn before
    // `runStreamingTurn`'s `busyRef` lock blocked the second *stream* — leaving
    // the user's message inserted twice. Checking `busyRef` here closes that at
    // the source: the second call bails before mutating anything.
    if (busyRef.current) return;
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

    // Surface the user's message as an EPHEMERAL bubble via the streaming
    // store (pendingUserText), NOT by appending to `turns`. The old approach —
    // an optimistic `setTurns` append — raced with the canonical transcript
    // reload: under React's async batching the optimistic append could flush
    // AFTER the `setTurns(data)` replace, re-appending a second copy of the
    // message and rendering it twice. Carrying the in-flight text here, outside
    // `turns`, means `turns` only ever holds canonical disk records — a
    // duplicate is structurally impossible. Chat renders `pendingUserText` as a
    // user bubble above the streaming bubble; the canonical user turn lands in
    // `turns` after reload and this field clears in the same cleanup pass.
    useStreamingState.getState().patch(projectPath, { pendingUserText: text });

    // Snapshot + reset the toggle before the `await` below — a one-shot
    // compose-time choice, not a standing mode. Resetting eagerly (rather
    // than after the turn completes) means the composer honestly reflects
    // "plan mode is off again" the instant the message is sent, matching
    // Claude Code's own shift-tab behavior. Gated on `planModeUsable` so a
    // harness switch to Codex landing between "toggle on" and "send" can't
    // request a plan-mode turn the active harness can't honor.
    const usePlanMode = planMode && planModeUsable;
    setPlanMode(false);

    await runStreamingTurn(text, usePlanMode);
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
   * Resolve a pending plan approval (Approve / Reject). Delivers the verdict
   * to the backend's parked read loop; on approve the agent starts
   * executing, on reject it revises the plan and calls `ExitPlanMode` again.
   * Mirrors `handleAnswerPermission`: keeps the card up until the resolved
   * `plan_approval` event arrives, so the pending marker's ✓/✗ transition
   * matches the same flow as auto-decided tools.
   */
  async function handleAnswerPlan(decision: PlanApprovalDecision) {
    if (!pendingPlan) return;
    const requestId = pendingPlan.requestId;
    try {
      await api.agentAnswerPlan(projectPath, requestId, decision);
    } catch (err) {
      if (mountedRef.current) {
        setError(describeError(err));
        clearPendingPlan(projectPath);
      }
    }
  }

  /**
   * "Always allow": resolve the current approval as an Allow, AND persist a
   * permission rule so future calls of the same tool/command short-circuit via
   * Claude Code's own allow-list. The rule is written to
   * `.claude/settings.local.json` and takes effect on the next spawned session;
   * the current approval is resolved immediately so the parked turn resumes.
   *
   * The two operations (resolve-now + remember-for-later) are intentionally
   * independent: a rule-write failure doesn't block the approval, and an
   * approval-delivery failure still surfaces — but the rule may have written.
   * Either failure is shown in the banner without wedging the turn.
   */
  async function handleAlwaysAllow(toolName: string, input: string) {
    if (!pendingPermission) return;
    const requestId = pendingPermission.requestId;
    const rule = buildAllowRule(toolName, input);

    // Resolve the current approval first — unblocks the parked turn.
    try {
      await api.agentAnswerPermission(projectPath, requestId, { allow: true });
    } catch (err) {
      if (mountedRef.current) {
        setError(describeError(err));
        clearPendingPermission(projectPath);
      }
      return; // No point writing a rule if the approval itself failed to land.
    }

    // Best-effort rule write — surfaces a banner on failure but doesn't undo
    // the approval. Most failures here (disk full, permissions) leave the
    // turn running normally; the user just won't get auto-allow next time.
    try {
      await api.agentAddAllowRule(projectPath, rule);
    } catch (err) {
      if (mountedRef.current) {
        setError(
          `Allowed this call, but couldn't save the "always allow" rule: ${describeError(err)}`,
        );
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
    // Root spans the full content area and is the positioning context for the
    // floating TaskPanel (`absolute right-0 top-0` below). Previously the root
    // was `max-w-3xl` and was itself the panel's parent, so the panel pinned to
    // the chat column's right edge instead of the page's. Now the chat column
    // is a centered inner element (`mx-auto max-w-3xl`) and the panel floats
    // over whatever empty space sits to its right at fullscreen width.
    <div className="relative flex h-full min-h-0 flex-1 flex-col">
      {/* Floating live-task panel — pinned to the TOP-RIGHT of the whole
          content area, not the chat column. `pointer-events-none` on the
          wrapper lets clicks fall through to the transcript where the panel
          has no content; the panel itself re-enables pointer events. Renders
          null when there are no tasks (outside an in-flight turn), so the
          overlay is absent entirely then. */}
      <div className="pointer-events-none absolute right-0 top-0 z-20 p-1">
        <TaskPanel projectPath={projectPath} />
      </div>

      {/* Centered chat column — toolbar + transcript + composer. `max-w-3xl`
          caps the readable line length; `mx-auto` centers it within the full
          width so the empty space splits evenly left/right (and the floating
          TaskPanel sits in the right-hand gutter at fullscreen). */}
      <div className="mx-auto flex h-full min-h-0 w-full max-w-3xl flex-1 flex-col">
      {/* ── Toolbar ── */}
      <div className="flex items-center gap-2 pb-3 mb-3 border-b border-border shrink-0">
        <PermissionModeBadge mode={autonomous ? "autonomous" : "confirm"} />
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
      <Chat
        projectPath={projectPath}
        turns={turns}
        streamingBlocks={isActiveView ? streamingBlocks : null}
        streamingResult={isActiveView ? streamingResult : null}
        pendingUserText={isActiveView ? pendingUserText : null}
        busy={busy}
        retrying={isActiveView ? retrying : null}
        autonomous={autonomous}
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
        onAlwaysAllow={handleAlwaysAllow}
        pendingPlan={pendingPlan ? { plan: pendingPlan.plan } : null}
        onAnswerPlan={handleAnswerPlan}
        // Omitted entirely (not just disabled) when the active harness can't
        // honor plan mode — Chat.tsx hides the toggle button whenever
        // `onTogglePlanMode` is undefined, per its documented prop contract.
        {...(planModeUsable
          ? {
              planMode,
              onTogglePlanMode: () => setPlanMode((v) => !v),
            }
          : {})}
      />
      </div>
    </div>
  );
}
