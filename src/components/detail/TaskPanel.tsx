import { useMemo, useState } from "react";
import { ListTodo, ChevronDown, ChevronUp, Check, X, Plus, Pencil, Loader2, Circle } from "lucide-react";
import type { TaskRecord } from "../../types";
import { useStreamingState } from "../../store/streamingState";

/**
 * Floating "current tasks" panel for the in-flight agent turn.
 *
 * The transcript renders every `task_update` event as an arrival-ordered
 * activity row — great for "what happened when," useless for "what's the state
 * of the agent's todo list right now." This panel is the latter view: it shows
 * the latest known status of each task id, deduped (a created→updated→completed
 * sequence collapses to a single "completed" row), parked top-right where it
 * stays visible without scrolling — same "the important thing should float"
 * principle as the AskUserQuestion / approval cards.
 *
 * Source of truth is `streamingState.tasks[path]`, a latest-wins-by-id map fed
 * by the `task_update` channel events (see `AgentPanel.runStreamingTurn`).
 * Because it lives in the navigation-stable Zustand store, the panel survives
 * `AgentPanel` unmount — navigate away and back mid-turn and the task list is
 * still there.
 *
 * Visibility:
 * - Hidden entirely when there are no tasks (no in-flight turn, or a turn with
 *   no TodoWrite activity). The panel is decoration, not chrome — an empty
 *   pinned box would just eat screen.
 * - Collapsible (defaults collapsed to a one-line summary badge) so a long task
 *   list never dominates the viewport. Click the header to expand.
 */
export function TaskPanel({ projectPath }: { projectPath: string }) {
  const tasksMap = useStreamingState((s) => s.byPath[projectPath]?.tasks ?? {});
  const [open, setOpen] = useState(false);

  // De-dup is already done in the store (latest per id); we just sort for
  // display: numeric by task id so #2 follows #1, not string-sorted ("10" < "2").
  const tasks = useMemo(() => {
    return Object.values(tasksMap).sort((a, b) => {
      const ai = parseInt(a.id, 10);
      const bi = parseInt(b.id, 10);
      const an = Number.isNaN(ai) ? Infinity : ai;
      const bn = Number.isNaN(bi) ? Infinity : bi;
      return an - bn;
    });
  }, [tasksMap]);

  if (tasks.length === 0) return null;

  const counts = countByStatus(tasks);
  const total = tasks.length;
  const done = counts.completed ?? 0;

  return (
    <div className="pointer-events-auto w-72 shrink-0 rounded-lg border border-border bg-popover/95 backdrop-blur shadow-lg">
      {/* Header — click to toggle. Always shows progress (done / total) so the
          collapsed state is still informative. */}
      <button
        onClick={() => setOpen((v) => !v)}
        className="flex w-full items-center gap-2 px-3 py-2 text-left transition-colors hover:bg-accent/50"
      >
        <ListTodo className="size-3.5 shrink-0 text-primary" />
        <span className="text-xs font-semibold flex-1">Tasks</span>
        {/* Progress badge: done/total, with a subtle bar when there's >1. */}
        <span className="text-[10px] font-mono text-muted-foreground tabular-nums">
          {done}/{total}
        </span>
        {open ? (
          <ChevronUp className="size-3 text-muted-foreground" />
        ) : (
          <ChevronDown className="size-3 text-muted-foreground" />
        )}
      </button>

      {/* Progress bar — slim, under the header. Visually communicates how much
          of the turn's plan is done without forcing the user to expand. */}
      <div className="h-0.5 w-full bg-border/60">
        <div
          className="h-full bg-primary transition-all duration-300"
          style={{ width: `${total === 0 ? 0 : (done / total) * 100}%` }}
        />
      </div>

      {/* Body — only when expanded. Scrollable so a big todo list doesn't blow
          out the panel height; the panel pins, the content scrolls inside. */}
      {open && (
        <ul className="max-h-64 overflow-y-auto p-1.5 space-y-0.5">
          {tasks.map((t) => (
            <TaskRow key={t.id} task={t} />
          ))}
        </ul>
      )}
    </div>
  );
}

/** A single task row with a status-coloured glyph and the subject. */
function TaskRow({ task }: { task: TaskRecord }) {
  const { icon, cls } = statusVisual(task.status);
  return (
    <li className="flex items-start gap-2 rounded-md px-2 py-1.5 text-xs leading-relaxed transition-colors hover:bg-accent/40">
      <span className={`mt-0.5 shrink-0 ${cls}`}>{icon}</span>
      <div className="min-w-0 flex-1">
        <div className="flex items-center gap-1.5">
          <span className="font-mono text-[10px] text-muted-foreground">#{task.id}</span>
          <span
            className={`text-[9px] font-semibold uppercase tracking-wide ${statusBadgeCls(task.status)}`}
          >
            {task.status}
          </span>
        </div>
        <div className="text-foreground/90 break-words">{task.subject}</div>
      </div>
    </li>
  );
}

/** Pick the glyph icon for a task status. */
function statusVisual(status: string): { icon: React.ReactNode; cls: string } {
  switch (status) {
    case "completed":
      return { icon: <Check className="size-3" strokeWidth={3} />, cls: "text-emerald-500" };
    case "in_progress":
      // The "actively working on it" state — the one genuinely new glyph TaskUpdate
      // introduces. A spinner reads instantly without needing the word label.
      return { icon: <Loader2 className="size-3 animate-spin" />, cls: "text-primary" };
    case "pending":
      return { icon: <Circle className="size-3" />, cls: "text-muted-foreground" };
    case "created":
      return { icon: <Plus className="size-3" strokeWidth={3} />, cls: "text-primary" };
    case "deleted":
      return { icon: <X className="size-3" strokeWidth={3} />, cls: "text-destructive" };
    case "updated":
    default:
      return { icon: <Pencil className="size-3" />, cls: "text-amber-500" };
  }
}

/** Tailwind classes for the status word badge (the uppercase "COMPLETED" tag). */
function statusBadgeCls(status: string): string {
  switch (status) {
    case "completed":
      return "text-emerald-600 dark:text-emerald-400";
    case "in_progress":
      return "text-primary";
    case "pending":
      return "text-muted-foreground";
    case "created":
      return "text-primary";
    case "deleted":
      return "text-destructive";
    case "updated":
    default:
      return "text-amber-600 dark:text-amber-400";
  }
}

/** Count tasks grouped by status — used for the header progress math. */
function countByStatus(tasks: TaskRecord[]): Record<string, number> {
  const out: Record<string, number> = {};
  for (const t of tasks) {
    out[t.status] = (out[t.status] ?? 0) + 1;
  }
  return out;
}
