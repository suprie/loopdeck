import assert from "node:assert/strict";
import test from "node:test";
import {
  doorInitials,
  sortByLastActive,
  selectRailDoors,
  hasActiveOrQueuedRun,
  morningReportReady,
  RAIL_DOOR_LIMIT,
} from "../../src/lib/rail.ts";

function project(overrides = {}) {
  return {
    path: "/tmp/x",
    name: "x",
    description: "",
    status: "active",
    last_opened: null,
    created_at: "2026-01-01T00:00:00Z",
    last_commit_date: null,
    last_commit_message: null,
    last_modified: null,
    uncommitted: { files: 0, added: 0, deleted: 0 },
    run_state: "idle",
    pinned: false,
    next_steps_total: 0,
    next_steps_done: 0,
    ...overrides,
  };
}

test("doorInitials takes the first 2 chars, uppercased", () => {
  assert.equal(doorInitials("loopdeck"), "LO");
  assert.equal(doorInitials("my-cool-app"), "MY");
  assert.equal(doorInitials("a"), "A");
});

test("sortByLastActive orders most recently opened first, never-opened last", () => {
  const older = project({ path: "/a", last_opened: "2026-01-01T00:00:00Z" });
  const newer = project({ path: "/b", last_opened: "2026-02-01T00:00:00Z" });
  const never = project({ path: "/c", last_opened: null });
  const sorted = sortByLastActive([older, never, newer]);
  assert.deepEqual(
    sorted.map((p) => p.path),
    ["/b", "/a", "/c"],
  );
});

test("selectRailDoors shows every project with no overflow at or under the limit", () => {
  const projects = Array.from({ length: RAIL_DOOR_LIMIT }, (_, i) => project({ path: `/p${i}` }));
  const { doors, overflow } = selectRailDoors(projects);
  assert.equal(doors.length, RAIL_DOOR_LIMIT);
  assert.equal(overflow, false);
});

test("selectRailDoors falls back to the 5 most recently active when nobody has pinned yet", () => {
  const projects = Array.from({ length: RAIL_DOOR_LIMIT + 3 }, (_, i) =>
    project({ path: `/p${i}`, last_opened: `2026-01-${String(i + 1).padStart(2, "0")}T00:00:00Z` }),
  );
  const { doors, overflow } = selectRailDoors(projects);
  assert.equal(overflow, true);
  assert.equal(doors.length, RAIL_DOOR_LIMIT);
  // Most recent 5 = the last 5 pushed (highest day-of-month).
  assert.deepEqual(
    doors.map((p) => p.path),
    ["/p7", "/p6", "/p5", "/p4", "/p3"],
  );
});

test("selectRailDoors shows pinned-only plus overflow once pins exist", () => {
  const projects = Array.from({ length: RAIL_DOOR_LIMIT + 3 }, (_, i) => project({ path: `/p${i}` }));
  projects[0].pinned = true;
  projects[2].pinned = true;
  const { doors, overflow } = selectRailDoors(projects);
  assert.equal(overflow, true);
  assert.deepEqual(
    doors.map((p) => p.path).sort(),
    ["/p0", "/p2"],
  );
});

test("hasActiveOrQueuedRun is false with no plan", () => {
  assert.equal(hasActiveOrQueuedRun(undefined), false);
  assert.equal(hasActiveOrQueuedRun({ plan: null, active: false }), false);
});

test("hasActiveOrQueuedRun is true when active or a phase is queued/running", () => {
  assert.equal(hasActiveOrQueuedRun({ plan: null, active: true }), true);
  assert.equal(
    hasActiveOrQueuedRun({
      plan: { phases: [{ status: "completed" }, { status: "queued" }] },
      active: false,
    }),
    true,
  );
  assert.equal(
    hasActiveOrQueuedRun({
      plan: { phases: [{ status: "completed" }, { status: "failed" }] },
      active: false,
    }),
    false,
  );
});

test("morningReportReady needs a plan with at least one terminal phase", () => {
  assert.equal(morningReportReady(undefined), false);
  assert.equal(morningReportReady({ plan: null, active: false }), false);
  // Freshly authored plan, never started — not a report.
  assert.equal(
    morningReportReady({ plan: { phases: [{ status: "queued" }] }, active: false }),
    false,
  );
  // Finished run: terminal phases, nothing active/queued.
  assert.equal(
    morningReportReady({ plan: { phases: [{ status: "completed" }, { status: "parked" }] }, active: false }),
    true,
  );
});

test("morningReportReady is false while the night variant owns the state", () => {
  // Active run.
  assert.equal(
    morningReportReady({ plan: { phases: [{ status: "completed" }] }, active: true }),
    false,
  );
  // Halt-on-stall: parked phase but queued phases remain — night variant's
  // parked inbox covers it, not the report.
  assert.equal(
    morningReportReady({
      plan: { phases: [{ status: "parked" }, { status: "queued" }] },
      active: false,
    }),
    false,
  );
});
