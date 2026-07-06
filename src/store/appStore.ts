import { create } from "zustand";
import type { DiscoveredRepo, ProjectEntry, AppView } from "../types";

interface AppState {
  // ── Data ──
  projects: ProjectEntry[];
  selectedProject: ProjectEntry | null;
  discoveredRepos: DiscoveredRepo[];

  // ── UI state ──
  currentView: AppView;
  isScanning: boolean;
  isLoading: boolean;
  error: string | null;

  // ── Actions ──
  setProjects: (projects: ProjectEntry[]) => void;
  setSelectedProject: (project: ProjectEntry | null) => void;
  setDiscoveredRepos: (repos: DiscoveredRepo[]) => void;
  setCurrentView: (view: AppView) => void;
  setScanning: (scanning: boolean) => void;
  setLoading: (loading: boolean) => void;
  setError: (error: string | null) => void;
  addProject: (project: ProjectEntry) => void;
  removeProjectByPath: (path: string) => void;
  updateProject: (project: ProjectEntry) => void;
  updateProjectDescription: (path: string, description: string) => void;
}

export const useAppStore = create<AppState>((set) => ({
  // Initial state
  projects: [],
  selectedProject: null,
  discoveredRepos: [],
  currentView: "dashboard",
  isScanning: false,
  isLoading: false,
  error: null,

  // Simple setters
  setProjects: (projects) => set({ projects }),
  setSelectedProject: (project) =>
    set({ selectedProject: project, currentView: project ? "detail" : "dashboard" }),
  setDiscoveredRepos: (repos) => set({ discoveredRepos: repos }),
  setCurrentView: (view) => set({ currentView: view, error: null }),
  setScanning: (scanning) => set({ isScanning: scanning }),
  setLoading: (loading) => set({ isLoading: loading }),
  setError: (error) => set({ error }),

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
}));
