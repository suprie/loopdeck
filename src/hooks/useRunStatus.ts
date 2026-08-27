import { useEffect, useState } from "react";
import * as api from "../lib/tauri";
import type { RunQueueStatus } from "../types";

const POLL_MS = 5000;

/**
 * Polls `getRunStatus` for one project — the drawer's night-variant signal
 * (`prd-night-run-surfaces` Phase 1). Same cadence and IPC `RunQueuePanel`
 * already uses; the drawer needs the full status (plan + `active`), not just
 * the boolean `useNightRunStatuses` distills for the rail doors.
 *
 * Returns null before the first load (or when no project is selected); a
 * failed poll keeps the last known status — a transient IPC error shouldn't
 * flip the drawer back to the standard variant mid-run.
 */
export function useRunStatus(projectPath: string | null): RunQueueStatus | null {
  const [status, setStatus] = useState<RunQueueStatus | null>(null);

  useEffect(() => {
    if (!projectPath) {
      setStatus(null);
      return;
    }
    let disposed = false;
    const load = async () => {
      try {
        const s = await api.getRunStatus(projectPath);
        if (!disposed) setStatus(s);
      } catch (err) {
        console.warn("getRunStatus failed", err);
      }
    };
    load();
    const id = setInterval(load, POLL_MS);
    return () => {
      disposed = true;
      clearInterval(id);
    };
  }, [projectPath]);

  return status;
}
