import assert from "node:assert/strict";
import test from "node:test";
import {
  emptyRunEventProjection,
  projectRunEvent,
} from "../../src/lib/runEventProjection.ts";

let sequence = 0;

function event(payload, overrides = {}) {
  sequence += 1;
  return {
    version: 1,
    run_id: "run-fixture",
    sequence: null,
    event_id: `event-${sequence}`,
    timestamp: "2026-10-14T00:00:00Z",
    provider: "claude",
    payload,
    ...overrides,
  };
}

function reduceAll(events, initial = emptyRunEventProjection()) {
  return events.reduce(projectRunEvent, initial);
}

test("content deltas coalesce and start a busy turn", () => {
  const state = reduceAll([
    event({ type: "content", kind: "text", text: "hello " }),
    event({ type: "content", kind: "text", text: "world" }),
    event({ type: "content", kind: "thinking", text: "plan" }),
  ]);

  assert.equal(state.busy, true);
  assert.deepEqual(state.blocks, [
    { type: "text", text: "hello world" },
    { type: "thinking", thinking: "plan" },
  ]);
});

test("tool events append activity rows but dedicated surfaces are suppressed", () => {
  const state = reduceAll([
    event({ type: "tool", name: "Read", input_redacted: '{"file_path":"/repo/a.rs"}' }),
    event({ type: "tool", name: "AskUserQuestion", input_redacted: "{}" }),
  ]);

  assert.deepEqual(state.blocks, [
    { type: "tool_use", name: "Read", input: '{"file_path":"/repo/a.rs"}' },
  ]);
});

test("TaskUpdate tool activity preserves the task panel state", () => {
  const state = reduceAll([
    event({ type: "tool", name: "TaskUpdate", input_redacted: '{"taskId":"10","subject":"Ship it","status":"in_progress"}' }),
    event({ type: "tool", name: "TaskUpdate", input_redacted: '{"taskId":"10","subject":"","status":"completed"}' }),
    event({ type: "tool", name: "TaskUpdate", input_redacted: '{"taskId":"11","subject":"Later","status":"deleted"}' }),
  ]);

  assert.deepEqual(state.tasks, {
    10: { id: "10", subject: "Ship it", status: "completed" },
  });
  assert.deepEqual(state.blocks, []);
});

test("pending approval keeps the full render payload; allowing clears it", () => {
  const pending = event({
    type: "decision",
    decision_id: "req-1",
    kind: "approval",
    state: "pending",
    summary: "Bash: Run the test suite",
    detail_redacted: JSON.stringify({
      tool_name: "Bash",
      input: { command: "cargo test" },
    }),
  });
  const allowed = event({
    type: "decision",
    decision_id: "req-1",
    kind: "approval",
    state: "allowed",
    summary: "auto-allow: Bash",
    detail_redacted: JSON.stringify({
      tool_name: "Bash",
      input: { command: "cargo test" },
    }),
  });

  const withPending = projectRunEvent(emptyRunEventProjection(), pending);
  assert.deepEqual(withPending.permission, {
    requestId: "req-1",
    toolName: "Bash",
    input: '{"command":"cargo test"}',
  });

  const resolved = projectRunEvent(withPending, allowed);
  assert.equal(resolved.permission, null);
});

test("question decisions carry the structured questions and clear on answer", () => {
  const questions = [
    {
      question: "Which scope should this change cover?",
      header: "Scope",
      options: [{ label: "Minimal", description: "Only the reported bug" }],
      multiSelect: false,
    },
  ];
  const pending = event({
    type: "decision",
    decision_id: "q-1",
    kind: "question",
    state: "pending",
    summary: questions[0].question,
    detail_redacted: JSON.stringify(questions),
  });
  const answered = event({
    type: "decision",
    decision_id: "q-1",
    kind: "question",
    state: "answered",
    summary: "answered: Which scope should this change cover?",
  });

  const withPending = projectRunEvent(emptyRunEventProjection(), pending);
  assert.deepEqual(withPending.question, {
    requestId: "q-1",
    questions,
  });
  assert.equal(projectRunEvent(withPending, answered).question, null);
});

test("plan decisions carry the plan text and clear on resolution", () => {
  const pending = event({
    type: "decision",
    decision_id: "plan-1",
    kind: "plan_approval",
    state: "pending",
    summary: "plan: Ship it",
    detail_redacted: "Ship it",
  });
  const denied = event({
    type: "decision",
    decision_id: "plan-1",
    kind: "plan_approval",
    state: "denied",
    summary: "deny: plan: missing tests",
    detail_redacted: "Ship it",
  });

  const withPending = projectRunEvent(emptyRunEventProjection(), pending);
  assert.deepEqual(withPending.plan, { requestId: "plan-1", plan: "Ship it" });
  assert.equal(projectRunEvent(withPending, denied).plan, null);
});

test("terminal synthesizes the legacy result shape and clears pending cards", () => {
  const state = reduceAll([
    event({ type: "content", kind: "text", text: "done" }),
    event({
      type: "usage",
      input_tokens: 720,
      output_tokens: 140,
      total_cost_usd: 0.0061,
    }),
    event({
      type: "decision",
      decision_id: "req-1",
      kind: "approval",
      state: "pending",
      summary: "Bash",
      detail_redacted: JSON.stringify({ tool_name: "Bash", input: {} }),
    }),
    event({ type: "terminal", outcome: "completed", message_redacted: "done" }),
  ]);

  assert.equal(state.busy, false);
  assert.equal(state.permission, null);
  assert.equal(state.result?.type, "result");
  assert.equal(state.result?.result, "done");
  assert.equal(state.result?.is_error, false);
  assert.deepEqual(state.result?.usage, {
    input_tokens: 720,
    output_tokens: 140,
    total_cost_usd: 0.0061,
  });
});

test("a duplicate terminal event is idempotent (same reference, no double apply)", () => {
  const terminal = event({
    type: "terminal",
    outcome: "completed",
    message_redacted: "done",
  });
  const once = projectRunEvent(emptyRunEventProjection(), terminal);
  const twice = projectRunEvent(once, terminal);
  assert.equal(twice, once);
});

test("retry is transient; the final terminal remains authoritative", () => {
  const state = reduceAll([
    event({ type: "content", kind: "text", text: "partial" }),
    event({ type: "terminal", outcome: "error", message_redacted: "overloaded" }),
    event({ type: "retrying", attempt: 2, max_attempts: 4, error_redacted: "overloaded" }),
    event({ type: "content", kind: "text", text: "final" }),
    event({ type: "terminal", outcome: "completed", message_redacted: "ok" }),
  ]);

  assert.equal(state.busy, false);
  assert.equal(state.retrying, null);
  assert.equal(state.result?.result, "ok");
  assert.equal(state.result?.is_error, false);
  assert.equal(state.error, null);
});

test("error terminals surface a banner; interrupted ones do not", () => {
  const errored = reduceAll([
    event({ type: "terminal", outcome: "error", message_redacted: "upstream timeout" }),
  ]);
  assert.equal(errored.error, "upstream timeout");
  assert.equal(errored.result?.is_error, true);

  const interrupted = reduceAll([
    event({ type: "terminal", outcome: "interrupted", message_redacted: "stopped" }),
  ]);
  assert.equal(interrupted.error, null);
  assert.equal(interrupted.result?.is_error, false);
});

test("unsupported capability gaps do not disturb streaming state", () => {
  const before = emptyRunEventProjection();
  const after = projectRunEvent(
    before,
    event({ type: "unsupported", capability: "plan_approval", detail_redacted: null }),
  );
  assert.equal(after, before);
});
