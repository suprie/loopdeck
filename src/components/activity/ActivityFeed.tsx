import { useState } from "react";
import { Activity, Loader2, AlertTriangle } from "lucide-react";
import {
  useActivityEvents,
  sanitise,
  dateGroup,
  fmtTime,
  EVENT_ICON,
  EVENT_COLOR,
  type ActivityEvent,
} from "../../hooks/useActivityEvents";
import { PageHeader } from "../layout/AppShell";

// ── Component ────────────────────────────────────────────────────────────────

export function ActivityFeed() {
  const { events, loading, error } = useActivityEvents();

  // ── Group by date ──────────────────────────────────────────────────────

  const grouped = groupByDate(events);
  const hasData = events.length > 0;

  // ── Render ─────────────────────────────────────────────────────────────

  return (
    <div className="flex-1 flex flex-col min-h-0">
      <PageHeader
        title="Activity"
        subtitle="Recent events across all projects"
        actions={
          !loading && hasData ? (
            <span className="text-[11px] text-muted-foreground">
              {events.length} event{events.length !== 1 ? "s" : ""}
            </span>
          ) : undefined
        }
      />

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

        {/* Timeline — left-rail with dot markers (clone anatomy) */}
        {!loading && !error && hasData && (
          <div className="mx-auto w-full max-w-3xl px-8 py-8">
            {grouped.map(([dateLabel, groupEvents]) => (
              <section key={dateLabel} className="mb-8 last:mb-0">
                {/* Date heading */}
                <div className="mb-4 flex items-center gap-3">
                  <h2 className="text-[10px] font-semibold uppercase tracking-wider text-muted-foreground">
                    {dateLabel}
                  </h2>
                  <div className="h-px flex-1 bg-border" />
                  <span className="text-[10px] text-muted-foreground/60">
                    {groupEvents.length}
                  </span>
                </div>

                {/* Event rows on a left rail */}
                <ol className="relative space-y-3 border-l border-border pl-6">
                  {groupEvents.map((event, i) => (
                    <li key={`${event.timestamp}-${i}`} className="relative">
                      <EventRow event={event} />
                    </li>
                  ))}
                </ol>
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
  const Icon = EVENT_ICON[event.kind];

  return (
    <div className="group">
      {/* Dot marker on the rail */}
      <span
        className={`absolute -left-[27px] top-3 flex size-4 items-center justify-center rounded-full border border-border bg-background ${colorClass}`}
      >
        {Icon}
      </span>

      <button
        onClick={() => hasDetail && setExpanded(!expanded)}
        className={`w-full rounded-lg border border-border bg-card p-3 text-left shadow-[var(--shadow-sm)] transition-colors hover:bg-accent/30 ${
          expanded ? "ring-1 ring-border" : ""
        } ${!hasDetail ? "cursor-default" : ""}`}
      >
        <div className="flex items-start gap-3">
          {/* Content */}
          <div className="min-w-0 flex-1">
            <div className="flex items-center justify-between text-xs">
              <span className="truncate font-semibold text-foreground">
                {event.projectName}
              </span>
              <span className="text-muted-foreground">
                {fmtTime(event.timestamp)}
              </span>
            </div>
            <p className="mt-1 text-xs leading-relaxed text-muted-foreground line-clamp-2">
              {event.summary}
            </p>

            {/* Expanded detail */}
            {expanded && hasDetail && (
              <p className="mt-2 max-h-48 overflow-y-auto whitespace-pre-wrap break-words rounded-md border border-border/50 bg-muted/40 p-2.5 font-mono text-[11px] leading-relaxed text-muted-foreground/90">
                {sanitise(event.detail!)}
              </p>
            )}
          </div>

          {/* Expand indicator */}
          {hasDetail && (
            <span className="mt-1 shrink-0 text-[10px] text-muted-foreground/40 opacity-0 transition-opacity group-hover:opacity-100">
              {expanded ? "−" : "+"}
            </span>
          )}
        </div>
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
