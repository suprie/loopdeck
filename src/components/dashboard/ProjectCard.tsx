import { memo } from "react";
import {
  Folder,
  Terminal,
  Trash2,
  RefreshCw,
  GitCommit,
  Clock,
  Circle,
  ArrowUpRight,
} from "lucide-react";
import type { ProjectEntry } from "../../types";
import { relativeTime } from "../../lib/time";

interface ProjectCardProps {
  project: ProjectEntry;
  onSelect: (project: ProjectEntry) => void;
  onOpenInFinder: (path: string) => void;
  onOpenInTerminal: (path: string) => void;
  onRemove: (path: string) => void;
  onRescan: (path: string) => void;
}

const ACCENTS = [
  "oklch(0.72 0.17 295)", // purple
  "oklch(0.74 0.16 155)", // green
  "oklch(0.78 0.15 75)",  // yellow
  "oklch(0.65 0.2 255)",  // blue
  "oklch(0.65 0.22 25)",  // red
  "oklch(0.7 0.18 180)",  // teal
];

function accentFor(name: string): string {
  let hash = 0;
  for (let i = 0; i < name.length; i++) {
    hash = name.charCodeAt(i) + ((hash << 5) - hash);
  }
  return ACCENTS[Math.abs(hash) % ACCENTS.length];
}

// Freshness derives from project.status (computed by Rust backend via update_project_status).
// active → green (0-6 days), warning → yellow (7-30 days), nonactive → red (30+ days)
type Freshness = "fresh" | "warning" | "danger";

function freshnessFromStatus(status: string): Freshness {
  switch (status) {
    case "active":
      return "fresh";
    case "warning":
      return "warning";
    case "nonactive":
      return "danger";
    default:
      return "fresh";
  }
}

const freshnessConfig: Record<Freshness, { bg: string }> = {
  fresh: {
    bg: "bg-muted/40 border-transparent",
  },
  warning: {
    bg: "bg-[color-mix(in_oklab,var(--warning)_10%,transparent)] border-[color-mix(in_oklab,var(--warning)_28%,transparent)]",
  },
  danger: {
    bg: "bg-[color-mix(in_oklab,var(--destructive)_12%,transparent)] border-[color-mix(in_oklab,var(--destructive)_30%,transparent)]",
  },
};

const statusConfig: Record<string, { color: string; label: string }> = {
  active: { color: "text-[var(--success)]", label: "Active" },
  warning: { color: "text-[var(--warning)]", label: "Warning" },
  nonactive: { color: "text-[var(--destructive)]", label: "Inactive" },
  archived: { color: "text-muted-foreground", label: "Archived" },
};

export const ProjectCard = memo(function ProjectCard({
  project,
  onSelect,
  onOpenInFinder,
  onOpenInTerminal,
  onRemove,
  onRescan,
}: ProjectCardProps) {
  const accent = accentFor(project.name);
  const freshness = freshnessFromStatus(project.status);
  const commitCfg = freshnessConfig[freshness];
  const status = statusConfig[project.status] ?? {
    color: "text-muted-foreground",
    label: project.status,
  };

  return (
    <div
      className="group relative rounded-xl border border-border bg-card p-5 hover:border-primary/40 hover:bg-surface-elevated transition-all flex flex-col cursor-pointer"
      onClick={() => onSelect(project)}
    >
      {/* Gradient top accent */}
      <div
        className="absolute inset-x-0 top-0 h-px rounded-t-xl"
        style={{
          background: `linear-gradient(90deg, transparent, ${accent}, transparent)`,
        }}
      />

      {/* Header: icon + name */}
      <div className="flex items-start justify-between mb-3">
        <div className="flex items-center gap-2.5">
          <div
            className="size-9 rounded-lg grid place-items-center text-sm font-semibold flex-shrink-0"
            style={{
              background: `color-mix(in oklab, ${accent} 18%, transparent)`,
              color: accent,
            }}
          >
            {project.name.charAt(0).toUpperCase()}
          </div>
          <div className="min-w-0">
            <div className="font-semibold text-sm truncate">{project.name}</div>
            {project.last_opened && (
              <div className="text-[11px] text-muted-foreground">
                Opened {relativeTime(project.last_opened)}
              </div>
            )}
          </div>
        </div>
        <ArrowUpRight className="size-4 text-muted-foreground group-hover:text-foreground transition shrink-0" />
      </div>

      {/* Description */}
      {project.description && (
        <p className="text-xs text-muted-foreground line-clamp-2 mb-4">
          {project.description}
        </p>
      )}

      {/* Git activity */}
      <div className="space-y-1.5 mb-3">
        {/* Last commit */}
        <div
          className={`flex items-start gap-2 rounded-md border px-2.5 py-1.5 ${commitCfg.bg}`}
        >
          <GitCommit className="size-3.5 mt-0.5 shrink-0 text-muted-foreground" />
          <div className="min-w-0 flex-1">
            <div className="text-[10px] uppercase tracking-wider text-muted-foreground">
              Last commit
              {project.last_commit_date && ` · ${relativeTime(project.last_commit_date)}`}
            </div>
            <div className="text-xs truncate font-mono">
              {project.last_commit_message ?? "Uncommitted"}
            </div>
          </div>
        </div>

        {/* Last modified */}
        {project.last_modified && (
          <div
            className={`flex items-start gap-2 rounded-md border px-2.5 py-1.5 ${commitCfg.bg}`}
          >
            <Clock className="size-3.5 mt-0.5 shrink-0 text-muted-foreground" />
            <div className="min-w-0 flex-1">
              <div className="text-[10px] uppercase tracking-wider text-muted-foreground">
                Folder modified
              </div>
              <div className="text-xs">{relativeTime(project.last_modified)}</div>
            </div>
          </div>
        )}
        {/* Current Loop */}
        {project.current_loop && (
          <div
            className={`flex items-start gap-2 rounded-md border px-2.5 py-1.5 ${commitCfg.bg}`}
          >
            <Clock className="size-3.5 mt-0.5 shrink-0 text-muted-foreground" />
            <div className="min-w-0 flex-1">
              <div className="text-[10px] uppercase tracking-wider text-muted-foreground">
                Current Loop
              </div>
              <div className="text-xs">{ project.current_loop}</div>
            </div>
          </div>
        )}
       </div>

      {/* Status badge */}
      <div className="flex items-center gap-1.5 text-[11px] mt-auto mb-3">
        <Circle className={`size-2 fill-current ${status.color}`} strokeWidth={0} />
        <span className="text-muted-foreground">{status.label}</span>
      </div>

      {/* Actions */}
      <div className="flex gap-1.5 pt-3 border-t border-border/60">
        <button
          className="flex-1 inline-flex items-center justify-center gap-1 h-7 rounded-md bg-muted/60 text-muted-foreground text-[11px] font-medium hover:bg-accent hover:text-foreground transition"
          onClick={(e) => {
            e.stopPropagation();
            onRescan(project.path);
          }}
          title="Rescan"
        >
          <RefreshCw className="size-3" />
        </button>
        <button
          className="flex-1 inline-flex items-center justify-center gap-1 h-7 rounded-md bg-muted/60 text-muted-foreground text-[11px] font-medium hover:bg-accent hover:text-foreground transition"
          onClick={(e) => {
            e.stopPropagation();
            onOpenInFinder(project.path);
          }}
          title="Open in Finder"
        >
          <Folder className="size-3" />
        </button>
        <button
          className="flex-1 inline-flex items-center justify-center gap-1 h-7 rounded-md bg-muted/60 text-muted-foreground text-[11px] font-medium hover:bg-accent hover:text-foreground transition"
          onClick={(e) => {
            e.stopPropagation();
            onOpenInTerminal(project.path);
          }}
          title="Open in Terminal"
        >
          <Terminal className="size-3" />
        </button>
        <button
          className="inline-flex items-center justify-center gap-1 h-7 w-7 rounded-md bg-muted/60 text-muted-foreground text-[11px] font-medium hover:bg-[color-mix(in_oklab,var(--destructive)_12%,transparent)] hover:text-destructive transition"
          onClick={(e) => {
            e.stopPropagation();
            onRemove(project.path);
          }}
          title="Remove"
        >
          <Trash2 className="size-3" />
        </button>
      </div>
    </div>
  );
});
