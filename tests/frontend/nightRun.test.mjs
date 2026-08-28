import assert from "node:assert/strict";
import test from "node:test";
import {
  budgetGauges,
  gaugePercent,
  formatDuration,
  formatTokens,
  parseParkedQuestions,
  parkedInbox,
  shouldAutoSelectNightVariant,
  hasQueueablePhases,
  dependencyLabel,
  hasUnresolvedParkedQuestions,
  isUnresolvedParkedQuestion,
  DEFAULT_RUN_PHASE_TOKEN_CAP,
  DEFAULT_RUN_PHASE_WALL_CLOCK_SECS,
  DEFAULT_RUN_TOTAL_WALL_CLOCK_SECS,
} from "../../src/lib/nightRun.ts";

// Minimal real-shape RunPlan (mirrors runplan.rs / types/index.ts).
function plan(overrides = {}) {
  return {
    id: "run-1",
    project: "/tmp/x",
    created: "2026-08-26T00:00:00Z",
    consent: { draft_pr_authorized: true },
    budgets: {},
    environment: { worktree_kept: true },
    wall_clock_secs: 0,
    stall_policy: "continue_independent",
    phases: [],
    ...overrides,
  };
}

function phase(overrides = {}) {
  return {
    execution_id: "x/loop-1",
    status: "completed",
    interview: [],
    interview_status: "skipped",
    depends_on: [],
    token_usage: 0,
    wall_clock_secs: 0,
    ...overrides,
  };
}

test("defaults mirror limits::DEFAULT_RUN_*", () => {
  assert.equal(DEFAULT_RUN_PHASE_TOKEN_CAP, 500_000);
  assert.equal(DEFAULT_RUN_PHASE_WALL_CLOCK_SECS, 5400); // 90min
  assert.equal(DEFAULT_RUN_TOTAL_WALL_CLOCK_SECS, 28800); // 8h
});

test("all-None budgets (the common case) fall back to the default caps", () => {
  const gauges = budgetGauges(
    plan({ phases: [phase({ token_usage: 100_000, wall_clock_secs: 600 })] }),
  );
  const byId = Object.fromEntries(gauges.map((g) => [g.id, g]));
  assert.equal(byId["phase-tokens"].cap, DEFAULT_RUN_PHASE_TOKEN_CAP);
  assert.equal(byId["phase-wall-clock"].cap, DEFAULT_RUN_PHASE_WALL_CLOCK_SECS);
  assert.equal(byId["run-wall-clock"].cap, DEFAULT_RUN_TOTAL_WALL_CLOCK_SECS);
});

test("explicit budgets override the defaults", () => {
  const gauges = budgetGauges(
    plan({
      budgets: {
        per_phase_token_cap: 50_000,
        per_phase_wall_clock_secs: 1200,
        total_run_wall_clock_secs: 7200,
      },
    }),
  );
  assert.deepEqual(
    gauges.map((g) => g.cap),
    [50_000, 1200, 7200],
  );
});

test("per-phase gauges track the worst phase, not the sum", () => {
  const gauges = budgetGauges(
    plan({
      phases: [
        phase({ execution_id: "a", token_usage: 200_000, wall_clock_secs: 1800 }),
        phase({ execution_id: "b", token_usage: 50_000, wall_clock_secs: 3600 }),
      ],
    }),
  );
  const byId = Object.fromEntries(gauges.map((g) => [g.id, g]));
  assert.equal(byId["phase-tokens"].used, 200_000);
  assert.equal(byId["phase-wall-clock"].used, 3600);
});

test("total gauge reads the plan's run-level wall clock, not the phase sum", () => {
  const gauges = budgetGauges(
    plan({
      wall_clock_secs: 900,
      phases: [
        phase({ execution_id: "a", wall_clock_secs: 300 }),
        phase({ execution_id: "b", wall_clock_secs: 300 }),
      ],
    }),
  );
  assert.equal(gauges.find((g) => g.id === "run-wall-clock").used, 900);
});

test("gaugePercent clamps a blown cap at 100 and guards zero caps", () => {
  assert.equal(gaugePercent(150, 100), 100);
  assert.equal(gaugePercent(50, 100), 50);
  assert.equal(gaugePercent(10, 0), 0);
});

test("formatters", () => {
  assert.equal(formatDuration(45), "45s");
  assert.equal(formatDuration(95), "1m 35s");
  assert.equal(formatDuration(5400), "1h 30m");
  assert.equal(formatTokens(1234567), "1,234,567");
});

test("parseParkedQuestions (shared with RunQueuePanel) extracts the executor marker", () => {
  const spec = [
    {
      question: "Which repo?",
      header: "Repo",
      options: [{ label: "a", description: "" }],
      multiSelect: false,
    },
  ];
  const payload = `stalled: __QUESTIONS__${JSON.stringify(spec)}__END__`;
  assert.deepEqual(parseParkedQuestions(payload).questions, spec);
  assert.equal(parseParkedQuestions("plain reason").questions, null);
  assert.equal(parseParkedQuestions(undefined).questions, null);
});

test("parkedInbox: one card per currently-parked phase, structured vs raw split", () => {
  const spec = [
    {
      question: "Which DB?",
      header: "Storage",
      options: [{ label: "sqlite", description: "" }],
      multiSelect: false,
    },
  ];
  const structured = phase({
    execution_id: "x/loop-1",
    status: "parked",
    park_payload: `__QUESTIONS__${JSON.stringify(spec)}__END__`,
  });
  const raw = phase({
    execution_id: "x/loop-2",
    status: "parked",
    park_payload: "manual approval waited past deadline",
  });
  // Same payload on a completed phase — park_payload persists, must NOT card.
  const doneWithPayload = phase({
    execution_id: "x/loop-3",
    status: "completed",
    park_payload: "stalled: old question",
  });
  const queuedNoPayload = phase({ execution_id: "x/loop-4", status: "queued" });

  const cards = parkedInbox(plan({ phases: [structured, raw, doneWithPayload, queuedNoPayload] }));

  assert.equal(cards.length, 2);
  assert.equal(cards[0].phase.execution_id, "x/loop-1");
  assert.deepEqual(cards[0].questions, spec);
  assert.equal(cards[1].phase.execution_id, "x/loop-2");
  assert.equal(cards[1].questions, null);
});

test("isUnresolvedParkedQuestion / hasUnresolvedParkedQuestions: mirror derive_verdict's Parked arm", () => {
  // Parked with a question payload (structured or raw text) → unresolved.
  assert.equal(
    isUnresolvedParkedQuestion(phase({ status: "parked", park_payload: "stalled: need input" })),
    true,
  );
  // Parked with a verify verdict → that's a verdict outcome, not an open question.
  assert.equal(
    isUnresolvedParkedQuestion(phase({ status: "parked", park_payload: "verify verdict: WARN" })),
    false,
  );
  assert.equal(
    isUnresolvedParkedQuestion(phase({ status: "parked", park_payload: "verify verdict: BLOCK" })),
    false,
  );
  // Any other status → not a parked question, even with a lingering payload.
  assert.equal(isUnresolvedParkedQuestion(phase({ status: "completed" })), false);
  assert.equal(isUnresolvedParkedQuestion(phase({ status: "queued" })), false);

  // The "morning report ready" flag: lit while any parked question remains…
  assert.equal(
    hasUnresolvedParkedQuestions(
      plan({
        phases: [
          phase({ execution_id: "a", status: "completed" }),
          phase({ execution_id: "b", status: "parked", park_payload: "waiting on you" }),
        ],
      }),
    ),
    true,
  );
  // …cleared once every parked question is resolved (requeued/completed).
  assert.equal(
    hasUnresolvedParkedQuestions(
      plan({
        phases: [
          phase({ execution_id: "a", status: "completed" }),
          phase({ execution_id: "b", status: "parked", park_payload: "verify verdict: WARN" }),
        ],
      }),
    ),
    false,
  );
  assert.equal(hasUnresolvedParkedQuestions(plan({ phases: [] })), false);
});

test("shouldAutoSelectNightVariant: once per drawer-open span, per project", () => {
  const base = {
    drawerOpen: true,
    projectPath: "/repo",
    nightPlan: plan(),
    activeTopLevelTab: "overview",
    switchedForPath: null,
  };
  // Fresh open on a project with an active run → switch.
  assert.equal(shouldAutoSelectNightVariant(base), true);
  // Already showing the Agent tab (user's persisted tab) → nothing to do…
  assert.equal(shouldAutoSelectNightVariant({ ...base, activeTopLevelTab: "agent" }), false);
  // …but the caller still latches, so a later manual tab change doesn't yank.
  assert.equal(
    shouldAutoSelectNightVariant({ ...base, switchedForPath: "/repo", activeTopLevelTab: "loops" }),
    false,
  );
  // Switching project while the drawer stays open re-arms the switch.
  assert.equal(
    shouldAutoSelectNightVariant({ ...base, projectPath: "/other", switchedForPath: "/repo" }),
    true,
  );
  // No active/queued run plan (or drawer closed) → never switch.
  assert.equal(shouldAutoSelectNightVariant({ ...base, nightPlan: null }), false);
  assert.equal(shouldAutoSelectNightVariant({ ...base, drawerOpen: false }), false);
});

// Minimal real-shape Epic tree (mirrors epic.rs / types/index.ts) for the
// "Plan tonight" gate. Each loop: [id?, checked, done_in_history].
function epicWithLoops(loops) {
  return {
    slug: "e",
    title: "E",
    milestone: "m",
    status: "in_progress",
    description: "",
    dir: "docs/epics/e",
    prds: [
      {
        slug: "p",
        epic: "e",
        status: "accepted",
        description: "",
        file: "prd-p.md",
        phases: [{ name: "Phase 1", loops }],
      },
    ],
  };
}

test("hasQueueablePhases: mirrors EpicsPanel's picker gate (!done && !noId)", () => {
  const loop = (id, checked = false, done_in_history = false) => ({
    title: "T",
    checked,
    done_in_history,
    ...(id ? { id } : {}),
  });

  // Open loop with a stable ID → queueable.
  assert.equal(hasQueueablePhases([epicWithLoops([loop("p/t")])]), true);
  // Legacy ID-less loop → not queueable (no join key).
  assert.equal(hasQueueablePhases([epicWithLoops([loop(null)])]), false);
  // Checked off or done in history → not queueable.
  assert.equal(hasQueueablePhases([epicWithLoops([loop("p/t", true)])]), false);
  assert.equal(hasQueueablePhases([epicWithLoops([loop("p/t", false, true)])]), false);
  // Any open ID'd loop anywhere in the tree qualifies…
  assert.equal(
    hasQueueablePhases([epicWithLoops([loop(null, true), loop("p/t2")])]),
    true,
  );
  // …and an empty tree doesn't.
  assert.equal(hasQueueablePhases([]), false);
  assert.equal(hasQueueablePhases([epicWithLoops([])]), false);
});

test("dependencyLabel: mirrors build_run_plan's authored-order predecessor chain", () => {
  const selected = ["prd/a", "prd/b", "prd/c"];
  const idToTitle = { "prd/a": "Loop A", "prd/b": "Loop B", "prd/c": "Loop C" };

  // Phase 0 has no dependencies…
  assert.equal(dependencyLabel(0, selected, idToTitle), "runs first — no dependencies");
  // …every later phase depends on its immediate predecessor in selection order.
  assert.equal(dependencyLabel(1, selected, idToTitle), "depends on Loop A");
  assert.equal(dependencyLabel(2, selected, idToTitle), "depends on Loop B");
  // Reordering the selection re-chains the labels — order IS the dependency.
  const reordered = ["prd/c", "prd/a"];
  assert.equal(dependencyLabel(1, reordered, idToTitle), "depends on Loop C");
  // Unknown titles fall back to the raw execution ID.
  assert.equal(dependencyLabel(1, ["prd/x", "prd/y"], {}), "depends on prd/x");
});
