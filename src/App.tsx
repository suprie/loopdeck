import { useEffect } from "react";
import { RouterProvider, router } from "./router";
import { useProjects } from "./hooks/useProjects";
import { useStuckSessions } from "./hooks/useStuckSessions";
import { ThemeProvider } from "./lib/theme";
import { Toaster } from "./components/ui/sonner";
import "./styles.css";

/**
 * Root application component.
 *
 * View routing is handled by `@tanstack/react-router` (see `src/router.tsx`).
 * This component bootstraps project data on mount and renders the router
 * inside the theme provider. The Toaster is mounted once here so any view
 * can fire `toast()` calls (e.g. promote-to-loop feedback).
 *
 * On mount, on window focus, and when the tab becomes visible again, it also
 * reconciles "stuck" `AskUserQuestion` prompts across the registry — so a
 * prompt that arrived while the Mac was locked or focus was elsewhere is
 * surfaced (toast + ProjectCard pill + detail-view callout) instead of
 * freezing the agent silently.
 */
export default function App() {
  const { loadProjects } = useProjects();
  const { reconcileStuckSessions } = useStuckSessions();

  useEffect(() => {
    loadProjects();
  }, [loadProjects]);

  useEffect(() => {
    // Initial reconcile on mount, then re-check whenever the window regains
    // focus or the tab becomes visible again (the "locked Mac / switched
    // window" recovery path). `pageshow` covers the back-forward cache case.
    reconcileStuckSessions();
    const onFocus = () => reconcileStuckSessions();
    const onVisibility = () => {
      if (document.visibilityState === "visible") reconcileStuckSessions();
    };
    window.addEventListener("focus", onFocus);
    document.addEventListener("visibilitychange", onVisibility);
    return () => {
      window.removeEventListener("focus", onFocus);
      document.removeEventListener("visibilitychange", onVisibility);
    };
  }, [reconcileStuckSessions]);

  return (
    <ThemeProvider>
      <RouterProvider router={router} />
      <Toaster />
    </ThemeProvider>
  );
}
