import { useEffect, useState } from "react";
import { User, Bot, AlertTriangle, Lightbulb, Play, CheckCircle2 } from "lucide-react";
import type { ConversationTurn, Decision, Loop } from "../types";
import * as api from "../lib/tauri";
import { useAppStore } from "../store/appStore";

/**
 * A unified event from any project data source, sorted into one timeline.
 *
 * Shared by `ActivityFeed` (the full `/activity` page) and the Dashboard's
 * "Today" panel, which both fetch the same per-project turns/decisions/loops
 * and just render/filter the resulting list differently.
 */
export interface ActivityEvent {
  /** ISO-8601 timestamp for sorting (may be date-only for decisions/loops). */
  timestamp: string;
  /** Display label for the project this event belongs to. */
  projectName: string;
  /** Project filesystem path (for contextual linking, future use). */
  projectPath: string;
  /** Event category drives the icon and colour. */
  kind:
    | "turn_user"
    | "turn_assistant"
    | "turn_error"
    | "decision"
    | "loop_started"
    | "loop_completed";
  /** One-line summary shown in the feed. */
  summary: string;
  /** Optional detail body (truncated turn text, decision context, etc.). */
  detail: string | null;
}

// ── Icons + colours per event kind ──────────────────────────────────────────

export const EVENT_ICON: Record<ActivityEvent["kind"], React.ReactNode> = {
  turn_user: <User className="size-3.5" />,
  turn_assistant: <Bot className="size-3.5" />,
  turn_error: <AlertTriangle className="size-3.5" />,
  decision: <Lightbulb className="size-3.5" />,
  loop_started: <Play className="size-3.5" />,
  loop_completed: <CheckCircle2 className="size-3.5" />,
};

export const EVENT_COLOR: Record<ActivityEvent["kind"], string> = {
  turn_user: "text-muted-foreground",
  turn_assistant: "text-[var(--primary)]",
  turn_error: "text-destructive",
  decision: "text-[var(--warning)]",
  loop_started: "text-[var(--success)]",
  loop_completed: "text-[var(--success)]",
};

// ── Formatting helpers ───────────────────────────────────────────────────────

/** Strip ANSI escape codes. */
export function sanitise(text: string): string {
  return text.replace(/\x1b\[[0-9;]*[a-zA-Z]/g, "");
}

/** Truncate text for feed display. */
export function truncate(text: string, max: number): string {
  const cleaned = sanitise(text).replace(/\s+/g, " ").trim();
  if (cleaned.length <= max) return cleaned;
  return cleaned.slice(0, max) + "…";
}

/**
 * Build a synthetic ISO timestamp from a date-only string.
 * Decisions use "2026-06-22" format — we treat them as midnight UTC.
 */
export function dateToTs(date: string): string {
  // If it already has a time component, return as-is.
  if (date.includes("T")) return date;
  // Build local midnight (not UTC midnight) from the Y/M/D parts. `dateGroup`
  // and `fmtTime` both read the resulting Date back out via *local* getters —
  // anchoring to UTC midnight instead would round-trip to the previous
  // calendar day in any timezone west of UTC (e.g. "2026-07-27" would land in
  // the "Yesterday" bucket for anyone in America/Los_Angeles).
  const [y, m, d] = date.split("-").map(Number);
  return new Date(y, m - 1, d).toISOString();
}

/** Group a date string into a human-readable bucket. */
export function dateGroup(iso: string): string {
  const d = new Date(iso);
  const now = new Date();
  const today = new Date(now.getFullYear(), now.getMonth(), now.getDate());
  const yesterday = new Date(today.getTime() - 86_400_000);
  const eventDay = new Date(d.getFullYear(), d.getMonth(), d.getDate());

  if (eventDay.getTime() === today.getTime()) return "Today";
  if (eventDay.getTime() === yesterday.getTime()) return "Yesterday";
  return d.toLocaleDateString("en-US", {
    weekday: "long",
    month: "long",
    day: "numeric",
    year: d.getFullYear() !== now.getFullYear() ? "numeric" : undefined,
  });
}

/** Format a time-of-day string from an ISO timestamp. */
export function fmtTime(iso: string): string {
  return new Date(iso).toLocaleTimeString("en-US", {
    hour: "numeric",
    minute: "2-digit",
    hour12: true,
  });
}

/** Duration string for agent turns. */
function fmtDuration(ms: number): string {
  if (ms < 1000) return `${ms}ms`;
  if (ms < 60_000) return `${(ms / 1000).toFixed(1)}s`;
  const mins = Math.floor(ms / 60_000);
  const secs = Math.round((ms % 60_000) / 1000);
  return `${mins}m ${secs}s`;
}

/** Format token count compactly. */
function fmtTokens(n: number): string {
  if (n >= 1_000_000) return `${(n / 1_000_000).toFixed(1)}M`;
  if (n >= 1_000) return `${(n / 1_000).toFixed(0)}k`;
  return String(n);
}

// ── Event collectors ─────────────────────────────────────────────────────────

/** Extract conversation turns into ActivityEvents. */
function turnsToEvents(
  turns: ConversationTurn[],
  projectName: string,
  projectPath: string,
): ActivityEvent[] {
  return turns.map((t) => {
    const isUser = t.role === "user";
    const isError = t.is_error ?? false;
    const kind: ActivityEvent["kind"] = isUser
      ? "turn_user"
      : isError
        ? "turn_error"
        : "turn_assistant";

    let summary: string;
    if (isUser) {
      summary = `You sent a message: "${truncate(t.text, 80)}"`;
    } else if (isError) {
      summary = `Agent error: ${truncate(t.text, 80)}`;
    } else {
      let meta = "";
      if (t.duration_ms) meta += ` · ${fmtDuration(t.duration_ms)}`;
      if (t.usage)
        meta += ` · ${fmtTokens(t.usage.input_tokens)}→${fmtTokens(t.usage.output_tokens)} tok`;
      summary = `Agent response${meta}`;
    }

    return {
      timestamp: t.ts,
      projectName,
      projectPath,
      kind,
      summary,
      detail: isUser ? t.text : t.text || null,
    };
  });
}

/** Extract decisions into ActivityEvents. */
function decisionsToEvents(
  decisions: Decision[],
  projectName: string,
  projectPath: string,
): ActivityEvent[] {
  return decisions.map((d) => ({
    timestamp: dateToTs(d.date),
    projectName,
    projectPath,
    kind: "decision" as const,
    summary: `Decision ${d.status}: ${d.title}`,
    detail: d.context || null,
  }));
}

/** Extract loop history into ActivityEvents. */
function loopsToEvents(
  loops: Loop[],
  projectName: string,
  projectPath: string,
): ActivityEvent[] {
  return loops
    .filter((l) => l.status === "completed")
    .map((l) => ({
      timestamp: dateToTs(l.completed ?? l.started),
      projectName,
      projectPath,
      kind: "loop_completed" as const,
      summary: `Loop completed: ${l.goal}`,
      detail: `Started ${l.started}${l.completed ? `, completed ${l.completed}` : ""}`,
    }));
}

// ── Hook ─────────────────────────────────────────────────────────────────────

/**
 * Collects a unified, newest-first activity timeline across every registered
 * project — conversation turns, decisions, and completed loops. Shared by the
 * full `/activity` page (`ActivityFeed`) and the Dashboard's "Today" panel.
 */
export function useActivityEvents(): {
  events: ActivityEvent[];
  loading: boolean;
  error: string | null;
} {
  const projects = useAppStore((s) => s.projects);
  const [events, setEvents] = useState<ActivityEvent[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let cancelled = false;

    async function collect() {
      setLoading(true);
      setError(null);

      const all: ActivityEvent[] = [];

      for (const p of projects) {
        if (cancelled) break;

        // Fetch conversation turns.
        try {
          const turns = await api.agentGetConversation(p.path);
          all.push(...turnsToEvents(turns, p.name, p.path));
        } catch {
          // No transcript — skip silently.
        }

        // Fetch decisions.
        try {
          const decisions = await api.getDecisions(p.path);
          all.push(...decisionsToEvents(decisions, p.name, p.path));
        } catch {
          // No decisions file.
        }

        // Fetch loops.
        try {
          const loopStatus = await api.getLoops(p.path);
          const loops: Loop[] = [];
          if (loopStatus.current) {
            loops.push({ ...loopStatus.current, status: loopStatus.current.status });
          }
          loops.push(...loopStatus.history);
          all.push(...loopsToEvents(loops, p.name, p.path));
        } catch {
          // No loops file.
        }
      }

      if (!cancelled) {
        // Sort descending by timestamp (newest first).
        all.sort((a, b) => new Date(b.timestamp).getTime() - new Date(a.timestamp).getTime());
        setEvents(all);
        setLoading(false);
      }
    }

    if (projects.length > 0) {
      collect();
    } else {
      setLoading(false);
      setEvents([]);
    }

    return () => {
      cancelled = true;
    };
  }, [projects]);

  return { events, loading, error };
}
