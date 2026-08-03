import assert from "node:assert/strict";
import test from "node:test";
import { createAgentRosterClient } from "../../src/lib/agentRosterClient.ts";

function inMemoryRoster() {
  const profiles = [];
  let defaultId = null;
  let sequence = 0;

  const invoke = async (command, args = {}) => {
    switch (command) {
      case "list_agent_configs":
        return profiles.map((profile) => ({ ...profile }));
      case "get_default_agent_config":
        return profiles.find((profile) => profile.id === defaultId) ?? null;
      case "create_agent_config": { // eslint-disable-line no-case-declarations
        const profile = { id: `agent-${++sequence}`, name: args.name, ...args.agentConfig };
        profiles.push(profile);
        defaultId ??= profile.id;
        return { ...profile };
      }
      case "update_agent_config": { // eslint-disable-line no-case-declarations
        const index = profiles.findIndex((profile) => profile.id === args.id);
        assert.notEqual(index, -1);
        profiles[index] = { id: args.id, name: args.name, ...args.agentConfig };
        return { ...profiles[index] };
      }
      case "delete_agent_config": { // eslint-disable-line no-case-declarations
        const index = profiles.findIndex((profile) => profile.id === args.id);
        assert.notEqual(index, -1);
        profiles.splice(index, 1);
        if (defaultId === args.id) defaultId = profiles[0]?.id ?? null;
        return undefined;
      }
      case "set_default_agent_config":
        defaultId = args.id;
        return { ...profiles.find((profile) => profile.id === args.id) };
      default:
        throw new Error(`unexpected command: ${command}`);
    }
  };

  return createAgentRosterClient(invoke);
}

test("named roster CRUD roundtrip preserves IDs and default selection", async () => {
  const roster = inMemoryRoster();
  assert.deepEqual(await roster.list(), []);

  const first = await roster.create({ name: "Opus", harness: "claude", model: "opus" });
  const second = await roster.create({ name: "Codex", harness: "codex", model: "gpt" });
  assert.equal(first.is_default, true);
  assert.equal(second.is_default, false);

  const updated = await roster.update(second.id, {
    name: "Codex reviewer",
    harness: "codex",
    model: "gpt-sol",
  });
  assert.equal(updated.id, second.id);
  assert.equal(updated.name, "Codex reviewer");

  await roster.setDefault(second.id);
  let profiles = await roster.list();
  assert.equal(profiles.find((profile) => profile.id === second.id)?.is_default, true);
  assert.equal(profiles.find((profile) => profile.id === first.id)?.is_default, false);

  await roster.delete(first.id);
  profiles = await roster.list();
  assert.deepEqual(profiles.map((profile) => profile.id), [second.id]);
  assert.equal((await roster.getDefault())?.id, second.id);
});
