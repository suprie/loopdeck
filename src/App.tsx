import { useEffect } from "react";
import { RouterProvider, router } from "./router";
import { useProjects } from "./hooks/useProjects";
import "./styles.css";

/**
 * Root application component.
 *
 * View routing is now handled by `@tanstack/react-router` (see `src/router.tsx`).
 * This component only bootstraps project data on mount and renders the router.
 */
export default function App() {
  const { loadProjects } = useProjects();

  useEffect(() => {
    loadProjects();
  }, [loadProjects]);

  return <RouterProvider router={router} />;
}
