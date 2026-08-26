import assert from "node:assert/strict";
import test from "node:test";
import {
  budgetGauges,
  gaugePercent,
  formatDuration,
  formatTokens,
  parseParkedQuestions,
  parkedInbox,
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
