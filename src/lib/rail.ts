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
 * Whether a project has an overnight run in flight or queued — the night-run
 * signal shared by the rail doors' moon badge and the drawer's night-variant
 * selection (`ProjectDrawer.tsx`). The detail-drawer spike (ADR-3,
 * `docs/epics/selasar-revamp/README.md`) resolved "night run" as exactly this
 * derived run-plan flag — no new `RunState` variant — so what started as the
 * rail's placeholder is now the confirmed representation. Derived from the
 * same run-plan data `RunQueuePanel` already polls, not a new backend concept.
 */
export function hasActiveOrQueuedRun(status: RunQueueStatus | undefined): boolean {
  if (!status) return false;
  if (status.active) return true;
  return status.plan?.phases.some((p) => p.status === "queued" || p.status === "running") ?? false;
}

/**
 * Whether the project's latest run plan holds a finished-but-unreviewed
 * morning report — the rail doors' "morning report ready" badge and the
 * drawer's report-slot signal (prd-night-run-surfaces Phase 3). True when a
 * plan exists, nothing is active/queued (that state belongs to the night
 * variant — a halt-on-stall plan with queued phases left shows its parked
 * inbox there instead), and at least one phase reached a terminal state: a
 * freshly authored plan (all `queued`, interviews pending) is not a report.
 */
export function morningReportReady(status: RunQueueStatus | undefined): boolean {
  if (!status?.plan) return false;
  if (hasActiveOrQueuedRun(status)) return false;
  return status.plan.phases.some((p) => p.status !== "queued");
}
