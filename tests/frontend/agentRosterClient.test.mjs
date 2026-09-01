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
        // Mirror the backend: connection edits replace name + config but
        // preserve the entry's role charter.
        profiles[index] = { ...profiles[index], id: args.id, name: args.name, ...args.agentConfig };
        return { ...profiles[index] };
      }
      case "update_agent_charter": { // eslint-disable-line no-case-declarations
        const index = profiles.findIndex((profile) => profile.id === args.id);
        assert.notEqual(index, -1);
        profiles[index] = {
          ...profiles[index],
          persona_prompt: args.charter.persona_prompt,
          allowed_skills: args.charter.allowed_skills,
          output_contract: args.charter.output_contract,
        };
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

test("charter update replaces role fields and keeps connection settings", async () => {
  const roster = inMemoryRoster();
  const created = await roster.create({ name: "QA", harness: "claude", model: "opus" });

  const chartered = await roster.updateCharter(created.id, {
    persona_prompt: "You are the QA agent.",
    allowed_skills: ["loopdeck-prd-verifier"],
    output_contract: "End with a verdict.",
  });
  assert.equal(chartered.persona_prompt, "You are the QA agent.");
  assert.deepEqual(chartered.allowed_skills, ["loopdeck-prd-verifier"]);
  assert.equal(chartered.output_contract, "End with a verdict.");
  assert.equal(chartered.model, "opus"); // connection settings untouched

  // A later connection edit must not drop the charter.
  await roster.update(created.id, { name: "QA", harness: "claude", model: "sonnet" });
  let profiles = await roster.list();
  assert.equal(profiles[0].persona_prompt, "You are the QA agent.");
  assert.equal(profiles[0].model, "sonnet");

  // Replace-all: omitted fields clear.
  const cleared = await roster.updateCharter(created.id, { persona_prompt: "Skeptic." });
  assert.equal(cleared.persona_prompt, "Skeptic.");
  assert.equal(cleared.allowed_skills, undefined);
  assert.equal(cleared.output_contract, undefined);
});
