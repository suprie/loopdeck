import { Link, useRouterState } from "@tanstack/react-router";
import {
  Command,
  LayoutDashboard,
  Activity,
  Bot,
  GitBranch,
  RotateCw,
  Layers,
  Settings,
  Sun,
  Moon,
  Monitor,
} from "lucide-react";
import type { ReactNode } from "react";

import { cn } from "@/lib/utils";
import { useTheme } from "@/lib/theme";
import { useAppStore } from "@/store/appStore";
import { CommandPalette } from "./CommandPalette";

const navGroups = [
  {
    label: "Work",
    items: [
      { to: "/", label: "Overview", icon: LayoutDashboard, exact: true },
      { to: "/activity", label: "Activity", icon: Activity },
      { to: "/agent", label: "Agent Runner", icon: Bot },
    ],
  },
  {
    label: "Memory",
    items: [
      { to: "/decisions", label: "Decisions", icon: GitBranch },
      { to: "/loops", label: "Loops", icon: RotateCw },
      { to: "/epics", label: "Epics", icon: Layers },
    ],
  },
] as const;

function NavLink({
  to,
  label,
  icon: Icon,
  exact,
  pathname,
  onNavClick,
}: {
  to: string;
  label: string;
  icon: React.ComponentType<{ className?: string }>;
  exact?: boolean;
  pathname: string;
  onNavClick?: (to: string, e: React.MouseEvent) => void;
}) {
  const active = exact ? pathname === to : pathname === to || pathname.startsWith(to + "/");
  return (
    <Link
      to={to}
      onClick={(e) => onNavClick?.(to, e)}
      className={cn(
        "relative flex items-center gap-2.5 rounded-md px-3 py-1.5 text-sm transition-colors",
        active
          ? "nav-active-bar bg-accent text-foreground font-medium"
          : "text-muted-foreground hover:bg-accent/60 hover:text-foreground",
      )}
    >
      <Icon className="size-4" />
      {label}
    </Link>
  );
}

function ThemeToggle() {
  const { theme, setTheme } = useTheme();
  const opts = [
    { v: "light", icon: Sun, label: "Light" },
    { v: "auto", icon: Monitor, label: "Auto" },
    { v: "dark", icon: Moon, label: "Dark" },
  ] as const;
  return (
    <div className="inline-flex items-center gap-0.5 rounded-md border border-border bg-background p-0.5">
      {opts.map((o) => {
        const active = theme === o.v;
        const Icon = o.icon;
        return (
          <button
            key={o.v}
            type="button"
            onClick={() => setTheme(o.v)}
            className={cn(
              "inline-flex h-6 items-center justify-center rounded px-2 text-[11px] transition-colors",
              active
                ? "bg-surface-elevated text-foreground shadow-[var(--shadow-sm)]"
                : "text-muted-foreground hover:text-foreground",
            )}
            aria-label={o.label}
          >
            <Icon className="size-3" />
          </button>
        );
      })}
    </div>
  );
}

/**
 * Application shell: sidebar + content area.
 *
 * Designed to wrap a router `<Outlet />` as children (rendered by the root
 * route in `src/router.tsx`). `onNavClick` lets the host intercept a nav
 * click (used by the Tauri shell to open a native folder dialog for /import
 * instead of routing).
 */
export function AppShell({
  children,
  onNavClick,
}: {
  children: ReactNode;
  onNavClick?: (to: string, e: React.MouseEvent) => void;
}) {
  const pathname = useRouterState({ select: (s) => s.location.pathname });
  const isLoading = useAppStore((s) => s.isLoading);

  return (
    <div className="flex h-screen w-full overflow-hidden bg-background text-foreground">
      <aside className="flex w-60 shrink-0 flex-col border-r border-border bg-surface">
        <div className="flex items-center gap-2.5 px-4 py-4">
          <div
            className="flex size-8 items-center justify-center rounded-md text-primary-foreground"
            style={{
              background: "linear-gradient(135deg, oklch(0.6 0.2 295), oklch(0.55 0.22 320))",
            }}
          >
            <Command className="size-4" />
          </div>
          <div className="flex flex-col leading-tight">
            <span className="font-display text-sm font-semibold tracking-tight">LoopDeck</span>
            <span className="font-display text-[9px] uppercase tracking-wider text-muted-foreground">
              Engineering Cockpit
            </span>
          </div>
        </div>

        <nav className="flex flex-1 flex-col gap-4 px-2 py-2">
          {navGroups.map((group) => (
            <div key={group.label}>
              <div className="px-2.5 pb-1.5 text-[10px] font-semibold uppercase tracking-wider text-muted-foreground/70">
                {group.label}
              </div>
              <div className="flex flex-col gap-0.5">
                {group.items.map((n) => (
                  <NavLink key={n.to} {...n} pathname={pathname} onNavClick={onNavClick} />
                ))}
              </div>
            </div>
          ))}
        </nav>

        <div className="border-t border-border px-2 py-2">
          <NavLink
            to="/settings"
            label="Settings"
            icon={Settings}
            pathname={pathname}
            onNavClick={onNavClick}
          />
        </div>

        <div className="mx-2 mb-2 flex h-8 items-center gap-1.5 rounded-md border border-border px-2.5 text-[11px] text-muted-foreground">
          <span className="size-1.5 rounded-full bg-success" />
          Local only
          <span className="ml-auto font-mono text-[10px] text-muted-foreground/70">v0.2.0</span>
        </div>
      </aside>

      <div className="flex min-w-0 flex-1 flex-col">
        <header className="flex h-14 shrink-0 items-center gap-3 border-b border-border px-6">
          <CommandPalette />
          <div className="ml-auto flex items-center gap-3">
            <ThemeToggle />
            <div className="flex items-center gap-1.5 text-xs text-muted-foreground">
              <span
                className={cn(
                  "size-1.5 rounded-full",
                  isLoading ? "bg-warning" : "bg-success",
                )}
              />
              {isLoading ? "Loading…" : "Ready"}
            </div>
          </div>
        </header>
        <main className="flex min-w-0 flex-1 flex-col overflow-hidden">{children}</main>
      </div>
    </div>
  );
}

/**
 * Reusable page header rendered at the top of most views.
 * Matches the clone's proportions: border-b, px-8 py-5, text-lg title.
 */
export function PageHeader({
  title,
  subtitle,
  actions,
}: {
  title: ReactNode;
  subtitle?: ReactNode;
  actions?: ReactNode;
}) {
  return (
    <header className="flex items-start justify-between gap-4 border-b border-border px-8 py-5">
      <div className="min-w-0">
        <h1 className="text-lg font-semibold tracking-tight">{title}</h1>
        {subtitle && <p className="mt-0.5 text-xs text-muted-foreground">{subtitle}</p>}
      </div>
      {actions && <div className="flex items-center gap-2">{actions}</div>}
    </header>
  );
}
