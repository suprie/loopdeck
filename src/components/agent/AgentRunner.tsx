import { useState, useMemo, useEffect } from "react";
import { Bot, Circle, ChevronRight, Clock, Hash, Plus } from "lucide-react";
import { useAppStore } from "../../store/appStore";
import { useProjects } from "../../hooks/useProjects";
import { AgentPanel } from "../detail/AgentPanel";
import { PageHeader } from "../layout/AppShell";
import { relativeTime } from "../../lib/time";
import { cn } from "../../lib/utils";

/**
 * Top-level Agent page.
 *
 * Replaces the legacy terminal runner with the same chat surface used inside
 * ProjectDetail's Agent tab (`AgentPanel`). Layout: a left project-picker
 * (sorted so projects with live agent activity float to the top) and the chat
 * surface on the right. Selecting a project mounts `AgentPanel` against it,
 * which owns the streaming orchestration, conversation history, and the
 * AskUserQuestion / manual-approval cards.
 *
 * The picker shows live/idle state from `ProjectEntry.run_state` (derived
 * server-side from the in-flight session + any pending approval/question) so
 * the user can see at a glance which projects have a turn running or are
 * parked waiting on them.
 */
/**
 * Module-level last selection. Survives unmount/remount when the user navigates
 * away from /agent and back, so they don't lose their picked project. Falls
 * back to the first project if the saved one is gone.
 */
let lastSelectedPath: string | null = null;

export function AgentRunner() {
  const projects = useAppStore((s) => s.projects);
  const [selectedPath, setSelectedPath] = useState<string | null>(
    lastSelectedPath && projects.some((p) => p.path === lastSelectedPath)
      ? lastSelectedPath
      : (projects[0]?.path ?? null),
  );

  // Persist the selection across remounts.
  useEffect(() => {
    if (selectedPath) lastSelectedPath = selectedPath;
  }, [selectedPath]);

  const { scanFolder } = useProjects();

  /** Sort: working → waiting → idle/done, then by name. Keeps actionable
   *  projects at the top so the user lands on something alive. */
  const ordered = useMemo(() => {
    const rank: Record<string, number> = { working: 0, waiting: 1, idle: 2, done: 2 };
    return [...projects].sort((a, b) => {
      const ra = rank[a.run_state] ?? 2;
      const rb = rank[b.run_state] ?? 2;
      if (ra !== rb) return ra - rb;
      return a.name.localeCompare(b.name);
    });
  }, [projects]);

  const selected = ordered.find((p) => p.path === selectedPath) ?? ordered[0] ?? null;

  async function handleScan() {
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
  }

  return (
    <div className="flex flex-1 flex-col min-h-0">
      <PageHeader
        title="Agent Runner"
        subtitle="Chat with the agent across any project"
        actions={
          <button
            type="button"
            onClick={handleScan}
            className="inline-flex h-8 items-center gap-1.5 rounded-md border border-border bg-transparent px-3 text-xs font-medium text-muted-foreground transition-colors hover:bg-accent hover:text-foreground"
          >
            <Plus className="size-3.5" />
            Import Repo
          </button>
        }
      />

      {projects.length === 0 ? (
        <EmptyProjects onScan={handleScan} />
      ) : (
        <div className="flex min-h-0 flex-1">
          {/* ── Project picker ── */}
          <aside className="flex w-64 shrink-0 flex-col border-r border-border bg-surface/60">
            <div className="flex h-10 items-center gap-2 border-b border-border px-4">
              <Bot className="size-3.5 text-muted-foreground" />
              <span className="text-xs font-semibold tracking-tight">Projects</span>
            </div>

            <div className="flex-1 overflow-y-auto">
              {ordered.map((p) => {
                const isActive = p.path === selected?.path;
                const isLive = p.run_state === "working" || p.run_state === "waiting";
                return (
                  <button
                    key={p.path}
                    onClick={() => setSelectedPath(p.path)}
                    className={cn(
                      "w-full border-b border-border/50 px-4 py-2.5 text-left transition-colors",
                      isActive
                        ? "bg-accent text-foreground"
                        : "text-muted-foreground hover:bg-accent/30 hover:text-foreground",
                    )}
                  >
                    <div className="mb-0.5 flex items-center gap-2">
                      <Circle
                        className={cn(
                          "size-2 shrink-0",
                          p.run_state === "working" && "fill-blue-500 text-blue-500",
                          p.run_state === "waiting" && "fill-amber-500 text-amber-500",
                          (p.run_state === "idle" || p.run_state === "done") &&
                            "fill-muted-foreground/30 text-muted-foreground/30",
                        )}
                      />
                      <span className="flex-1 truncate text-xs font-medium">{p.name}</span>
                      {isLive && (
                        <span className="text-[9px] font-semibold uppercase tracking-wider text-primary">
                          {p.run_state}
                        </span>
                      )}
                      <ChevronRight className="size-3 opacity-30" />
                    </div>
                    <div className="flex items-center gap-2 text-[10px] text-muted-foreground">
                      {p.last_opened ? (
                        <>
                          <Clock className="size-2.5" />
                          <span>{relativeTime(p.last_opened)}</span>
                        </>
                      ) : (
                        <span className="italic">Never opened</span>
                      )}
                    </div>
                    <div className="mt-0.5 truncate font-mono text-[9px] text-muted-foreground/50">
                      {p.path}
                    </div>
                  </button>
                );
              })}
            </div>

            <div className="flex items-center justify-between border-t border-border px-4 py-2 text-[10px] text-muted-foreground">
              <span>
                {projects.length} project{projects.length !== 1 ? "s" : ""}
              </span>
              <span>{ordered.filter((p) => p.run_state !== "idle").length} active</span>
            </div>
          </aside>

          {/* ── Chat surface ── */}
          <main className="min-w-0 flex-1 p-6">
            {selected ? (
              <AgentPanel key={selected.path} projectPath={selected.path} />
            ) : (
              <EmptyProjects onScan={handleScan} embedded />
            )}
          </main>
        </div>
      )}
    </div>
  );
}

/** Empty-state shown when no projects are registered yet. */
function EmptyProjects({
  onScan,
  embedded,
}: {
  onScan: () => void;
  embedded?: boolean;
}) {
  return (
    <div
      className={cn(
        "flex flex-col items-center justify-center text-center",
        embedded ? "h-full py-12" : "flex-1 py-24",
      )}
    >
      <Hash className="mb-3 size-8 text-muted-foreground/30" />
      <h3 className="text-sm font-semibold text-foreground">No projects registered</h3>
      <p className="mt-1 max-w-xs text-xs leading-relaxed text-muted-foreground">
        Import a repository to start a conversation with its agent.
      </p>
      <button
        type="button"
        onClick={onScan}
        className="mt-5 inline-flex h-8 items-center gap-1.5 rounded-md bg-primary px-3 text-xs font-medium text-primary-foreground transition-opacity hover:opacity-90"
      >
        <Plus className="size-3.5" />
        Import Repo
      </button>
    </div>
  );
}
