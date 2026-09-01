import type { NamedAgentConfig, NamedAgentConfigInput, RoleCharter } from "../types";

type BackendNamedAgentConfig = Omit<NamedAgentConfig, "is_default">;

export type AgentRosterInvoke = <T>(
  command: string,
  args?: Record<string, unknown>,
) => Promise<T>;

/**
 * Typed roster client with an injectable transport. Production passes Tauri's
 * invoke wrapper; the dependency-free frontend contract test uses an in-memory
 * transport to exercise a complete CRUD/default-selection roundtrip.
 */
export function createAgentRosterClient(invoke: AgentRosterInvoke) {
  const getDefaultBackend = () =>
    invoke<BackendNamedAgentConfig | null>("get_default_agent_config");

  const enrichDefault = async (
    profile: BackendNamedAgentConfig,
  ): Promise<NamedAgentConfig> => {
    const currentDefault = await getDefaultBackend();
    return { ...profile, is_default: profile.id === currentDefault?.id };
  };

  return {
    async list(): Promise<NamedAgentConfig[]> {
      const [profiles, currentDefault] = await Promise.all([
        invoke<BackendNamedAgentConfig[]>("list_agent_configs"),
        getDefaultBackend(),
      ]);
      return profiles.map((profile) => ({
        ...profile,
        is_default: profile.id === currentDefault?.id,
      }));
    },

    async create(config: NamedAgentConfigInput): Promise<NamedAgentConfig> {
      const { name, ...agentConfig } = config;
      const profile = await invoke<BackendNamedAgentConfig>("create_agent_config", {
        name,
        agentConfig,
      });
      return enrichDefault(profile);
    },

    async update(
      id: string,
      config: NamedAgentConfigInput,
    ): Promise<NamedAgentConfig> {
      const { name, ...agentConfig } = config;
      const profile = await invoke<BackendNamedAgentConfig>("update_agent_config", {
        id,
        name,
        agentConfig,
      });
      return enrichDefault(profile);
    },

    delete(id: string): Promise<void> {
      return invoke<void>("delete_agent_config", { id });
    },

    /** Replace a profile's advisory role charter; empty fields clear. */
    async updateCharter(id: string, charter: RoleCharter): Promise<NamedAgentConfig> {
      const profile = await invoke<BackendNamedAgentConfig>("update_agent_charter", {
        id,
        charter,
      });
      return enrichDefault(profile);
    },

    async getDefault(): Promise<NamedAgentConfig | null> {
      const profile = await getDefaultBackend();
      return profile ? { ...profile, is_default: true } : null;
    },

    async setDefault(id: string): Promise<NamedAgentConfig> {
      const profile = await invoke<BackendNamedAgentConfig>("set_default_agent_config", { id });
      return { ...profile, is_default: true };
    },
  };
}
