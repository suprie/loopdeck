import { Link } from "@tanstack/react-router";
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

import { StatusBadge } from "./status-badge";
import type { Project, RunState } from "@/lib/mock-data";
import { cn } from "@/lib/utils";

const runConfig: Record<
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
    className:
      "bg-amber-500/15 text-amber-700 dark:text-amber-400 hover:bg-amber-500/20",
  },
  done: {
    label: "Done",
    icon: Check,
    className:
      "bg-emerald-500/15 text-emerald-700 dark:text-emerald-400 hover:bg-emerald-500/20",
  },
};

function RunButton({ state }: { state: RunState }) {
  const cfg = runConfig[state];
  const Icon = cfg.icon;
  return (
    <button
      type="button"
      onClick={(e) => {
        e.preventDefault();
        e.stopPropagation();
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

export function ProjectCard({ project }: { project: Project }) {
  const { uncommitted } = project;
  const hasChanges = uncommitted.files > 0;

  return (
    <Link
      to="/projects/$id"
      params={{ id: project.id }}
      className="group card-accent-top relative flex flex-col rounded-xl border border-border bg-card p-5 shadow-[var(--shadow-sm)] transition-all duration-150 hover:-translate-y-px hover:border-primary/40 hover:shadow-[var(--shadow-md)]"
      style={
        {
          ["--tw-gradient-from" as string]: project.accentFrom,
          ["--tw-gradient-to" as string]: project.accentTo,
        } as React.CSSProperties
      }
    >
      <div className="flex items-start gap-3">
        <div
          className="flex size-9 items-center justify-center rounded-lg text-sm font-semibold"
          style={{
            background: `linear-gradient(135deg, ${project.accentFrom}, ${project.accentTo})`,
            color: "white",
          }}
        >
          {project.monogram}
        </div>
        <div className="flex-1 min-w-0">
          <div className="flex items-center gap-1">
            <h3 className="truncate text-sm font-semibold tracking-tight">{project.name}</h3>
            <ArrowUpRight className="size-3.5 text-muted-foreground opacity-0 transition-opacity group-hover:opacity-100" />
          </div>
          <p className="text-[11px] text-muted-foreground">Opened {project.openedAgo}</p>
        </div>
        <RunButton state={project.runState} />
      </div>

      <p className="mt-4 line-clamp-2 text-xs leading-relaxed text-muted-foreground">
        {project.description}
      </p>

      <div className="mt-4 space-y-1.5 rounded-md bg-muted/40 p-2.5 text-[11px]">
        <div className="flex items-start gap-2 text-muted-foreground">
          <GitCommitHorizontal className="mt-0.5 size-3 shrink-0" />
          <div className="min-w-0 flex-1">
            <span className="text-foreground/80">Last commit</span> · {project.lastCommit.ago}
            <div className="truncate font-mono text-[10.5px] text-muted-foreground/90">
              {project.lastCommit.message}
            </div>
          </div>
        </div>
        <div className="flex items-center gap-2 text-muted-foreground">
          <Clock className="size-3 shrink-0" />
          <span className="text-foreground/80">Folder modified</span> · {project.lastModified}
        </div>
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
        {project.currentLoop && (
          <div className="flex items-center gap-2 text-muted-foreground">
            <RotateCw className="size-3 shrink-0" />
            <span className="text-foreground/80">Current loop</span> · {project.currentLoop}
          </div>
        )}
      </div>

      <div className="mt-4">
        <StatusBadge status={project.status} />
      </div>

      <div className="mt-4 flex items-center justify-around border-t border-border pt-3">
        {[RefreshCw, FolderOpen, Terminal, Trash2].map((Icon, i) => (
          <button
            key={i}
            type="button"
            onClick={(e) => e.preventDefault()}
            className="flex size-8 items-center justify-center rounded-md text-muted-foreground transition-colors hover:bg-accent hover:text-foreground"
          >
            <Icon className="size-3.5" />
          </button>
        ))}
      </div>
    </Link>
  );
}
