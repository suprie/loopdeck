import { useCallback } from "react";
import { useNavigate } from "@tanstack/react-router";
import { useAppStore } from "../store/appStore";
import * as api from "../lib/tauri";
import type { AppError } from "../types";

/**
 * Hook providing async operations for project management.
 *
 * Navigation after side-effecting operations (scan → import, import → dashboard,
 * remove → dashboard) now uses `useNavigate()` from TanStack Router instead of
 * the old Zustand `setCurrentView`.
 */
export function useProjects() {
  const navigate = useNavigate();
  const setProjects = useAppStore((s) => s.setProjects);
  const setScanning = useAppStore((s) => s.setScanning);
  const setLoading = useAppStore((s) => s.setLoading);
  const setDiscoveredRepos = useAppStore((s) => s.setDiscoveredRepos);
  const setError = useAppStore((s) => s.setError);
  const addProject = useAppStore((s) => s.addProject);
  const removeProjectByPath = useAppStore((s) => s.removeProjectByPath);
  const updateProjectInStore = useAppStore((s) => s.updateProject);
  const updateProjectDescription = useAppStore((s) => s.updateProjectDescription);

  /** Load all projects from the global config on app startup.
   *
   * No selection reconciliation is needed: the selected project is derived from
   * this list (`selectSelectedProject` in `appStore`), so a project removed in
   * another session resolves to `null` and a changed one is automatically fresh
   * — both come straight from Rust, with no persisted copy to reconcile against.
   */
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
        navigate({ to: "/import" });
      } catch (err) {
        const appErr = err as AppError;
        setError(appErr.message ?? String(err));
      } finally {
        setScanning(false);
      }
    },
    [setScanning, setError, setDiscoveredRepos, navigate],
  );

  /** Import a discovered repository into the registry. */
  const importRepo = useCallback(
    async (path: string) => {
      setLoading(true);
      setError(null);
      try {
        const entry = await api.importProject(path);
        addProject(entry);
        navigate({ to: "/" });
        return entry;
      } catch (err) {
        const appErr = err as AppError;
        setError(appErr.message ?? String(err));
        return null;
      } finally {
        setLoading(false);
      }
    },
    [setLoading, setError, addProject, navigate],
  );

  /** Remove a project from the registry. */
  const removeProject = useCallback(
    async (path: string) => {
      setLoading(true);
      setError(null);
      try {
        await api.removeProject(path);
        removeProjectByPath(path);
        navigate({ to: "/" });
      } catch (err) {
        const appErr = err as AppError;
        setError(appErr.message ?? String(err));
      } finally {
        setLoading(false);
      }
    },
    [setLoading, setError, removeProjectByPath, navigate],
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

  /** Toggle per-project autonomous mode and reflect it in the store.
   *  Takes effect on the next spawned session (the live one keeps its policy
   *  until reset). The caller (ProjectDetail) gates the enable path behind a
   *  confirm dialog spelling out the risk. */
  const setAutonomous = useCallback(
    async (path: string, autonomous: boolean) => {
      setError(null);
      try {
        await api.setProjectAutonomous(path, autonomous);
        // Patch the store entry in place — the backend only persists the flag,
        // it doesn't return the full ProjectEntry.
        const existing = useAppStore
          .getState()
          .projects.find((p) => p.path === path);
        if (existing) {
          updateProjectInStore({ ...existing, autonomous });
        }
      } catch (err) {
        const appErr = err as AppError;
        setError(appErr.message ?? String(err));
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
    setAutonomous,
    openInFinder,
    openInTerminal,
  };
}
