import { create } from "zustand";
import { persist } from "zustand/middleware";
import type { DiscoveredRepo, ProjectEntry, DetailTab } from "../types";

/**
 * Application state managed by Zustand.
 *
 * NOTE: View routing is now handled by `@tanstack/react-router` (see
 * `src/router.tsx`).  The `currentView` / `setCurrentView` fields that
 * previously drove the conditional render in `App.tsx` have been removed.
 * Navigation is done via the router's `useNavigate()` hook or `<Link>`
 * components.
 */
interface AppState {
  // ── Data ──
  projects: ProjectEntry[];
  selectedProject: ProjectEntry | null;
  discoveredRepos: DiscoveredRepo[];

  // ── UI state ──
  isScanning: boolean;
  isLoading: boolean;
  error: string | null;
  /** Active tab in ProjectDetail (lifted from local state so the dashboard's
   *  Start button can land the user on the Agent tab). */
  detailTab: DetailTab;
  /** Project path awaiting an auto-fired `agent_start_loop` when its Agent tab
   *  mounts. Set by the dashboard Start CTA; consumed + cleared by AgentPanel. */
  pendingAgentStart: string | null;

  // ── Actions ──
  setProjects: (projects: ProjectEntry[]) => void;
  setSelectedProject: (project: ProjectEntry | null) => void;
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

export const useAppStore = create<AppState>()(
  persist(
    (set) => ({
      // Initial state
      projects: [],
      selectedProject: null,
      discoveredRepos: [],
      isScanning: false,
      isLoading: false,
      error: null,
      detailTab: "overview",
      pendingAgentStart: null,

      // Simple setters
      setProjects: (projects) => set({ projects }),
      setSelectedProject: (project) => set({ selectedProject: project }),
      setDiscoveredRepos: (repos) => set({ discoveredRepos: repos }),
      setScanning: (scanning) => set({ isScanning: scanning }),
      setLoading: (loading) => set({ isLoading: loading }),
      setError: (error) => set({ error }),
      setDetailTab: (tab) => set({ detailTab: tab }),
      setPendingAgentStart: (path) => set({ pendingAgentStart: path }),

      // Mutations
      addProject: (project) =>
        set((state) => ({
          projects: [...state.projects, project],
        })),

      removeProjectByPath: (path) =>
        set((state) => ({
          projects: state.projects.filter((p) => p.path !== path),
          selectedProject:
            state.selectedProject?.path === path ? null : state.selectedProject,
        })),

      updateProject: (project) =>
        set((state) => ({
          projects: state.projects.map((p) =>
            p.path === project.path ? project : p,
          ),
          selectedProject:
            state.selectedProject?.path === project.path
              ? project
              : state.selectedProject,
        })),

      updateProjectDescription: (path, description) =>
        set((state) => ({
          projects: state.projects.map((p) =>
            p.path === path ? { ...p, description } : p,
          ),
          selectedProject:
            state.selectedProject?.path === path
              ? { ...state.selectedProject, description }
              : state.selectedProject,
        })),
    }),
    {
      name: "loopdeck-nav",
      // Persist navigation-related state so an app restart lands the user back
      // where they were.  Note: `currentView` was removed — routing is now
      // handled by TanStack Router (memory history handles URL restoration).
      partialize: (state) => ({
        selectedProject: state.selectedProject,
        detailTab: state.detailTab,
      }),
    },
  ),
);
