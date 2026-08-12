import { useEffect, useState } from "react";
import * as api from "../lib/tauri";
import { hasActiveOrQueuedRun } from "../lib/rail";

const POLL_MS = 5000;

/**
 * Polls `getRunStatus` for each given project path, returning which ones
 * have an active or queued overnight run — the rail's night-run door badge
 * (`prd-rail-corridor-shell` Phase 1). Same poll cadence `RunQueuePanel`
 * already uses for a single project, fanned out to every visible door.
 */
export function useNightRunStatuses(paths: string[]): Record<string, boolean> {
  const key = paths.join("|");
  const [statuses, setStatuses] = useState<Record<string, boolean>>({});

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
            return [path, hasActiveOrQueuedRun(status)] as const;
          } catch {
            return [path, false] as const;
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
