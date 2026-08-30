import { useEffect, useState } from "react";
import * as api from "../lib/tauri";
import { hasActiveOrQueuedRun, morningReportReady } from "../lib/rail";
import type { RunQueueStatus } from "../types";

const POLL_MS = 5000;

export interface DoorRunBadges {
  /** Moon badge — overnight run active or queued. */
  night: boolean;
  /** Sun badge — finished run with an unreviewed morning report
   *  (prd-night-run-surfaces Phase 3 item 2). Mutually exclusive with
   *  `night` by derivation: `morningReportReady` requires nothing
   *  active/queued. */
  reportReady: boolean;
  /** The finished plan the report badge refers to — the caller compares it
   *  against `appStore.morningReportSeen` to implement "clear once opened"
   *  (a new plan id re-arms the badge). Undefined when not reportReady. */
  reportPlanId?: string;
}

function badges(status: RunQueueStatus): DoorRunBadges {
  const reportReady = morningReportReady(status);
  return {
    night: hasActiveOrQueuedRun(status),
    reportReady,
    reportPlanId: reportReady ? status.plan?.id : undefined,
  };
}

/**
 * Polls `getRunStatus` for each given project path, returning each door's
 * run badges. Started as a placeholder in `prd-rail-corridor-shell` Phase 1;
 * the detail-drawer spike (ADR-3) later resolved this derived flag as *the*
 * night-run representation, so the moon badge is the confirmed indicator.
 * Phase 3 added `reportReady` for the "morning report ready" sun badge. Same
 * poll cadence `RunQueuePanel` uses for a single project, fanned out to
 * every visible door.
 */
export function useNightRunStatuses(paths: string[]): Record<string, DoorRunBadges> {
  const key = paths.join("|");
  const [statuses, setStatuses] = useState<Record<string, DoorRunBadges>>({});

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
            return [path, badges(status)] as const;
          } catch {
            return [path, { night: false, reportReady: false } as DoorRunBadges] as const;
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
