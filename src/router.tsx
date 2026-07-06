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
import { LayoutGrid, GitBranch, Settings2, Command, Terminal, Activity, Lightbulb, Repeat } from "lucide-react";
import { useAppStore } from "./store/appStore";
import { useProjects } from "./hooks/useProjects";
import type { AppView } from "./types";

// ── View imports ─────────────────────────────────────────────────────────────

import { Dashboard } from "./components/dashboard/Dashboard";
import { ActivityFeed } from "./components/activity/ActivityFeed";
import { AgentRunner } from "./components/agent/AgentRunner";
import { DecisionsView } from "./components/decisions/DecisionsView";
import { LoopsView } from "./components/loops/LoopsView";
import { ImportFlow } from "./components/import/ImportFlow";
import { ProjectDetail } from "./components/detail/ProjectDetail";
import { Settings } from "./components/settings/Settings";

// ── Navigation config ────────────────────────────────────────────────────────

export const NAV_ITEMS: { to: string; label: string; icon: typeof LayoutGrid; view: AppView }[] = [
  { to: "/", label: "Dashboard", icon: LayoutGrid, view: "dashboard" },
  { to: "/activity", label: "Activity Feed", icon: Activity, view: "activity" },
  { to: "/agent", label: "Agent Runner", icon: Terminal, view: "agent" },
  { to: "/decisions", label: "Decisions", icon: Lightbulb, view: "decisions" },
  { to: "/loops", label: "Loops", icon: Repeat, view: "loops" },
  { to: "/import", label: "Import Repo", icon: GitBranch, view: "import" },
];

// ── Root layout (AppShell) ───────────────────────────────────────────────────

function AppShellLayout() {
  const error = useAppStore((s) => s.error);
  const setError = useAppStore((s) => s.setError);
  const { scanFolder } = useProjects();

  // Determine which nav item is active based on the current route location.
  const routerState = useRouterState();
  const currentPath = routerState.location.pathname;

  const handleScan = async () => {
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
    <div className="dark h-screen flex overflow-hidden bg-background text-foreground">
      {/* Sidebar */}
      <aside className="w-60 shrink-0 border-r border-border bg-surface/60 backdrop-blur flex flex-col">
        {/* Brand */}
        <div className="px-5 h-14 flex items-center gap-2 border-b border-border">
          <div className="size-7 rounded-md bg-gradient-to-br from-primary to-[oklch(0.65_0.2_255)] grid place-items-center">
            <Command className="size-4 text-primary-foreground" strokeWidth={2.5} />
          </div>
          <div>
            <div className="text-sm font-semibold tracking-tight">LoopDeck</div>
            <div className="text-[10px] text-muted-foreground uppercase tracking-widest">
              Engineering cockpit
            </div>
          </div>
        </div>

        {/* Navigation */}
        <nav className="flex-1 p-2 space-y-0.5">
          {NAV_ITEMS.map((item) => {
            const active = currentPath === item.to || (item.to !== "/" && currentPath.startsWith(item.to));
            const Icon = item.icon;
            return (
              <Link
                key={item.to}
                to={item.to}
                onClick={(e) => {
                  if (item.view === "import") {
                    e.preventDefault();
                    handleScan();
                  }
                }}
                className={`flex items-center gap-2.5 px-3 py-2 rounded-md text-sm transition-colors w-full text-left ${
                  active
                    ? "bg-accent text-foreground"
                    : "text-muted-foreground hover:bg-accent/50 hover:text-foreground"
                }`}
              >
                <Icon className="size-4" />
                {item.label}
              </Link>
            );
          })}
          {/* Settings — always last */}
          <Link
            to="/settings"
            className={`flex items-center gap-2.5 px-3 py-2 rounded-md text-sm transition-colors w-full text-left ${
              currentPath === "/settings"
                ? "bg-accent text-foreground"
                : "text-muted-foreground hover:bg-accent/50 hover:text-foreground"
            }`}
          >
            <Settings2 className="size-4" />
            Settings
          </Link>
        </nav>

        {/* Footer */}
        <div className="p-3 border-t border-border text-[11px] text-muted-foreground">
          <div className="flex items-center justify-between">
            <span>Local · v0.1.0</span>
            <kbd className="px-1.5 py-0.5 rounded bg-muted font-mono text-[10px]">⌘K</kbd>
          </div>
        </div>
      </aside>

      {/* Main content */}
      <main className="flex-1 min-w-0 flex flex-col">
        {/* Error banner */}
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
      </main>
    </div>
  );
}

// ── Route tree ───────────────────────────────────────────────────────────────

const rootRoute = createRootRoute({
  component: AppShellLayout,
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

const routeTree = rootRoute.addChildren([
  dashboardRoute,
  activityRoute,
  agentRoute,
  decisionsRoute,
  loopsRoute,
  settingsRoute,
  importRoute,
  projectRoute,
]);

// ── Memory history (no browser URL bar in Tauri) ─────────────────────────────

const memoryHistory = createMemoryHistory({
  initialEntries: ["/"],
});

// ── Router instance ──────────────────────────────────────────────────────────

export const router = createRouter({ routeTree, history: memoryHistory });

// Register router type for type-safe navigation.
declare module "@tanstack/react-router" {
  interface Register {
    router: typeof router;
  }
}

// ── Re-exports for convenience ───────────────────────────────────────────────

export { RouterProvider, Outlet, useNavigate, useParams, Link };
