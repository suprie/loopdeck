import { useMemo } from "react";
import { Link } from "@tanstack/react-router";
import { Loader2 } from "lucide-react";
import {
  useActivityEvents,
  dateGroup,
  fmtTime,
  EVENT_ICON,
  EVENT_COLOR,
} from "../../hooks/useActivityEvents";

const MAX_ROWS = 6;

/**
 * Compact same-day activity timeline for the Dashboard, sitting alongside
 * `ProjectList`. Reuses `useActivityEvents` (shared with the full `/activity`
 * page) filtered to today's date bucket and capped to a handful of rows.
 */
export function TodayPanel() {
  const { events, loading } = useActivityEvents();

  const today = useMemo(
    () => events.filter((e) => dateGroup(e.timestamp) === "Today").slice(0, MAX_ROWS),
    [events],
  );

  return (
    <section aria-labelledby="today-title">
      <div className="mb-2.5 flex min-h-[28px] items-center">
        <h2 id="today-title" className="text-base font-semibold tracking-tight">
          Today
        </h2>
        <Link
          to="/activity"
          className="ml-auto text-xs text-muted-foreground transition-colors hover:text-foreground"
        >
          View all
        </Link>
      </div>

      <div className="min-h-[200px] rounded-2xl border border-border bg-card p-5">
        {loading && (
          <div className="flex items-center justify-center gap-2 py-10 text-xs text-muted-foreground">
            <Loader2 className="size-3.5 animate-spin" />
            Collecting activity…
          </div>
        )}

        {!loading && today.length === 0 && (
          <p className="py-10 text-center text-xs text-muted-foreground">
            No activity yet today.
          </p>
        )}

        {!loading && today.length > 0 && (
          <ol className="relative space-y-4 border-l border-border pl-6">
            {today.map((event, i) => (
              <li key={`${event.timestamp}-${i}`} className="relative">
                <span
                  className={`absolute -left-[27px] top-0.5 flex size-4 items-center justify-center rounded-full border border-border bg-background ${EVENT_COLOR[event.kind]}`}
                >
                  {EVENT_ICON[event.kind]}
                </span>
                <div className="text-sm font-medium leading-tight">{event.projectName}</div>
                <p className="mt-1 text-xs leading-relaxed text-muted-foreground line-clamp-2">
                  {event.summary}
                </p>
                <div className="mt-1 text-[11px] text-muted-foreground/70">
                  {fmtTime(event.timestamp)}
                </div>
              </li>
            ))}
          </ol>
        )}
      </div>
    </section>
  );
}
