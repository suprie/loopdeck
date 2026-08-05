import { clsx, type ClassValue } from "clsx";
import { twMerge } from "tailwind-merge";
import type { Epic } from "../types";

/** Tailwind-aware className combiner. */
export function cn(...inputs: ClassValue[]) {
  return twMerge(clsx(inputs));
}

/** Kebab-case slug from a title: lowercase, trim, non-alphanumeric runs → `-`.
 *  `"My Cool Feature!"` → `"my-cool-feature"`. */
export function slugify(title: string): string {
  return title
    .toLowerCase()
    .trim()
    .replace(/[^a-z0-9]+/g, "-")
    .replace(/^-+|-+$/g, "");
}

// ── Epic ordering ────────────────────────────────────────────────────────────

/** Primary sort rank: active work first, completed last. */
export const EPIC_STATUS_RANK: Record<string, number> = {
  in_progress: 0,
  proposed: 1,
  abandoned: 2,
  completed: 3,
};

export const EPIC_STATUS_LABEL: Record<string, string> = {
  in_progress: "In Progress",
  proposed: "Proposed",
  abandoned: "Abandoned",
  completed: "Completed",
};

/**
 * Parses a "0.5.0"-shaped milestone into a comparable tuple. Non-numeric or
 * empty milestones sort after every numbered one.
 */
function milestoneRank(milestone: string): [number, number, number] {
  const parts = milestone.split(".").map((p) => Number.parseInt(p, 10));
  if (parts.length === 0 || parts.some((p) => Number.isNaN(p))) {
    return [Number.POSITIVE_INFINITY, Number.POSITIVE_INFINITY, Number.POSITIVE_INFINITY];
  }
  return [parts[0] ?? 0, parts[1] ?? 0, parts[2] ?? 0];
}

function compareMilestones(a: string, b: string): number {
  const ra = milestoneRank(a);
  const rb = milestoneRank(b);
  return ra[0] - rb[0] || ra[1] - rb[1] || ra[2] - rb[2];
}

/**
 * Sorts epics by status (active work first, completed last), then milestone
 * ascending (0.4.0 before 0.5.0), then slug. Shared by every view that lists
 * epics so ordering stays consistent across the app.
 */
export function sortEpics<T extends Pick<Epic, "status" | "milestone" | "slug">>(
  epics: T[],
): T[] {
  return [...epics].sort(
    (a, b) =>
      (EPIC_STATUS_RANK[a.status] ?? 1) - (EPIC_STATUS_RANK[b.status] ?? 1) ||
      compareMilestones(a.milestone, b.milestone) ||
      a.slug.localeCompare(b.slug),
  );
}
