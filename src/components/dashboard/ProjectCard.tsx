import { memo } from "react";
import {
  ArrowUpRight,
  GitCommitHorizontal,
  Clock,
  RefreshCw,
  FolderOpen,
  Terminal,
  Trash2,
  RotateCw,
  Play,
  Loader2,
  MessageCircleQuestion,
  Check,
  FileDiff,
} from "lucide-react";
import type { ProjectEntry, RunState } from "../../types";
import { relativeTime } from "../../lib/time";
import { StatusBadge } from "../shared/StatusBadge";
import { cn } from "../../lib/utils";

interface ProjectCardProps {
  project: ProjectEntry;
  onSelect: (project: ProjectEntry) => void;
  onOpenInFinder: (path: string) => void;
  onOpenInTerminal: (path: string) => void;
  onRemove: (path: string) => void;
  onRescan: (path: string) => void;
  /** Start the agent for this project (lands on the Agent tab). */
  onStart: (path: string) => void;
}

// ── Per-project gradient accents ────────────────────────────────────────────
// Deterministic hash of the project name → one of six gradient pairs, so each
// project gets a stable monogram colour identity (matches the clone's intent).
const ACCENT_PAIRS: ReadonlyArray<[string, string]> = [
  ["oklch(0.7 0.19 295)", "oklch(0.72 0.18 340)"], // violet → magenta
  ["oklch(0.72 0.18 200)", "oklch(0.75 0.17 160)"], // cyan → green
  ["oklch(0.72 0.16 60)", "oklch(0.75 0.18 30)"], // amber → red
  ["oklch(0.65 0.2 255)", "oklch(0.7 0.18 295)"], // blue → violet
  ["oklch(0.75 0.15 75)", "oklch(0.72 0.19 25)"], // yellow → red
  ["oklch(0.72 0.16 155)", "oklch(0.74 0.14 200)"], // green → cyan
];

function accentsFor(name: string): [string, string] {
  let hash = 0;
  for (let i = 0; i < name.length; i++) {
    hash = name.charCodeAt(i) + ((hash << 5) - hash);
  }
  return ACCENT_PAIRS[Math.abs(hash) % ACCENT_PAIRS.length];
}

// ── Run button ───────────────────────────────────────────────────────────────

const RUN_CONFIG: Record<
  RunState,
  { label: string; icon: React.ComponentType<{ className?: string }>; className: string; spin?: boolean }
> = {
  idle: {
    label: "Start",
    icon: Play,
    className: "bg-primary text-primary-foreground hover:opacity-90",
  },
  working: {
    label: "Working",
    icon: Loader2,
    className: "bg-blue-500/15 text-blue-600 dark:text-blue-400 hover:bg-blue-500/20",
    spin: true,
  },
  waiting: {
    label: "Waiting",
    icon: MessageCircleQuestion,
    className: "bg-amber-500/15 text-amber-700 dark:text-amber-400 hover:bg-amber-500/20",
  },
  done: {
    label: "Done",
    icon: Check,
    className: "bg-emerald-500/15 text-emerald-700 dark:text-emerald-400 hover:bg-emerald-500/20",
  },
};

function RunButton({
  state,
  onClick,
}: {
  state: RunState | undefined;
  onClick: (e: React.MouseEvent) => void;
}) {
  // Defensive: older backends may not emit `run_state`, and any unknown value
  // must not blow up the whole card. Fall back to `idle`.
  const cfg = RUN_CONFIG[state ?? "idle"] ?? RUN_CONFIG.idle;
  const Icon = cfg.icon;
  return (
    <button
      type="button"
      onClick={(e) => {
        e.preventDefault();
        e.stopPropagation();
        onClick(e);
      }}
      className={cn(
        "inline-flex h-7 items-center gap-1.5 rounded-md px-2.5 text-[11px] font-medium transition-colors",
        cfg.className,
      )}
    >
      <Icon className={cn("size-3.5", cfg.spin && "animate-spin")} />
      {cfg.label}
    </button>
  );
}

// ── Card ─────────────────────────────────────────────────────────────────────

export const ProjectCard = memo(function ProjectCard({
  project,
  onSelect,
  onOpenInFinder,
  onOpenInTerminal,
  onRemove,
  onRescan,
  onStart,
}: ProjectCardProps) {
  const [accentFrom, accentTo] = accentsFor(project.name);
  const monogram = project.name.charAt(0).toUpperCase() || "?";
  const { uncommitted } = project;
  const hasChanges = uncommitted.files > 0;

  return (
    <div
      role="button"
      tabIndex={0}
      onClick={() => onSelect(project)}
      onKeyDown={(e) => {
        if (e.key === "Enter" || e.key === " ") {
          e.preventDefault();
          onSelect(project);
        }
      }}
      className={cn(
        "card-accent-top group relative flex cursor-pointer flex-col rounded-xl border border-border bg-card p-5 transition-all duration-150",
        "hover:-translate-y-px hover:border-primary/40 hover:shadow-[var(--shadow-md)]",
        "focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring",
      )}
      style={
        {
          ["--tw-gradient-from"]: accentFrom,
          ["--tw-gradient-to"]: accentTo,
        } as React.CSSProperties
      }
    >
      {/* Header: monogram + name + run button */}
      <div className="flex items-start gap-3">
        <div
          className="flex size-9 shrink-0 items-center justify-center rounded-lg text-sm font-semibold text-white"
          style={{ background: `linear-gradient(135deg, ${accentFrom}, ${accentTo})` }}
        >
          {monogram}
        </div>
        <div className="min-w-0 flex-1">
          <div className="flex items-center gap-1">
            <h3 className="truncate text-sm font-semibold tracking-tight">{project.name}</h3>
            <ArrowUpRight className="size-3.5 text-muted-foreground opacity-0 transition-opacity group-hover:opacity-100" />
          </div>
          <p className="text-[11px] text-muted-foreground">
            {project.last_opened ? `Opened ${relativeTime(project.last_opened)}` : "Never opened"}
          </p>
        </div>
        <RunButton state={project.run_state} onClick={() => onStart(project.path)} />
      </div>

      {/* Description */}
      {project.description && (
        <p className="mt-4 line-clamp-2 text-xs leading-relaxed text-muted-foreground">
          {project.description}
        </p>
      )}

      {/* Activity panel */}
      <div className="mt-4 space-y-1.5 rounded-md bg-muted/40 p-2.5 text-[11px]">
        {/* Last commit */}
        <div className="flex items-start gap-2 text-muted-foreground">
          <GitCommitHorizontal className="mt-0.5 size-3 shrink-0" />
          <div className="min-w-0 flex-1">
            <span className="text-foreground/80">Last commit</span>
            {project.last_commit_date ? ` · ${relativeTime(project.last_commit_date)}` : " · none"}
            <div className="truncate font-mono text-[10.5px] text-muted-foreground/90">
              {project.last_commit_message ?? "Uncommitted"}
            </div>
          </div>
        </div>

        {/* Folder modified */}
        {project.last_modified && (
          <div className="flex items-center gap-2 text-muted-foreground">
            <Clock className="size-3 shrink-0" />
            <span className="text-foreground/80">Folder modified</span> ·{" "}
            {relativeTime(project.last_modified)}
          </div>
        )}

        {/* Uncommitted diff */}
        <div className="flex items-center gap-2 text-muted-foreground">
          <FileDiff className="size-3 shrink-0" />
          {hasChanges ? (
            <>
              <span className="text-foreground/80">Uncommitted</span> ·{" "}
              <span>
                {uncommitted.files} {uncommitted.files === 1 ? "file" : "files"}
              </span>
              <span className="font-mono text-emerald-600 dark:text-emerald-400">
                +{uncommitted.added}
              </span>
              <span className="font-mono text-rose-600 dark:text-rose-400">
                −{uncommitted.deleted}
              </span>
            </>
          ) : (
            <>
              <span className="text-foreground/80">Working tree</span> · clean
            </>
          )}
        </div>

        {/* Current loop */}
        {project.current_loop && (
          <div className="flex items-center gap-2 text-muted-foreground">
            <RotateCw className="size-3 shrink-0" />
            <span className="text-foreground/80">Current loop</span> · {project.current_loop}
          </div>
        )}
      </div>

      {/* Status */}
      <div className="mt-4">
        <StatusBadge status={project.status} />
      </div>

      {/* Action footer */}
      <div className="mt-4 flex items-center justify-around border-t border-border pt-3">
        {([
          { icon: RefreshCw, title: "Rescan", onClick: onRescan },
          { icon: FolderOpen, title: "Open in Finder", onClick: onOpenInFinder },
          { icon: Terminal, title: "Open in Terminal", onClick: onOpenInTerminal },
          { icon: Trash2, title: "Remove", onClick: onRemove, destructive: true },
        ] as Array<{
          icon: React.ComponentType<{ className?: string }>;
          title: string;
          onClick: (path: string) => void;
          destructive?: boolean;
        }>).map(({ icon: Icon, title, onClick, destructive }) => (
          <button
            key={title}
            type="button"
            title={title}
            aria-label={title}
            onClick={(e) => {
              e.preventDefault();
              e.stopPropagation();
              onClick(project.path);
            }}
            className={cn(
              "flex size-8 items-center justify-center rounded-md text-muted-foreground transition-colors hover:bg-accent hover:text-foreground",
              destructive && "hover:bg-destructive/10 hover:text-destructive",
            )}
          >
            <Icon className="size-3.5" />
          </button>
        ))}
      </div>
    </div>
  );
});
