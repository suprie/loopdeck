import {
  createRouter,
  createMemoryHistory,
  createRootRoute,
  createRoute,
  RouterProvider,
  Outlet,
  useNavigate,
  useParams,
  useRouterState,
  Link,
} from "@tanstack/react-router";
import { useAppStore } from "./store/appStore";
import { useProjects } from "./hooks/useProjects";
import { AppShell } from "./components/layout/AppShell";

// ── View imports ─────────────────────────────────────────────────────────────

import { Dashboard } from "./components/dashboard/Dashboard";
import { ActivityFeed } from "./components/activity/ActivityFeed";
import { AgentRunner } from "./components/agent/AgentRunner";
import { DecisionsView } from "./components/decisions/DecisionsView";
import { LoopsView } from "./components/loops/LoopsView";
import { EpicsView } from "./components/epics/EpicsView";
import { ImportFlow } from "./components/import/ImportFlow";
import { ProjectDetail } from "./components/detail/ProjectDetail";
import { Settings } from "./components/settings/Settings";
import { SpecEditorPage } from "./components/spec/SpecEditorPage";

// ── Root layout (AppShell) ───────────────────────────────────────────────────

function AppShellLayout() {
  const error = useAppStore((s) => s.error);
  const setError = useAppStore((s) => s.setError);
  const { scanFolder } = useProjects();

  /** The "Import Repo" nav item triggers a native folder dialog instead of
   *  routing to /import directly. The shared AppShell owns the link markup; we
   *  pass a handler keyed by destination so this Tauri-only behaviour stays
   *  out of the design-system shell. */
  const handleNavClick = async (to: string, e: React.MouseEvent) => {
    if (to !== "/import") return;
    e.preventDefault();
    try {
      const { open } = await import("@tauri-apps/plugin-dialog");
      const selected = await open({
        directory: true,
        multiple: false,
        title: "Select Folder to Scan",
      });
      if (selected && typeof selected === "string") {
        await scanFolder(selected);
      }
    } catch {
      // User cancelled or dialog failed.
    }
  };

  return (
    <AppShell onNavClick={handleNavClick}>
      {/* Error banner — kept from the previous shell */}
      {error && (
        <div className="flex items-center justify-between px-4 py-2 bg-[color-mix(in_oklab,var(--destructive)_12%,transparent)] text-destructive text-xs flex-shrink-0">
          <span>{error}</span>
          <button
            onClick={() => setError(null)}
            className="text-destructive/70 hover:text-destructive text-xs px-2 py-0.5 rounded hover:bg-[color-mix(in_oklab,var(--destructive)_15%,transparent)] transition"
          >
            Dismiss
          </button>
        </div>
      )}
      <Outlet />
    </AppShell>
  );
}

// ── 404 / error boundaries ───────────────────────────────────────────────────

function NotFoundComponent() {
  return (
    <div className="flex flex-1 items-center justify-center px-4">
      <div className="max-w-md text-center">
        <h1 className="text-7xl font-semibold tracking-tight">404</h1>
        <h2 className="mt-4 text-lg font-semibold">Page not found</h2>
        <p className="mt-2 text-sm text-muted-foreground">
          The page you're looking for doesn't exist or has been moved.
        </p>
        <Link
          to="/"
          className="mt-6 inline-flex h-9 items-center justify-center rounded-md bg-primary px-4 text-sm font-medium text-primary-foreground transition-opacity hover:opacity-90"
        >
          Go home
        </Link>
      </div>
    </div>
  );
}

function ErrorComponent({ error, reset }: { error: Error; reset: () => void }) {
  console.error(error);
  return (
    <div className="flex flex-1 items-center justify-center px-4">
      <div className="max-w-md text-center">
        <h1 className="text-xl font-semibold tracking-tight">This page didn't load</h1>
        <p className="mt-2 text-sm text-muted-foreground">
          Something went wrong. Try again or head back home.
        </p>
        <div className="mt-6 flex flex-wrap justify-center gap-2">
          <button
            onClick={reset}
            className="inline-flex h-9 items-center justify-center rounded-md bg-primary px-4 text-sm font-medium text-primary-foreground transition-opacity hover:opacity-90"
          >
            Try again
          </button>
          <Link
            to="/"
            className="inline-flex h-9 items-center justify-center rounded-md border border-border bg-background px-4 text-sm font-medium text-foreground transition-colors hover:bg-accent"
          >
            Go home
          </Link>
        </div>
      </div>
    </div>
  );
}

// ── Route tree ───────────────────────────────────────────────────────────────

const rootRoute = createRootRoute({
  component: AppShellLayout,
  notFoundComponent: NotFoundComponent,
  errorComponent: ErrorComponent,
});

const dashboardRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/",
  component: Dashboard,
});

const activityRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/activity",
  component: ActivityFeed,
});

const agentRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/agent",
  component: AgentRunner,
});

const decisionsRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/decisions",
  component: DecisionsView,
});

const loopsRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/loops",
  component: LoopsView,
});

const epicsRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/epics",
  component: EpicsView,
});

const settingsRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/settings",
  component: Settings,
});

const importRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/import",
  component: ImportFlow,
});

/**
 * Project detail route.
 *
 * The project's filesystem path is passed as a URL-encoded path segment so
 * slashes don't break routing (e.g. `/Users/foo` → `/project/%2FUsers%2Ffoo`).
 * The component decodes it back via `decodeURIComponent`.
 */
const projectRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/project/$projectPath",
  component: ProjectDetail,
});

/**
 * Spec editor route.
 *
 * Full-screen editor for epic README and PRD files. The relative path
 * (e.g. "feature-auth/README.md" or "feature-auth/prd-spec-layer.md") is
 * passed as a URL-encoded path segment and decoded by the component.
 */
const specEditorRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/spec/$relPath",
  component: () => {
    const { relPath } = useParams({ from: "/spec/$relPath" });
    return <SpecEditorPage relPath={decodeURIComponent(relPath)} />;
  },
});

const routeTree = rootRoute.addChildren([
  dashboardRoute,
  activityRoute,
  agentRoute,
  decisionsRoute,
  loopsRoute,
  epicsRoute,
  settingsRoute,
  importRoute,
  projectRoute,
  specEditorRoute,
]);

// ── Memory history (no browser URL bar in Tauri) ─────────────────────────────

const memoryHistory = createMemoryHistory({
  initialEntries: ["/"],
});

// ── Router instance ──────────────────────────────────────────────────────────

export const router = createRouter({
  routeTree,
  history: memoryHistory,
  defaultErrorComponent: ErrorComponent,
});

// Register router type for type-safe navigation.
declare module "@tanstack/react-router" {
  interface Register {
    router: typeof router;
  }
}

// ── Re-exports for convenience ───────────────────────────────────────────────
export { RouterProvider, Outlet, useNavigate, useParams, Link };
export { useRouterState };
