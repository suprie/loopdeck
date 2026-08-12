import type { ProjectEntry, RunQueueStatus } from "../types";

/** Rail doors past this count collapse to pinned-only + one overflow door. */
export const RAIL_DOOR_LIMIT = 5;

/** First 2 characters of the project name, uppercased — the door's initials. */
export function doorInitials(name: string): string {
  return name.slice(0, 2).toUpperCase();
}

/** Most recently opened first. Never-opened projects sort last. */
export function sortByLastActive(projects: ProjectEntry[]): ProjectEntry[] {
  return [...projects].sort((a, b) => {
    const ta = a.last_opened ? Date.parse(a.last_opened) : -Infinity;
    const tb = b.last_opened ? Date.parse(b.last_opened) : -Infinity;
    return tb - ta;
  });
}

/**
 * Which projects render as rail doors, and whether a fixed overflow door
 * (back to the corridor) is needed.
 *
 * - `<= RAIL_DOOR_LIMIT` registered projects: every door shows, no overflow.
 * - `> RAIL_DOOR_LIMIT`: pinned projects only, plus overflow. If nobody has
 *   pinned anything yet (a fresh install crossing 5 projects), falls back to
 *   the 5 most recently active instead of an empty rail.
 */
export function selectRailDoors(projects: ProjectEntry[]): {
  doors: ProjectEntry[];
  overflow: boolean;
} {
  const sorted = sortByLastActive(projects);
  if (sorted.length <= RAIL_DOOR_LIMIT) {
    return { doors: sorted, overflow: false };
  }
  const pinned = sorted.filter((p) => p.pinned);
  return {
    doors: pinned.length > 0 ? pinned : sorted.slice(0, RAIL_DOOR_LIMIT),
    overflow: true,
  };
}

/**
 * Whether a project has an overnight run in flight or queued — the rail's
 * placeholder night-run signal (prd-night-run-surfaces owns the real one,
 * pending its detail-drawer spike). Derived from the same run-plan data
 * `RunQueuePanel` already polls, not a new backend concept.
 */
export function hasActiveOrQueuedRun(status: RunQueueStatus | undefined): boolean {
  if (!status) return false;
  if (status.active) return true;
  return status.plan?.phases.some((p) => p.status === "queued" || p.status === "running") ?? false;
}
