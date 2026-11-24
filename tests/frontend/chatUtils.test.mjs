import assert from "node:assert/strict";
import test from "node:test";
import { describeTool } from "../../src/components/detail/chatUtils.ts";

test("sub-agent call completion keeps a running child visible", () => {
  const label = describeTool("Sub-agent spawnAgent result", JSON.stringify({
    status: "completed", model: "small-model", prompt: "Check recovery",
    receiverThreadIds: ["child-1"], agentsStates: { "child-1": { status: "running" } },
  }));
  assert.match(label, /call completed/);
  assert.match(label, /child-1: running/);
  assert.match(label, /small-model/);
  assert.match(label, /Check recovery/);
});

test("sub-agent rows handle unavailable child status and malformed input", () => {
  assert.match(describeTool("Sub-agent wait", JSON.stringify({ receiverThreadIds: ["child-1"] })), /child-1: status unknown/);
  assert.equal(describeTool("Sub-agent wait", "invalid"), "Sub-agent wait");
  assert.equal(describeTool("Bash", '{"command":"cargo test"}'), "Bash · cargo test");
});
