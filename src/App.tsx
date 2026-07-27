import { useEffect } from "react";
import { RouterProvider, router } from "./router";
import { useStuckSessions } from "./hooks/useStuckSessions";
import { ThemeProvider } from "./lib/theme";
import { Toaster } from "./components/ui/sonner";
import "./styles.css";

/**
 * Root application component.
 *
 * View routing is handled by `@tanstack/react-router` (see `src/router.tsx`).
 * The Toaster is mounted once here so any view can fire `toast()` calls (e.g.
 * promote-to-loop feedback).
 *
 * Project data is NOT bootstrapped here: the router's memory history always
 * starts at `/` (`initialEntries: ["/"]` in `router.tsx` — this is a Tauri
 * app with no deep-linkable URL bar), so `Dashboard`'s own mount effect is
 * the sole `loadProjects()` call site. It doubles as both the initial load
 * and the refresh-on-return-to-Dashboard trigger. An extra call here would
 * run concurrently with Dashboard's on every cold start — `list_projects`
 * spawns several git subprocesses and walks the tree per registered
 * project, so that's a real duplicate full-repository scan, not a cheap one.
 *
 * On mount, on window focus, and when the tab becomes visible again, this
 * component reconciles "stuck" `AskUserQuestion` prompts across the registry
 * — so a prompt that arrived while the Mac was locked or focus was elsewhere
 * is surfaced (toast + ProjectCard pill + detail-view callout) instead of
 * freezing the agent silently.
 */
export default function App() {
  const { reconcileStuckSessions } = useStuckSessions();

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
