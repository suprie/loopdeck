import { useEffect, useState } from "react";
import * as api from "../lib/tauri";
import { hasActiveOrQueuedRun } from "../lib/rail";
import { hasUnresolvedParkedQuestions } from "../lib/nightRun";

const POLL_MS = 5000;

/** The two run-derived badges a project surface can carry. */
export interface RunIndicators {
  /** Overnight run active or queued — the rail door's moon badge. */
  night: boolean;
  /** Finished run with unresolved parked questions — the "morning report
   *  ready" badge (prd-night-run-surfaces Phase 3, item 2), which opens the
   *  drawer's morning-report variant. Stays lit until every parked question
   *  in the report is resolved, per the Phase 3 clarification. */
  morning: boolean;
}

const NONE: RunIndicators = { night: false, morning: false };

/**
 * Polls `getRunStatus` for each given project path, deriving both run
 * indicators from the one status call — no report fetch needed. Started as
 * the rail's night-run placeholder in `prd-rail-corridor-shell` Phase 1; the
 * detail-drawer spike (ADR-3) resolved the night flag as *the* night-run
 * representation, and prd-night-run-surfaces Phase 3 added the morning flag
 * on the same poll. Same cadence `RunQueuePanel` uses for a single project,
 * fanned out to every visible project.
 */
export function useRunIndicators(paths: string[]): Record<string, RunIndicators> {
  const key = paths.join("|");
  const [statuses, setStatuses] = useState<Record<string, RunIndicators>>({});

  useEffect(() => {
    if (!key) {
      setStatuses({});
      return;
    }
    let disposed = false;
    const load = async () => {
      const entries = await Promise.all(
        key.split("|").map(async (path) => {
          try {
            const status = await api.getRunStatus(path);
            const night = hasActiveOrQueuedRun(status);
            // Morning and night are mutually exclusive by construction: a
            // queued/running phase is the night signal, parked-only a
            // finished (or stalled-but-inactive) run's morning signal.
            const morning = !night && !!status.plan && hasUnresolvedParkedQuestions(status.plan);
            return [path, { night, morning }] as const;
          } catch {
            return [path, NONE] as const;
          }
        }),
      );
      if (!disposed) setStatuses(Object.fromEntries(entries));
    };
    load();
    const id = setInterval(load, POLL_MS);
    return () => {
      disposed = true;
      clearInterval(id);
    };
  }, [key]);

  return statuses;
}
