import type { ReactNode } from "react";
import { LayoutGrid, GitBranch, Command } from "lucide-react";
import { useAppStore } from "../../store/appStore";
import { useProjects } from "../../hooks/useProjects";
import type { AppView } from "../../types";

interface AppShellProps {
  children: ReactNode;
}

const NAV_ITEMS: { view: AppView; label: string; icon: typeof LayoutGrid }[] = [
  { view: "dashboard", label: "Dashboard", icon: LayoutGrid },
  { view: "import", label: "Import Repo", icon: GitBranch },
];

export function AppShell({ children }: AppShellProps) {
  const currentView = useAppStore((s) => s.currentView);
  const error = useAppStore((s) => s.error);
  const setError = useAppStore((s) => s.setError);
  const setCurrentView = useAppStore((s) => s.setCurrentView);
  const { scanFolder } = useProjects();

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
      // User cancelled or dialog failed
    }
  };

  return (
    <div className="dark min-h-screen flex bg-background text-foreground">
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
            const active = currentView === item.view;
            const Icon = item.icon;
            return (
              <button
                key={item.view}
                onClick={() => {
                  if (item.view === "import") {
                    handleScan();
                  } else {
                    setCurrentView(item.view);
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
              </button>
            );
          })}
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
        {children}
      </main>
    </div>
  );
}

export function PageHeader({
  title,
  subtitle,
  actions,
}: {
  title: string;
  subtitle?: string;
  actions?: ReactNode;
}) {
  return (
    <header className="h-14 px-6 border-b border-border flex items-center justify-between bg-background/80 backdrop-blur sticky top-0 z-10 flex-shrink-0">
      <div>
        <h1 className="text-sm font-semibold tracking-tight">{title}</h1>
        {subtitle && (
          <p className="text-xs text-muted-foreground">{subtitle}</p>
        )}
      </div>
      {actions && <div className="flex items-center gap-2">{actions}</div>}
    </header>
  );
}
