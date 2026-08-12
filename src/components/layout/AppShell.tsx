import { Sun, Moon, Monitor } from "lucide-react";
import type { ReactNode } from "react";

import { cn } from "@/lib/utils";
import { useTheme } from "@/lib/theme";
import { useAppStore } from "@/store/appStore";
import { CommandPalette } from "./CommandPalette";
import { Rail } from "./Rail";

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
 * Application shell: 72px project rail + content area.
 *
 * Designed to wrap a router `<Outlet />` as children (rendered by the root
 * route in `src/router.tsx`). The rail (`Rail.tsx`) replaced the old
 * feature-first sidebar nav — see `prd-rail-corridor-shell` Phase 1; its
 * brand mark and "Local only vX" footer were dropped rather than carried
 * onto the rail, with the version badge moved into this shared header
 * instead so it stays visible on every page, not just the rail's foot.
 */
export function AppShell({ children }: { children: ReactNode }) {
  const isLoading = useAppStore((s) => s.isLoading);

  return (
    <div className="flex h-screen w-full overflow-hidden bg-background text-foreground">
      <Rail />

      <div className="flex min-w-0 flex-1 flex-col">
        <header className="flex h-14 shrink-0 items-center gap-3 border-b border-border px-6">
          <CommandPalette />
          <div className="ml-auto flex items-center gap-3">
            <div className="flex h-6 items-center gap-1.5 rounded-md border border-border px-2 text-[11px] text-muted-foreground">
              <span className="size-1.5 rounded-full bg-success" />
              Local only
              <span className="font-mono text-[10px] text-muted-foreground/70">v0.2.0</span>
            </div>
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
