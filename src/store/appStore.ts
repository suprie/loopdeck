import { create } from "zustand";
import { persist } from "zustand/middleware";
import type { DiscoveredRepo, ProjectEntry, DetailTab } from "../types";

/**
 * Application state managed by Zustand.
 *
 * Persistence policy: only **navigation identifiers / preferences** are
 * persisted — `selectedProjectPath` (which project the user last opened) and
 * `detailTab` (which tab they were on). The full `ProjectEntry` — including its
 * live `run_state`, git metadata, and description — is **never** persisted; it
 * is always reloaded from Rust (`list_projects`) and derived via
 * `selectSelectedProject`. This avoids schema drift: a stale persisted object
 * can never shadow the Rust-backed list, and the selected project's run state
 * is always current rather than frozen at last close.
 *
 * NOTE: View routing is handled by `@tanstack/react-router` (see
 * `src/router.tsx`). The `currentView` / `setCurrentView` fields that
 * previously drove the conditional render in `App.tsx` have been removed.
 * Navigation is done via the router's `useNavigate()` hook or `<Link>`
 * components.
 */
interface AppState {
  // ── Data (never persisted; always reloaded from Rust) ──
  projects: ProjectEntry[];
  discoveredRepos: DiscoveredRepo[];

  // ── Navigation identifiers / preferences (persisted) ──
  /** Filesystem path of the project the user last opened. Only the identifier
   *  is persisted — the live `ProjectEntry` is derived from `projects` (Rust)
   *  via {@link selectSelectedProject}. */
  selectedProjectPath: string | null;
  /** Active tab in ProjectDetail (lifted from local state so the dashboard's
   *  Start button can land the user on the Agent tab). */
  detailTab: DetailTab;

  // ── Transient UI state (never persisted) ──
  isScanning: boolean;
  isLoading: boolean;
  error: string | null;
  /** Project path awaiting an auto-fired `agent_start_loop` when its Agent tab
   *  mounts. Set by the dashboard Start CTA; consumed + cleared by AgentPanel. */
  pendingAgentStart: string | null;

  // ── Actions ──
  setProjects: (projects: ProjectEntry[]) => void;
  setSelectedProjectPath: (path: string | null) => void;
  setDiscoveredRepos: (repos: DiscoveredRepo[]) => void;
  setScanning: (scanning: boolean) => void;
  setLoading: (loading: boolean) => void;
  setError: (error: string | null) => void;
  setDetailTab: (tab: DetailTab) => void;
  setPendingAgentStart: (path: string | null) => void;
  addProject: (project: ProjectEntry) => void;
  removeProjectByPath: (path: string) => void;
  updateProject: (project: ProjectEntry) => void;
  updateProjectDescription: (path: string, description: string) => void;
}

/**
 * Derive the live selected project from the Rust-backed `projects` list.
 *
 * Never persisted — always reflects the most recent `list_projects`, so the
 * returned entry's `run_state` / git metadata are always current, and a project
 * removed in another session resolves to `null` automatically (no separate
 * reconciliation pass needed in `loadProjects`). Returns a stable reference
 * (an element of `state.projects`, or `null`), so it is safe to use directly as
 * a Zustand selector under the default `Object.is` equality.
 */
export function selectSelectedProject(state: AppState): ProjectEntry | null {
  if (!state.selectedProjectPath) return null;
  return state.projects.find((p) => p.path === state.selectedProjectPath) ?? null;
}

export const useAppStore = create<AppState>()(
  persist(
    (set) => ({
      // Initial state
      projects: [],
      selectedProjectPath: null,
      discoveredRepos: [],
      isScanning: false,
      isLoading: false,
      error: null,
      detailTab: "overview",
      pendingAgentStart: null,

      // Simple setters
      setProjects: (projects) => set({ projects }),
      setSelectedProjectPath: (path) => set({ selectedProjectPath: path }),
      setDiscoveredRepos: (repos) => set({ discoveredRepos: repos }),
      setScanning: (scanning) => set({ isScanning: scanning }),
      setLoading: (loading) => set({ isLoading: loading }),
      setError: (error) => set({ error }),
      setDetailTab: (tab) => set({ detailTab: tab }),
      setPendingAgentStart: (path) => set({ pendingAgentStart: path }),

      // Mutations — only ever touch `projects`; the selected project is derived.
      addProject: (project) =>
        set((state) => ({
          projects: [...state.projects, project],
        })),

      removeProjectByPath: (path) =>
        set((state) => ({
          projects: state.projects.filter((p) => p.path !== path),
          selectedProjectPath:
            state.selectedProjectPath === path ? null : state.selectedProjectPath,
        })),

      updateProject: (project) =>
        set((state) => ({
          projects: state.projects.map((p) =>
            p.path === project.path ? project : p,
          ),
        })),

      updateProjectDescription: (path, description) =>
        set((state) => ({
          projects: state.projects.map((p) =>
            p.path === path ? { ...p, description } : p,
          ),
        })),
    }),
    {
      name: "loopdeck-nav",
      version: 1,
      // Persist navigation identifiers/preferences only. The full ProjectEntry
      // (carrying live run_state from Rust) is deliberately excluded — see the
      // file-level doc comment on schema drift.
      partialize: (state) => ({
        selectedProjectPath: state.selectedProjectPath,
        detailTab: state.detailTab,
      }),
      // Schema migration: v0 persisted the full `selectedProject` object; v1
      // persists only its `path`. Extract the path on first load so a one-time
      // upgrade keeps the user's last-opened project instead of silently
      // dropping the selection.
      migrate: (persisted, version) => {
        const p = (persisted ?? {}) as {
          selectedProject?: { path?: string } | null;
          selectedProjectPath?: string | null;
          detailTab?: DetailTab;
        };
        // v0 persisted the full `selectedProject` object; v1 persists only its
        // path. Extract the path on first load so a one-time upgrade keeps the
        // user's last-opened project instead of silently dropping the selection.
        const path =
          version < 1 && p.selectedProjectPath === undefined
            ? (p.selectedProject?.path ?? null)
            : (p.selectedProjectPath ?? null);
        return {
          selectedProjectPath: path,
          detailTab: p.detailTab ?? "overview",
        };
      },
    },
  ),
);
