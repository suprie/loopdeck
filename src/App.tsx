import { useEffect } from "react";
import { RouterProvider, router } from "./router";
import { useProjects } from "./hooks/useProjects";
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
 */
export default function App() {
  const { loadProjects } = useProjects();

  useEffect(() => {
    loadProjects();
  }, [loadProjects]);

  return (
    <ThemeProvider>
      <RouterProvider router={router} />
      <Toaster />
    </ThemeProvider>
  );
}
