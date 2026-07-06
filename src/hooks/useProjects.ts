import { useCallback } from "react";
import { useAppStore } from "../store/appStore";
import * as api from "../lib/tauri";
import type { AppError } from "../types";

/**
 * Hook providing async operations for project management.
 * All IPC calls go through typed wrappers in lib/tauri.ts.
 * Loading/error state is managed via the Zustand store.
 */
export function useProjects() {
  const setProjects = useAppStore((s) => s.setProjects);
  const setScanning = useAppStore((s) => s.setScanning);
  const setLoading = useAppStore((s) => s.setLoading);
  const setDiscoveredRepos = useAppStore((s) => s.setDiscoveredRepos);
  const setCurrentView = useAppStore((s) => s.setCurrentView);
  const setError = useAppStore((s) => s.setError);
  const addProject = useAppStore((s) => s.addProject);
  const removeProjectByPath = useAppStore((s) => s.removeProjectByPath);
  const updateProjectInStore = useAppStore((s) => s.updateProject);
  const updateProjectDescription = useAppStore((s) => s.updateProjectDescription);

  /** Load all projects from the global config on app startup. */
  const loadProjects = useCallback(async () => {
    setLoading(true);
    try {
      const projects = await api.listProjects();
      setProjects(projects);
    } catch (err) {
      const appErr = err as AppError;
      setError(appErr.message ?? String(err));
    } finally {
      setLoading(false);
    }
  }, [setProjects, setLoading, setError]);

  /** Scan a directory for discoverable repositories. */
  const scanFolder = useCallback(
    async (path: string) => {
      setScanning(true);
      setError(null);
      try {
        const repos = await api.scanDirectory(path);
        setDiscoveredRepos(repos);
        setCurrentView("import");
      } catch (err) {
        const appErr = err as AppError;
        setError(appErr.message ?? String(err));
      } finally {
        setScanning(false);
      }
    },
    [setScanning, setError, setDiscoveredRepos, setCurrentView],
  );

  /** Import a discovered repository into the registry. */
  const importRepo = useCallback(
    async (path: string) => {
      setLoading(true);
      setError(null);
      try {
        const entry = await api.importProject(path);
        addProject(entry);
        setCurrentView("dashboard");
        return entry;
      } catch (err) {
        const appErr = err as AppError;
        setError(appErr.message ?? String(err));
        return null;
      } finally {
        setLoading(false);
      }
    },
    [setLoading, setError, addProject, setCurrentView],
  );

  /** Remove a project from the registry. */
  const removeProject = useCallback(
    async (path: string) => {
      setLoading(true);
      setError(null);
      try {
        await api.removeProject(path);
        removeProjectByPath(path);
        setCurrentView("dashboard");
      } catch (err) {
        const appErr = err as AppError;
        setError(appErr.message ?? String(err));
      } finally {
        setLoading(false);
      }
    },
    [setLoading, setError, removeProjectByPath, setCurrentView],
  );

  /** Update a project's description. */
  const updateDescription = useCallback(
    async (path: string, description: string) => {
      setError(null);
      try {
        await api.updateDescription(path, description);
        updateProjectDescription(path, description);
      } catch (err) {
        const appErr = err as AppError;
        setError(appErr.message ?? String(err));
      }
    },
    [setError, updateProjectDescription],
  );

  /** Regenerate description from README and structure. */
  const regenerateDesc = useCallback(
    async (path: string) => {
      setLoading(true);
      setError(null);
      try {
        const newDesc = await api.regenerateDescription(path);
        updateProjectDescription(path, newDesc);
        return newDesc;
      } catch (err) {
        const appErr = err as AppError;
        setError(appErr.message ?? String(err));
        return null;
      } finally {
        setLoading(false);
      }
    },
    [setLoading, setError, updateProjectDescription],
  );

  /** Open a path in the system file manager. */
  const openInFinder = useCallback(
    async (path: string) => {
      try {
        await api.openInFinder(path);
      } catch (err) {
        const appErr = err as AppError;
        setError(appErr.message ?? String(err));
      }
    },
    [setError],
  );

  /** Rescan a project to refresh git info. */
  const rescanProject = useCallback(
    async (path: string) => {
      setError(null);
      try {
        const updated = await api.rescanProject(path);
        updateProjectInStore(updated);
        return updated;
      } catch (err) {
        const appErr = err as AppError;
        setError(appErr.message ?? String(err));
        return null;
      }
    },
    [setError, updateProjectInStore],
  );

  /** Open a path in the system terminal. */
  const openInTerminal = useCallback(
    async (path: string) => {
      try {
        await api.openInTerminal(path);
      } catch (err) {
        const appErr = err as AppError;
        setError(appErr.message ?? String(err));
      }
    },
    [setError],
  );

  return {
    loadProjects,
    scanFolder,
    importRepo,
    removeProject,
    updateDescription,
    regenerateDesc,
    rescanProject,
    openInFinder,
    openInTerminal,
  };
}
