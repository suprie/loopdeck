import { useState, useEffect } from "react";
import {
  Activity,
  Loader2,
  User,
  Bot,
  AlertTriangle,
  Lightbulb,
  Play,
  CheckCircle2,
  Clock,
} from "lucide-react";
import type { ConversationTurn, Decision, Loop } from "../../types";
import * as api from "../../lib/tauri";
import { useAppStore } from "../../store/appStore";

// ── Types ────────────────────────────────────────────────────────────────────

/** A unified event from any project data source, sorted into one timeline. */
interface ActivityEvent {
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

// ── Helpers ──────────────────────────────────────────────────────────────────

/** Strip ANSI escape codes. */
function sanitise(text: string): string {
  return text.replace(/\x1b\[[0-9;]*[a-zA-Z]/g, "");
}

/** Truncate text for feed display. */
function truncate(text: string, max: number): string {
  const cleaned = sanitise(text).replace(/\s+/g, " ").trim();
  if (cleaned.length <= max) return cleaned;
  return cleaned.slice(0, max) + "…";
}

/**
 * Build a synthetic ISO timestamp from a date-only string.
 * Decisions use "2026-06-22" format — we treat them as midnight UTC.
 */
function dateToTs(date: string): string {
  // If it already has a time component, return as-is.
  if (date.includes("T")) return date;
  return `${date}T00:00:00.000Z`;
}

/** Group a date string into a human-readable bucket. */
function dateGroup(iso: string): string {
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
function fmtTime(iso: string): string {
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

// ── Icons per event kind ─────────────────────────────────────────────────────

const EVENT_ICON: Record<ActivityEvent["kind"], React.ReactNode> = {
  turn_user: <User className="size-3.5" />,
  turn_assistant: <Bot className="size-3.5" />,
  turn_error: <AlertTriangle className="size-3.5" />,
  decision: <Lightbulb className="size-3.5" />,
  loop_started: <Play className="size-3.5" />,
  loop_completed: <CheckCircle2 className="size-3.5" />,
};

const EVENT_COLOR: Record<ActivityEvent["kind"], string> = {
  turn_user: "text-muted-foreground",
  turn_assistant: "text-[var(--primary)]",
  turn_error: "text-destructive",
  decision: "text-[var(--warning)]",
  loop_started: "text-[var(--success)]",
  loop_completed: "text-[var(--success)]",
};

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

// ── Component ────────────────────────────────────────────────────────────────

export function ActivityFeed() {
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
        all.sort(
          (a, b) =>
            new Date(b.timestamp).getTime() - new Date(a.timestamp).getTime(),
        );
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

  // ── Group by date ──────────────────────────────────────────────────────

  const grouped = groupByDate(events);
  const hasData = events.length > 0;

  // ── Render ─────────────────────────────────────────────────────────────

  return (
    <div className="flex-1 flex flex-col min-h-0">
      {/* Header */}
      <div className="h-14 shrink-0 px-6 border-b border-border flex items-center justify-between bg-background/80 backdrop-blur sticky top-0 z-10">
        <div>
          <h1 className="text-sm font-semibold tracking-tight">Activity Feed</h1>
          <p className="text-xs text-muted-foreground">
            Chronological events across all projects
          </p>
        </div>
        {!loading && hasData && (
          <span className="text-[11px] text-muted-foreground">
            {events.length} event{events.length !== 1 ? "s" : ""}
          </span>
        )}
      </div>

      {/* Body */}
      <div className="flex-1 min-h-0 overflow-y-auto">
        {/* Loading spinner */}
        {loading && (
          <div className="flex flex-col items-center justify-center py-20 gap-4 text-muted-foreground">
            <Loader2 className="size-8 animate-spin" />
            <span className="text-sm">Collecting activity…</span>
          </div>
        )}

        {/* Error */}
        {error && !loading && (
          <div className="flex items-center justify-center py-20">
            <div className="flex flex-col items-center gap-3 text-center max-w-sm">
              <AlertTriangle className="size-8 text-destructive/60" />
              <p className="text-sm text-destructive">{error}</p>
            </div>
          </div>
        )}

        {/* Empty state */}
        {!loading && !error && !hasData && (
          <div className="flex flex-col items-center justify-center py-20 text-center">
            <Activity size={40} className="text-muted-foreground/20 mb-4" />
            <h3 className="text-sm font-semibold text-foreground mb-1.5">
              No activity yet
            </h3>
            <p className="text-xs text-muted-foreground max-w-xs leading-relaxed">
              Activity from agent conversations, decisions, and development
              loops will appear here. Start an agent conversation or import a
              project to get started.
            </p>
          </div>
        )}

        {/* Timeline */}
        {!loading && !error && hasData && (
          <div className="max-w-2xl mx-auto px-6 py-4">
            {grouped.map(([dateLabel, groupEvents]) => (
              <section key={dateLabel} className="mb-6">
                {/* Date heading */}
                <div className="flex items-center gap-3 mb-3 sticky top-0 pt-1 pb-2 bg-background/90 backdrop-blur z-[5]">
                  <Clock className="size-4 text-muted-foreground" />
                  <h2 className="text-xs font-semibold text-muted-foreground uppercase tracking-wider">
                    {dateLabel}
                  </h2>
                  <div className="flex-1 h-px bg-border" />
                  <span className="text-[10px] text-muted-foreground/60">
                    {groupEvents.length}
                  </span>
                </div>

                {/* Event rows */}
                <div className="space-y-0.5">
                  {groupEvents.map((event, i) => (
                    <EventRow key={`${event.timestamp}-${i}`} event={event} />
                  ))}
                </div>
              </section>
            ))}
          </div>
        )}
      </div>
    </div>
  );
}

// ── Event row ────────────────────────────────────────────────────────────────

function EventRow({ event }: { event: ActivityEvent }) {
  const colorClass = EVENT_COLOR[event.kind];
  const [expanded, setExpanded] = useState(false);
  const hasDetail = event.detail && event.detail.length > 0;

  return (
    <div className="group">
      <button
        onClick={() => hasDetail && setExpanded(!expanded)}
        className={`w-full text-left flex items-start gap-3 px-3 py-2 rounded-md transition-colors hover:bg-accent/30 ${
          expanded ? "bg-accent/20" : ""
        } ${!hasDetail ? "cursor-default" : ""}`}
      >
        {/* Icon — coloured per kind */}
        <div
          className={`size-7 shrink-0 rounded-full grid place-items-center bg-[color-mix(in_oklab,var(--primary)_10%,transparent)] ${colorClass}`}
        >
          {EVENT_ICON[event.kind]}
        </div>

        {/* Content */}
        <div className="min-w-0 flex-1">
          <div className="flex items-baseline gap-2 flex-wrap">
            <span className="text-xs font-medium text-foreground truncate">
              {event.projectName}
            </span>
            <span className="text-[10px] text-muted-foreground/60 shrink-0">
              {fmtTime(event.timestamp)}
            </span>
          </div>
          <p className="text-xs text-muted-foreground leading-relaxed mt-0.5 line-clamp-2">
            {event.summary}
          </p>

          {/* Expanded detail */}
          {expanded && hasDetail && (
            <p className="mt-1.5 text-xs text-muted-foreground/80 leading-relaxed whitespace-pre-wrap break-words bg-muted/50 rounded-md p-2.5 border border-border/50 max-h-48 overflow-y-auto font-mono">
              {sanitise(event.detail!)}
            </p>
          )}
        </div>

        {/* Expand indicator */}
        {hasDetail && (
          <span className="shrink-0 text-[10px] text-muted-foreground/40 mt-1.5 opacity-0 group-hover:opacity-100 transition-opacity">
            {expanded ? "−" : "+"}
          </span>
        )}
      </button>
    </div>
  );
}

// ── Date grouping ────────────────────────────────────────────────────────────

/** Group sorted events by date bucket, preserving descending order within each group. */
function groupByDate(events: ActivityEvent[]): [string, ActivityEvent[]][] {
  const map = new Map<string, ActivityEvent[]>();
  for (const e of events) {
    const key = dateGroup(e.timestamp);
    const bucket = map.get(key);
    if (bucket) {
      bucket.push(e);
    } else {
      map.set(key, [e]);
    }
  }
  return [...map.entries()];
}
