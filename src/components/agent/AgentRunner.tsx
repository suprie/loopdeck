import { useState, useEffect, useRef, useCallback } from "react";
import { Channel } from "@tauri-apps/api/core";
import {
  Terminal,
  Play,
  RotateCcw,
  Loader2,
  Bot,
  AlertTriangle,
  ChevronRight,
  Circle,
  Hash,
  Clock,
} from "lucide-react";
import type {
  ProjectEntry,
  ConversationTurn,
  ClaudeEvent,
  AppError,
  ToolCall,
  AskUserQuestionSpec,
  AskUserQuestionAnswers,
} from "../../types";
import * as api from "../../lib/tauri";
import { useAppStore } from "../../store/appStore";

// ── Helpers ──────────────────────────────────────────────────────────────────

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

/** Strip ANSI escape codes. */
function sanitise(text: string): string {
  return text.replace(/\x1b\[[0-9;]*[a-zA-Z]/g, "");
}

/** Format a short relative-time string. */
function timeAgo(iso: string): string {
  const ms = Date.now() - new Date(iso).getTime();
  const sec = Math.floor(ms / 1000);
  if (sec < 60) return `${sec}s ago`;
  const min = Math.floor(sec / 60);
  if (min < 60) return `${min}m ago`;
  const hr = Math.floor(min / 60);
  if (hr < 24) return `${hr}h ago`;
  return `${Math.floor(hr / 24)}d ago`;
}

/** Extract a destructured tool-input summary for terminal display. */
function describeTool(name: string, rawInput: string): string {
  let input: Record<string, unknown> = {};
  try {
    const parsed = JSON.parse(rawInput);
    if (parsed && typeof parsed === "object" && !Array.isArray(parsed)) {
      input = parsed as Record<string, unknown>;
    }
  } catch { /* not JSON */ }
  const str = (v: unknown): string =>
    typeof v === "string" ? v : JSON.stringify(v);
  const candidate =
    input.file_path ?? input.path ?? input.command ?? input.pattern ?? input.query ?? input.url;
  const detail = candidate
    ? str(candidate)
    : rawInput && rawInput !== "{}"
      ? rawInput
      : "";
  return detail ? `${name} ${detail}` : name;
}

/** Format token count for terminal display. */
function fmtTokens(n: number): string {
  if (n >= 1_000_000) return `${(n / 1_000_000).toFixed(1)}M`;
  if (n >= 1_000) return `${(n / 1_000).toFixed(1)}k`;
  return String(n);
}

// ── Types ────────────────────────────────────────────────────────────────────

interface SessionMeta {
  project: ProjectEntry;
  transcript: ConversationTurn[];
  /** Last assistant turn timestamp (for "idle since" display). */
  lastActivity: string | null;
  /** How many turns are in the transcript. */
  turnCount: number;
  /** Whether there is a live ClaudeSession spawned for this project. */
  hasLive: boolean;
}

// ── AgentRunner ──────────────────────────────────────────────────────────────

export function AgentRunner() {
  // ── Project registry ──
  const projects = useAppStore((s) => s.projects);
  const [sessions, setSessions] = useState<SessionMeta[]>([]);
  const [scanning, setScanning] = useState(true);
  const [registryError, setRegistryError] = useState<string | null>(null);

  // ── Selected project ──
  const [selectedPath, setSelectedPath] = useState<string | null>(null);

  // ── Transcript for selected project ──
  const [turns, setTurns] = useState<ConversationTurn[]>([]);
  const [loadingTranscript, setLoadingTranscript] = useState(false);

  // ── Streaming state ──
  const [busy, setBusy] = useState(false);
  const [streamingText, setStreamingText] = useState<string | null>(null);
  const [streamingTools, setStreamingTools] = useState<ToolCall[] | null>(null);
  const [streamingResult, setStreamingResult] =
    useState<(ClaudeEvent & { type: "result" }) | null>(null);
  const [error, setError] = useState<string | null>(null);

  // ── Pending AskUserQuestion — set when Claude asks the user a question
  // mid-turn. The backend read loop parks until we resolve it via
  // agent_answer_question. Cleared on submit or when the turn ends.
  const [pendingQuestion, setPendingQuestion] = useState<{
    requestId: string;
    questions: AskUserQuestionSpec[];
  } | null>(null);

  // ── Composer ──
  const [draft, setDraft] = useState("");
  const inputRef = useRef<HTMLInputElement>(null);
  const outputRef = useRef<HTMLDivElement>(null);

  // Mounted guard for channel events.
  const mountedRef = useRef(true);
  useEffect(() => {
    return () => { mountedRef.current = false; };
  }, []);

  // ── Scan: build session metadata for each registered project ─────────────

  useEffect(() => {
    let cancelled = false;

    async function scan() {
      setScanning(true);
      setRegistryError(null);

      const results: SessionMeta[] = [];

      for (const p of projects) {
        if (cancelled) break;
        try {
          const transcript = await api.agentGetConversation(p.path);
          const lastAsst = [...transcript].reverse().find((t) => t.role === "assistant");
          // Detect live session by checking if there's an active process for this project.
          // We approximate by checking if the most recent turn was within the last 30 minutes.
          let hasLive = false;
          if (lastAsst) {
            const age = Date.now() - new Date(lastAsst.ts).getTime();
            hasLive = age < 30 * 60_000;
          }
          results.push({
            project: p,
            transcript,
            lastActivity: lastAsst?.ts ?? null,
            turnCount: transcript.length,
            hasLive,
          });
        } catch {
          // No transcript yet — treat as idle, no session.
          results.push({
            project: p,
            transcript: [],
            lastActivity: null,
            turnCount: 0,
            hasLive: false,
          });
        }
      }

      if (!cancelled) {
        // Sort: live sessions first, then by last activity.
        results.sort((a, b) => {
          if (a.hasLive !== b.hasLive) return a.hasLive ? -1 : 1;
          if (a.lastActivity && b.lastActivity) {
            return b.lastActivity.localeCompare(a.lastActivity);
          }
          return a.lastActivity ? -1 : 1;
        });
        setSessions(results);
        setScanning(false);
      }
    }

    if (projects.length > 0) {
      scan();
    } else {
      setScanning(false);
      setSessions([]);
    }

    return () => { cancelled = true; };
  }, [projects]);

  // ── Load transcript when selected project changes ───────────────────────

  const loadTranscript = useCallback(async (path: string) => {
    setLoadingTranscript(true);
    setError(null);
    try {
      const data = await api.agentGetConversation(path);
      if (mountedRef.current) setTurns(data);
    } catch (err) {
      if (mountedRef.current) setError(describeError(err));
    } finally {
      if (mountedRef.current) setLoadingTranscript(false);
    }
  }, []);

  useEffect(() => {
    if (selectedPath) {
      loadTranscript(selectedPath);
    } else {
      setTurns([]);
    }
  }, [selectedPath, loadTranscript]);

  // ── Auto-scroll output ──────────────────────────────────────────────────

  useEffect(() => {
    if (outputRef.current) {
      outputRef.current.scrollTop = outputRef.current.scrollHeight;
    }
  }, [turns, streamingText, streamingTools, busy]);

  // ── Streaming orchestration ─────────────────────────────────────────────

  async function runStreamingTurn(prompt?: string) {
    if (!selectedPath) return;
    setBusy(true);
    setError(null);
    setStreamingText("");
    setStreamingTools([]);
    setStreamingResult(null);

    const channel = new Channel<ClaudeEvent>();
    let resultHandled = false;

    channel.onmessage = (event: ClaudeEvent) => {
      if (!mountedRef.current) return;
      switch (event.type) {
        case "text_delta":
          setStreamingText((prev) => (prev ?? "") + event.text);
          break;
        case "tool_use":
          setStreamingTools((prev) => [
            ...(prev ?? []),
            { name: event.name, input: event.input },
          ]);
          break;
        case "ask_user_question":
          // Agent is asking the user a clarifying question. The backend read
          // loop parks until we answer via agent_answer_question.
          setPendingQuestion({
            requestId: event.request_id,
            questions: event.questions,
          });
          break;
        case "result":
          resultHandled = true;
          setStreamingResult(event);
          setStreamingText(null);
          setStreamingTools(null);
          setBusy(false);
          // Turn ended — clear any pending question (e.g. errored while parked).
          setPendingQuestion(null);
          if (event.is_error) {
            setError(event.result || "The agent turn ended in an error.");
          }
          loadTranscript(selectedPath!);
          break;
      }
    };

    try {
      if (prompt !== undefined) {
        await api.agentSendMessageStreaming(selectedPath!, prompt, channel);
      } else {
        await api.agentStartLoopStreaming(selectedPath!, channel);
      }
      if (!resultHandled && mountedRef.current) {
        setStreamingText(null);
        setStreamingTools(null);
        setStreamingResult(null);
        setBusy(false);
        setPendingQuestion(null);
        loadTranscript(selectedPath!);
      }
    } catch (err) {
      if (mountedRef.current) {
        setError(describeError(err));
        setStreamingText(null);
        setStreamingTools(null);
        setStreamingResult(null);
        setBusy(false);
        setPendingQuestion(null);
        loadTranscript(selectedPath!);
      }
    }
  }

  /**
   * Answer a pending AskUserQuestion. Delivers the user's answers to the
   * backend's parked read loop; the turn resumes. Keeps busy=true until the
   * terminal `result` event fires.
   */
  async function handleAnswerQuestion(answers: AskUserQuestionAnswers) {
    if (!pendingQuestion || !selectedPath) return;
    const requestId = pendingQuestion.requestId;
    try {
      await api.agentAnswerQuestion(selectedPath, requestId, answers);
      if (mountedRef.current) setPendingQuestion(null);
    } catch (err) {
      if (mountedRef.current) {
        setError(describeError(err));
        setPendingQuestion(null);
      }
    }
  }

  /**
   * Reset the selected project's agent session: archive the transcript and
   * focus the composer so the user can type the first message of a fresh
   * conversation. Available even on an empty transcript — that's the entry
   * point for starting a conversation by hand, without running a loop first.
   */
  async function handleReset() {
    if (!selectedPath) return;
    setBusy(true);
    setError(null);
    try {
      await api.agentResetSession(selectedPath);
      await loadTranscript(selectedPath);
      if (mountedRef.current) inputRef.current?.focus();
    } catch (err) {
      setError(describeError(err));
    } finally {
      if (mountedRef.current) setBusy(false);
    }
  }

  /** Send from composer. */
  function handleSend() {
    const text = draft.trim();
    if (!text || busy || !selectedPath) return;
    setDraft("");
    runStreamingTurn(text);
  }

  const selectedSession = sessions.find((s) => s.project.path === selectedPath);
  const selectedProject = selectedSession?.project ?? null;
  const isStreaming = streamingText !== null;

  // ── Render ──────────────────────────────────────────────────────────────

  return (
    <div className="flex h-full">
      {/* ── Left panel: project list ── */}
      <aside className="w-64 shrink-0 border-r border-border bg-surface/60 flex flex-col">
        <div className="px-4 h-10 flex items-center border-b border-border">
          <Terminal className="size-3.5 text-muted-foreground mr-2" />
          <span className="text-xs font-semibold tracking-tight">Sessions</span>
          {scanning && <Loader2 className="size-3 ml-auto animate-spin text-muted-foreground" />}
        </div>

        <div className="flex-1 overflow-y-auto">
          {registryError && (
            <div className="mx-2 mt-2 p-2 rounded bg-[color-mix(in_oklab,var(--destructive)_10%,transparent)] text-destructive text-[10px]">
              {registryError}
            </div>
          )}

          {!scanning && sessions.length === 0 && (
            <div className="px-4 py-8 text-center text-xs text-muted-foreground">
              <Hash className="size-5 mx-auto mb-2 opacity-30" />
              No projects registered.
              <br />
              Import a repo to get started.
            </div>
          )}

          {sessions.map((s) => {
            const isActive = s.project.path === selectedPath;
            const isLive = s.hasLive;
            return (
              <button
                key={s.project.path}
                onClick={() => setSelectedPath(s.project.path)}
                className={`w-full text-left px-4 py-2.5 border-b border-border/50 transition-colors ${
                  isActive
                    ? "bg-accent text-foreground"
                    : "hover:bg-accent/30 text-muted-foreground hover:text-foreground"
                }`}
              >
                <div className="flex items-center gap-2 mb-0.5">
                  {/* Status dot */}
                  <Circle
                    className={`size-2 shrink-0 ${
                      isLive
                        ? "fill-[var(--success)] text-[var(--success)]"
                        : "fill-muted-foreground/30 text-muted-foreground/30"
                    }`}
                  />
                  <span className="text-xs font-medium truncate flex-1">
                    {s.project.name}
                  </span>
                  <ChevronRight className="size-3 opacity-30" />
                </div>
                <div className="flex items-center gap-2 text-[10px] text-muted-foreground">
                  {s.turnCount > 0 ? (
                    <>
                      <span>{s.turnCount} turn{s.turnCount !== 1 ? "s" : ""}</span>
                      {s.lastActivity && (
                        <>
                          <span>·</span>
                          <Clock className="size-2.5" />
                          <span>{timeAgo(s.lastActivity)}</span>
                        </>
                      )}
                    </>
                  ) : (
                    <span className="italic">No activity</span>
                  )}
                </div>
                <div className="text-[9px] text-muted-foreground/50 truncate mt-0.5 font-mono">
                  {s.project.path}
                </div>
              </button>
            );
          })}
        </div>

        {/* Quick stats */}
        <div className="px-4 py-2 border-t border-border text-[10px] text-muted-foreground flex items-center justify-between">
          <span>{sessions.length} project{sessions.length !== 1 ? "s" : ""}</span>
          <span>
            {sessions.filter((s) => s.hasLive).length} live
          </span>
        </div>
      </aside>

      {/* ── Right panel: terminal ── */}
      <main className="flex-1 min-w-0 flex flex-col bg-[oklch(0.13_0.01_270)]">
        {/* Terminal header */}
        <div className="h-10 shrink-0 px-4 flex items-center gap-2 border-b border-border/50 bg-surface/40">
          <Terminal className="size-3.5 text-[var(--success)]" />
          <span className="text-xs font-mono text-muted-foreground">
            {selectedProject
              ? `loopdeck agent ~/${selectedProject.name}`
              : "loopdeck agent"}
          </span>
          {selectedProject && (
            <span className="text-[9px] font-mono text-muted-foreground/40 ml-auto truncate max-w-[40%]">
              {selectedProject.path}
            </span>
          )}
        </div>

        {/* Toolbar (only visible when a project is selected) */}
        {selectedProject && (
          <div className="flex items-center gap-2 px-4 py-2 border-b border-border/30 bg-surface/20 shrink-0">
            <button
              onClick={() => runStreamingTurn(undefined)}
              disabled={busy}
              className="inline-flex items-center gap-1.5 h-7 px-2.5 rounded text-[11px] font-medium bg-[var(--success)] text-black hover:opacity-90 transition disabled:opacity-50 disabled:cursor-not-allowed"
              title="Start / continue the next development loop"
            >
              {busy ? (
                <Loader2 className="size-3 animate-spin" />
              ) : (
                <Play className="size-3 fill-current" />
              )}
              Run next loop
            </button>
            <button
              onClick={handleReset}
              disabled={busy}
              className="inline-flex items-center gap-1.5 h-7 px-2.5 rounded text-[11px] font-medium bg-muted text-muted-foreground hover:bg-accent hover:text-foreground transition disabled:opacity-50 disabled:cursor-not-allowed"
              title="Archive the transcript and start a fresh conversation (type your first message below)"
            >
              <RotateCcw className="size-3" />
              New conversation
            </button>
            <span className="ml-auto text-[10px] text-muted-foreground">
              {turns.length} turn{turns.length !== 1 ? "s" : ""}
            </span>
          </div>
        )}

        {/* Error banner */}
        {error && (
          <div className="flex items-start gap-2 mx-4 mt-2 p-2.5 rounded bg-[color-mix(in_oklab,var(--destructive)_10%,transparent)] border border-[color-mix(in_oklab,var(--destructive)_25%,transparent)] text-destructive text-[11px] leading-relaxed shrink-0">
            <AlertTriangle className="size-3.5 shrink-0 mt-0.5" />
            <span className="break-words flex-1">{error}</span>
            <button
              onClick={() => setError(null)}
              className="shrink-0 text-[10px] font-medium opacity-60 hover:opacity-100 transition-opacity"
            >
              Dismiss
            </button>
          </div>
        )}

        {/* ── Terminal output area ── */}
        <div
          ref={outputRef}
          className="flex-1 min-h-0 overflow-y-auto px-4 py-3 font-mono text-sm leading-relaxed"
        >
          {/* Empty state */}
          {!selectedProject && (
            <div className="flex flex-col items-center justify-center h-full text-center">
              <Terminal size={40} className="text-muted-foreground/20 mb-4" />
              <p className="text-sm text-muted-foreground mb-1">
                Agent Runner
              </p>
              <p className="text-xs text-muted-foreground/60 max-w-sm leading-relaxed">
                Select a project from the left panel to open its agent terminal.
                You can start development loops, send follow-up messages, and
                monitor agent activity in real time.
              </p>
            </div>
          )}

          {/* Loading transcript */}
          {selectedProject && loadingTranscript && (
            <div className="flex items-center gap-2 text-muted-foreground py-4">
              <Loader2 className="size-3.5 animate-spin" />
              <span>Loading transcript...</span>
            </div>
          )}

          {/* Empty transcript */}
          {selectedProject && !loadingTranscript && turns.length === 0 && !isStreaming && !busy && (
            <div className="flex flex-col items-center justify-center h-full text-center">
              <Bot size={28} className="text-muted-foreground/20 mb-3" />
              <p className="text-xs text-muted-foreground/70 max-w-xs leading-relaxed">
                No conversation yet. Type a message below to start, or press{" "}
                <span className="text-[var(--success)] font-semibold">Run next loop</span>
                {" "}to spawn the agent for{" "}
                <span className="text-foreground/70">{selectedProject.name}</span>.
              </p>
            </div>
          )}

          {/* ── Pending AskUserQuestion (terminal-style) ──
              The agent parked mid-turn to ask the user a question. Rendered as
              a compact monospace prompt with numbered options; the user picks a
              number (or types "other: <text>") to answer. Mirrors the Chat card
              at lower visual fidelity, fitting the terminal aesthetic. */}
          {pendingQuestion && pendingQuestion.questions.length > 0 && (
            <RunnerQuestionPrompt
              questions={pendingQuestion.questions}
              busy={busy}
              onSubmit={handleAnswerQuestion}
            />
          )}

          {/* ── Terminal output lines ── */}
          {selectedProject &&
            !loadingTranscript &&
            turns.map((turn, i) => {
              const isUser = turn.role === "user";
              const isError = turn.is_error ?? false;
              return (
                <div key={i} className="mb-3">
                  {/* Prompt line / metadata */}
                  <div className="flex items-center gap-2 mb-0.5">
                    {isUser ? (
                      <span className="text-[var(--success)] font-semibold text-xs">
                        ❯ user
                      </span>
                    ) : (
                      <span
                        className={`font-semibold text-xs ${
                          isError ? "text-destructive" : "text-[var(--primary)]"
                        }`}
                      >
                        ❯ assistant
                      </span>
                    )}
                    <span className="text-[10px] text-muted-foreground/50">
                      {new Date(turn.ts).toLocaleTimeString([], {
                        hour: "2-digit",
                        minute: "2-digit",
                        second: "2-digit",
                      })}
                    </span>
                    {!isUser && turn.duration_ms != null && turn.duration_ms > 0 && (
                      <span className="text-[10px] text-muted-foreground/40">
                        {turn.duration_ms < 1000
                          ? `${turn.duration_ms}ms`
                          : `${(turn.duration_ms / 1000).toFixed(1)}s`}
                      </span>
                    )}
                    {!isUser && turn.usage && (
                      <span className="text-[10px] text-muted-foreground/40">
                        {fmtTokens(turn.usage.input_tokens)}→
                        {fmtTokens(turn.usage.output_tokens)} tok
                        {turn.usage.total_cost_usd > 0 &&
                          ` $${turn.usage.total_cost_usd.toFixed(3)}`}
                      </span>
                    )}
                  </div>

                  {/* Content */}
                  <p
                    className={`whitespace-pre-wrap break-words text-sm ${
                      isUser
                        ? "text-foreground/80 pl-4"
                        : isError
                          ? "text-destructive/80 pl-4"
                          : "text-foreground/90 pl-4"
                    }`}
                  >
                    {sanitise(turn.text)}
                  </p>
                </div>
              );
            })}

          {/* Streaming output */}
          {isStreaming && (
            <div className="mb-3">
              <div className="flex items-center gap-2 mb-0.5">
                <span className="text-[var(--primary)] font-semibold text-xs">
                  ❯ assistant
                </span>
                <span className="text-[10px] text-muted-foreground/50">
                  {new Date().toLocaleTimeString([], {
                    hour: "2-digit",
                    minute: "2-digit",
                    second: "2-digit",
                  })}
                </span>
                {streamingResult && (
                  <>
                    {streamingResult.duration_ms > 0 && (
                      <span className="text-[10px] text-muted-foreground/40">
                        {streamingResult.duration_ms < 1000
                          ? `${streamingResult.duration_ms}ms`
                          : `${(streamingResult.duration_ms / 1000).toFixed(1)}s`}
                      </span>
                    )}
                    {streamingResult.usage && (
                      <span className="text-[10px] text-muted-foreground/40">
                        {fmtTokens(streamingResult.usage.input_tokens)}→
                        {fmtTokens(streamingResult.usage.output_tokens)} tok
                        {streamingResult.usage.total_cost_usd > 0 &&
                          ` $${streamingResult.usage.total_cost_usd.toFixed(3)}`}
                      </span>
                    )}
                  </>
                )}
              </div>

              {/* Tool calls */}
              {streamingTools && streamingTools.length > 0 && (
                <div className="pl-4 mb-1 space-y-0.5">
                  {streamingTools.map((tool, i) => (
                    <div
                      key={i}
                      className="text-[11px] text-[var(--warning)]/80 font-mono"
                    >
                      ◈ {sanitise(describeTool(tool.name, tool.input))}
                    </div>
                  ))}
                </div>
              )}

              {/* Streaming text */}
              {streamingText && streamingText.length > 0 ? (
                <p className="text-sm whitespace-pre-wrap break-words text-foreground/90 pl-4">
                  {sanitise(streamingText)}
                  {!streamingResult && (
                    <span className="inline-block w-2 h-4 ml-0.5 bg-[var(--success)] animate-pulse align-middle" />
                  )}
                </p>
              ) : (
                <p className="text-sm text-muted-foreground/60 pl-4 italic">
                  {streamingResult ? "(empty response)" : "Waiting for response…"}
                </p>
              )}

              {/* Busy indicator */}
              {!streamingResult && (
                <div className="flex items-center gap-2 mt-1 pl-4 text-[10px] text-muted-foreground/60">
                  <Loader2 className="size-3 animate-spin" />
                  Agent is working…
                </div>
              )}
            </div>
          )}

          {/* Initial busy (no tokens yet) */}
          {busy && !isStreaming && (
            <div className="flex items-center gap-2 text-muted-foreground/60 text-sm pl-4">
              <Loader2 className="size-3.5 animate-spin" />
              <span>Agent is working…</span>
            </div>
          )}
        </div>

        {/* ── Composer (terminal prompt) ── */}
        {selectedProject && (
          <div className="shrink-0 px-4 py-3 border-t border-border/30 bg-surface/30">
            <div className="flex items-center gap-2">
              <span className="text-[var(--success)] font-mono text-sm font-semibold shrink-0">
                ❯
              </span>
              <input
                ref={inputRef}
                type="text"
                value={draft}
                onChange={(e) => setDraft(e.target.value)}
                onKeyDown={(e) => {
                  if (e.key === "Enter" && !e.shiftKey) {
                    e.preventDefault();
                    handleSend();
                  }
                }}
                placeholder={
                  pendingQuestion
                    ? "Answer the question above…"
                    : busy
                      ? "Agent is working…"
                      : turns.length === 0
                        ? "Start a new conversation… (Enter to send)"
                        : "Send a message… (Enter to send)"
                }
                disabled={busy || !selectedPath || pendingQuestion != null}
                className="flex-1 bg-transparent border-none outline-none text-sm font-mono text-foreground placeholder:text-muted-foreground/40 disabled:opacity-50"
                spellCheck={false}
                autoComplete="off"
              />
            </div>
          </div>
        )}
      </main>
    </div>
  );
}

/**
 * Terminal-styled AskUserQuestion prompt for the AgentRunner view.
 *
 * Mirrors the richer `AskUserQuestionCard` (used in Chat) but with the
 * monospace terminal aesthetic: numbered options, a `❯` prompt affordance,
 * and an inline "Other…" text override. Handles both single-select (radio)
 * and multi-select (checkbox) questions.
 */
function RunnerQuestionPrompt({
  questions,
  busy,
  onSubmit,
}: {
  questions: AskUserQuestionSpec[];
  busy: boolean;
  onSubmit: (answers: AskUserQuestionAnswers) => void;
}) {
  // Per-question selection state: single label, or a Set for multi.
  const [single, setSingle] = useState<Record<string, string | null>>({});
  const [multi, setMulti] = useState<Record<string, Set<string>>>({});
  const [otherText, setOtherText] = useState<Record<string, string>>({});
  const [otherActive, setOtherActive] = useState<Record<string, boolean>>({});

  function isAnswered(q: AskUserQuestionSpec): boolean {
    if (otherActive[q.question] && otherText[q.question]?.trim()) return true;
    if (q.multiSelect) return (multi[q.question]?.size ?? 0) > 0;
    return single[q.question] != null;
  }

  function toggleMulti(question: string, label: string) {
    setMulti((prev) => {
      const next = new Set(prev[question] ?? []);
      if (next.has(label)) next.delete(label);
      else next.add(label);
      return { ...prev, [question]: next };
    });
  }

  function buildAnswers(): AskUserQuestionAnswers {
    const out: AskUserQuestionAnswers = {};
    for (const q of questions) {
      const labels = q.multiSelect
        ? Array.from(multi[q.question] ?? [])
        : single[q.question] != null
          ? [single[q.question] as string]
          : [];
      const ot = otherActive[q.question] ? otherText[q.question] : undefined;
      out[q.question] = { labels, otherText: ot };
    }
    return out;
  }

  const allAnswered = questions.every(isAnswered);

  return (
    <div className="my-3 rounded border border-[var(--primary)]/30 bg-[var(--primary)]/5 p-3 font-mono">
      <div className="flex items-center gap-2 text-[11px] text-[var(--primary)] mb-3">
        <span className="inline-block size-1.5 rounded-full bg-[var(--primary)] animate-pulse" />
        <span>? agent question</span>
      </div>

      <div className="space-y-4">
        {questions.map((q, qi) => (
          <div key={qi} className="space-y-1.5">
            <div className="text-[10px] text-[var(--primary)]/80 uppercase tracking-wide">
              [{q.header}]
            </div>
            <p className="text-sm text-foreground/90">{q.question}</p>

            <div className="space-y-0.5 pl-1">
              {q.options.map((opt, oi) => {
                const selected = q.multiSelect
                  ? (multi[q.question]?.has(opt.label) ?? false)
                  : single[q.question] === opt.label;
                return (
                  <button
                    key={oi}
                    type="button"
                    disabled={busy}
                    onClick={() => {
                      if (q.multiSelect) {
                        toggleMulti(q.question, opt.label);
                      } else {
                        setSingle((p) => ({ ...p, [q.question]: opt.label }));
                        setOtherActive((p) => ({ ...p, [q.question]: false }));
                      }
                    }}
                    className={`w-full text-left flex items-start gap-2 px-2 py-1 rounded text-xs transition disabled:opacity-50 disabled:cursor-not-allowed ${
                      selected
                        ? "text-foreground bg-[var(--primary)]/10"
                        : "text-foreground/70 hover:text-foreground"
                    }`}
                  >
                    <span className="text-[var(--primary)] shrink-0">
                      {q.multiSelect ? (selected ? "[x]" : "[ ]") : selected ? "(*)" : "( )"}
                    </span>
                    <span className="break-all">
                      {opt.label}
                      {opt.description && (
                        <span className="text-muted-foreground/60"> — {opt.description}</span>
                      )}
                    </span>
                  </button>
                );
              })}

              <div className="flex items-center gap-2 pt-1">
                <button
                  type="button"
                  disabled={busy}
                  onClick={() =>
                    setOtherActive((p) => ({
                      ...p,
                      [q.question]: !(p[q.question] ?? false),
                    }))
                  }
                  className={`text-[11px] px-1.5 py-0.5 rounded transition disabled:opacity-50 disabled:cursor-not-allowed ${
                    otherActive[q.question]
                      ? "text-[var(--primary)] bg-[var(--primary)]/10"
                      : "text-muted-foreground/60 hover:text-foreground"
                  }`}
                >
                  {otherActive[q.question] ? "[x]" : "[ ]"} other…
                </button>
                {otherActive[q.question] && (
                  <input
                    type="text"
                    disabled={busy}
                    value={otherText[q.question] ?? ""}
                    onChange={(e) =>
                      setOtherText((p) => ({ ...p, [q.question]: e.target.value }))
                    }
                    onFocus={() => {
                      if (!q.multiSelect) setSingle((p) => ({ ...p, [q.question]: null }));
                    }}
                    placeholder="type your answer…"
                    className="flex-1 bg-transparent border-b border-border text-xs text-foreground placeholder:text-muted-foreground/40 focus:outline-none focus:border-[var(--primary)] disabled:opacity-50"
                  />
                )}
              </div>
            </div>
          </div>
        ))}
      </div>

      <div className="flex justify-end mt-3">
        <button
          type="button"
          disabled={busy || !allAnswered}
          onClick={() => onSubmit(buildAnswers())}
          className="text-[11px] px-3 py-1 rounded bg-[var(--primary)] text-black font-semibold hover:opacity-90 transition disabled:opacity-50 disabled:cursor-not-allowed"
        >
          submit ❯
        </button>
      </div>
    </div>
  );
}
