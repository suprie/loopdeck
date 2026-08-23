import { useEffect, useState } from "react";
import {
  LayoutDashboard,
  Activity,
  Bot,
  GitBranch,
  RotateCw,
  Layers,
  Settings,
  FolderKanban,
} from "lucide-react";
import { useNavigate } from "@tanstack/react-router";
import { useAppStore } from "../../store/appStore";
import {
  CommandDialog,
  CommandEmpty,
  CommandGroup,
  CommandInput,
  CommandItem,
  CommandList,
  CommandSeparator,
} from "../ui/command";

const DESTINATIONS = [
  { to: "/", label: "Dashboard", icon: LayoutDashboard },
  { to: "/activity", label: "Activity", icon: Activity },
  { to: "/agent", label: "Agent Runner", icon: Bot },
  { to: "/decisions", label: "Decisions", icon: GitBranch },
  { to: "/loops", label: "Loops", icon: RotateCw },
  { to: "/epics", label: "Epics", icon: Layers },
  { to: "/settings", label: "Settings", icon: Settings },
] as const;

/**
 * Global ⌘K / Ctrl+K command palette — jump to a view or a registered
 * project. Replaces the previously-decorative `⌘K` hint in the top bar
 * (flagged in loops.md P6 as dead/misleading) with a real, minimal
 * implementation on top of the already-installed `cmdk`/`ui/command.tsx`.
 */
export function CommandPalette() {
  const [open, setOpen] = useState(false);
  const navigate = useNavigate();
  const projects = useAppStore((s) => s.projects);
  const openDrawer = useAppStore((s) => s.openDrawer);

  useEffect(() => {
    const onKeyDown = (e: KeyboardEvent) => {
      if (e.key === "k" && (e.metaKey || e.ctrlKey)) {
        e.preventDefault();
        setOpen((v) => !v);
      }
    };
    document.addEventListener("keydown", onKeyDown);
    return () => document.removeEventListener("keydown", onKeyDown);
  }, []);

  const goTo = (to: (typeof DESTINATIONS)[number]["to"]) => {
    setOpen(false);
    navigate({ to });
  };

  const goToProject = (path: string) => {
    setOpen(false);
    openDrawer(path);
  };

  return (
    <>
      <button
        type="button"
        onClick={() => setOpen(true)}
        className="flex h-8 w-full max-w-[360px] flex-1 items-center gap-2 rounded-md border border-border bg-background px-2.5 text-xs text-muted-foreground transition-colors hover:border-primary/40 hover:text-foreground"
      >
        <svg
          viewBox="0 0 16 16"
          fill="none"
          stroke="currentColor"
          strokeWidth="1.5"
          className="size-3.5 shrink-0"
        >
          <circle cx="7" cy="7" r="5" />
          <path d="m11 11 4 4" />
        </svg>
        <span className="truncate">Jump to a project, loop, or decision…</span>
        <kbd className="ml-auto shrink-0 rounded border border-border bg-muted px-1 py-0.5 font-mono text-[10px] text-muted-foreground">
          ⌘K
        </kbd>
      </button>

      <CommandDialog open={open} onOpenChange={setOpen}>
        <CommandInput placeholder="Jump to a view or a project…" />
        <CommandList>
          <CommandEmpty>Nothing found.</CommandEmpty>
          <CommandGroup heading="Go to">
            {DESTINATIONS.map((d) => (
              <CommandItem key={d.to} onSelect={() => goTo(d.to)}>
                <d.icon className="size-4" />
                {d.label}
              </CommandItem>
            ))}
          </CommandGroup>
          {projects.length > 0 && (
            <>
              <CommandSeparator />
              <CommandGroup heading="Projects">
                {projects.map((p) => (
                  <CommandItem
                    key={p.path}
                    value={`${p.name} ${p.path}`}
                    onSelect={() => goToProject(p.path)}
                  >
                    <FolderKanban className="size-4" />
                    <span className="truncate">{p.name}</span>
                  </CommandItem>
                ))}
              </CommandGroup>
            </>
          )}
        </CommandList>
      </CommandDialog>
    </>
  );
}
