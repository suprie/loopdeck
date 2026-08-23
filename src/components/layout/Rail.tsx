import { Link, useRouterState } from "@tanstack/react-router";
import { LayoutGrid, Moon, Pin, PinOff, Settings } from "lucide-react";
import { useState } from "react";
import { cn } from "@/lib/utils";
import { useAppStore } from "@/store/appStore";
import { useProjects } from "@/hooks/useProjects";
import { useNightRunStatuses } from "@/hooks/useNightRunStatuses";
import { doorInitials, selectRailDoors } from "@/lib/rail";
import type { ProjectEntry, RunState } from "@/types";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
} from "../ui/dropdown-menu";

/** Text/ring tone per live run state. `done` is transient/rarely observed
 *  (see `RunState` doc) but styled defensively for completeness. */
const RUN_STATE_TONE: Record<RunState, string | null> = {
  idle: null,
  working: "var(--primary)",
  waiting: "var(--destructive)",
  done: "var(--success)",
};

function doorStyle(tone: string | null): React.CSSProperties {
  if (!tone) return {};
  return {
    boxShadow: `0 0 0 1.5px ${tone}, 0 0 10px color-mix(in oklab, ${tone} 45%, transparent)`,
    background: `color-mix(in oklab, ${tone} 14%, var(--surface))`,
  };
}

const RUN_STATE_TEXT: Record<RunState, string> = {
  idle: "text-muted-foreground",
  working: "text-primary",
  waiting: "text-destructive",
  done: "text-success",
};

function Door({
  project,
  active,
  nightRun,
  onSelect,
}: {
  project: ProjectEntry;
  active: boolean;
  nightRun: boolean;
  onSelect: () => void;
}) {
  const { setPinned } = useProjects();
  const [menuOpen, setMenuOpen] = useState(false);
  const tone = RUN_STATE_TONE[project.run_state];

  return (
    <DropdownMenu open={menuOpen} onOpenChange={setMenuOpen}>
      <div className="relative">
        {/* Invisible, click-through anchor: positions the menu at the door
         *  without hijacking left-click (Radix's real Trigger opens on any
         *  click — pin/unpin is right-click only per the PRD's Phase 1
         *  decision, so we drive `open` ourselves from onContextMenu below). */}
        <DropdownMenuTrigger asChild>
          <span className="pointer-events-none absolute inset-0" aria-hidden />
        </DropdownMenuTrigger>
        <button
          type="button"
          title={project.name}
          onClick={onSelect}
          onContextMenu={(e) => {
            e.preventDefault();
            setMenuOpen(true);
          }}
          className={cn(
            "relative grid size-11 shrink-0 place-items-center rounded-2xl border font-display text-[13px] font-semibold transition-transform hover:scale-105",
            tone ? "border-transparent" : "border-border bg-background",
            RUN_STATE_TEXT[project.run_state],
            active && "ring-2 ring-foreground/40 ring-offset-2 ring-offset-surface",
          )}
          style={doorStyle(tone)}
        >
          {doorInitials(project.name)}
          {project.pinned && (
            <Pin className="absolute -bottom-1 -left-1 size-3 rotate-45 fill-current text-muted-foreground/70" />
          )}
          {nightRun && (
            <span
              className="absolute -right-1 -top-1 grid size-4 place-items-center rounded-full border border-surface"
              style={{ background: "var(--warning)" }}
              title="Overnight run active or queued"
            >
              <Moon className="size-2.5 text-white" />
            </span>
          )}
        </button>
      </div>
      <DropdownMenuContent align="start" side="right">
        <DropdownMenuItem onClick={() => setPinned(project.path, !project.pinned)}>
          {project.pinned ? (
            <>
              <PinOff className="size-3.5" /> Unpin from rail
            </>
          ) : (
            <>
              <Pin className="size-3.5" /> Pin to rail
            </>
          )}
        </DropdownMenuItem>
      </DropdownMenuContent>
    </DropdownMenu>
  );
}

/**
 * 72px project rail — one door per registered project, replacing
 * `AppShell.tsx`'s old feature-first sidebar nav. See
 * `docs/epics/selasar-revamp/prd-rail-corridor-shell.md` Phase 1.
 */
export function Rail() {
  const pathname = useRouterState({ select: (s) => s.location.pathname });
  const projects = useAppStore((s) => s.projects);
  const drawerOpen = useAppStore((s) => s.drawerOpen);
  const selectedProjectPath = useAppStore((s) => s.selectedProjectPath);
  const openDrawer = useAppStore((s) => s.openDrawer);
  const { doors, overflow } = selectRailDoors(projects);
  const nightRunStatuses = useNightRunStatuses(doors.map((p) => p.path));

  return (
    <nav className="flex w-[72px] shrink-0 flex-col items-center border-r border-border bg-surface py-3">
      <div className="flex min-h-0 flex-1 flex-col items-center gap-2 overflow-y-auto px-2">
        {doors.map((project) => (
          <Door
            key={project.path}
            project={project}
            active={drawerOpen && selectedProjectPath === project.path}
            nightRun={nightRunStatuses[project.path] ?? false}
            onSelect={() => openDrawer(project.path)}
          />
        ))}
        {overflow && (
          <Link
            to="/"
            title="All projects"
            className="grid size-11 shrink-0 place-items-center rounded-2xl border border-dashed border-border text-muted-foreground transition-colors hover:border-foreground/30 hover:text-foreground"
          >
            <LayoutGrid className="size-4" />
          </Link>
        )}
      </div>

      <Link
        to="/settings"
        title="Settings"
        className={cn(
          "mt-2 grid size-11 shrink-0 place-items-center rounded-2xl transition-colors",
          pathname === "/settings"
            ? "bg-accent text-foreground"
            : "text-muted-foreground hover:bg-accent/60 hover:text-foreground",
        )}
      >
        <Settings className="size-4" />
      </Link>
    </nav>
  );
}
