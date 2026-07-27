import { useState, useEffect, useCallback } from "react";
import {
  Repeat,
  Circle,
  CheckCircle2,
  History,
  ArrowRight,
  Loader2,
  AlertTriangle,
} from "lucide-react";
import { toast } from "sonner";
import type {
  LoopStatus,
  Loop,
  LoadedExecution,
  HistoryLoop,
  QueuedLoop,
  MigrationPreview,
} from "../../types";
import * as api from "../../lib/tauri";
import { LoadingSpinner } from "../shared/LoadingSpinner";
import { Markdown } from "../shared/Markdown";
import { MigrationCard } from "./MigrationCard";

interface LoopsPanelProps {
  projectPath: string;
}

const LOOP_STATUS_COLORS: Record<string, string> = {
  in_progress: "var(--primary)",
  queued: "var(--warning)",
  completed: "var(--success)",
  abandoned: "var(--muted-foreground)",
};

const LOOP_STATUS_BG: Record<string, string> = {
  in_progress: "bg-[color-mix(in_oklab,var(--primary)_12%,transparent)]",
  queued: "bg-[color-mix(in_oklab,var(--warning)_12%,transparent)]",
  completed: "bg-[color-mix(in_oklab,var(--success)_12%,transparent)]",
  abandoned: "bg-muted",
};

/** A loop in legacy `.loopdeck/loops.md` (goal/status/completed). */
function LoopCard({ loop, isCurrent }: { loop: Loop; isCurrent?: boolean }) {
  return (
    <div
      className={`rounded-lg border p-3.5 ${
        isCurrent
          ? "border-[var(--primary)] bg-[color-mix(in_oklab,var(--primary)_5%,transparent)]"
          : "border-border bg-card"
      }`}
    >
      <div className="flex items-center gap-2 mb-1.5 flex-wrap">
        <span className="text-[11px] font-mono text-muted-foreground">
          {loop.started}
        </span>
        <span
          className={`text-[10px] font-semibold uppercase tracking-wider px-1.5 py-0.5 rounded ${LOOP_STATUS_BG[loop.status] ?? "bg-muted"}`}
          style={{
            color: LOOP_STATUS_COLORS[loop.status] ?? "var(--muted-foreground)",
          }}
        >
          {loop.status.replace("_", " ")}
        </span>
        {loop.completed && (
          <span className="text-[10px] text-muted-foreground flex items-center gap-1">
            <ArrowRight size={10} /> {loop.completed}
          </span>
        )}
      </div>
      <div className="text-sm font-semibold text-foreground leading-snug">
        <Markdown>{loop.goal}</Markdown>
      </div>
    </div>
  );
}

/** The YYYY-MM-DD portion of an RFC3339 timestamp (the legacy display shape). */
function dateOnly(iso: string): string {
  return iso.slice(0, 10);
}

/** A structured loop from `execution.yaml`, keyed by stable ID. */
function StructuredCard({
  loop,
  status,
  date,
  isCurrent,
}: {
  loop: { id: string; title: string };
  status: "in_progress" | "queued" | "completed" | "abandoned";
  date: string;
  isCurrent?: boolean;
}) {
  return (
    <div
      className={`rounded-lg border p-3.5 ${
        isCurrent
          ? "border-[var(--primary)] bg-[color-mix(in_oklab,var(--primary)_5%,transparent)]"
          : "border-border bg-card"
      }`}
    >
      <div className="flex items-center gap-2 mb-1.5 flex-wrap">
        <span className="text-[11px] font-mono text-muted-foreground">{date}</span>
        <span
          className={`text-[10px] font-semibold uppercase tracking-wider px-1.5 py-0.5 rounded ${LOOP_STATUS_BG[status] ?? "bg-muted"}`}
          style={{ color: LOOP_STATUS_COLORS[status] ?? "var(--muted-foreground)" }}
        >
          {status.replace("_", " ")}
        </span>
      </div>
      <div className="text-sm font-semibold text-foreground leading-snug">
        {loop.title}
      </div>
      <div className="mt-1 text-[10px] font-mono text-muted-foreground/70">
        {loop.id}
      </div>
    </div>
  );
}

function StructuredView({ loaded }: { loaded: LoadedExecution }) {
  const s = loaded.state;
  const current = s.current;
  const isEmpty =
    !current && s.queue.length === 0 && s.history.length === 0;
  return (
    <div className="max-w-2xl">
      {loaded.warnings.length > 0 && (
        <div className="mb-4 flex items-start gap-2 rounded-md border border-[color-mix(in_oklab,var(--warning)_40%,transparent)] bg-[color-mix(in_oklab,var(--warning)_8%,transparent)] p-2.5 text-[11px] text-muted-foreground">
          <AlertTriangle
            size={13}
            className="mt-0.5 shrink-0"
            style={{ color: "var(--warning)" }}
          />
          <div>
            {loaded.warnings.map((w, i) => (
              <p key={i}>{w}</p>
            ))}
          </div>
        </div>
      )}

      {current && (
        <section className="mb-6">
          <h3 className="text-[11px] font-semibold uppercase tracking-wider text-muted-foreground mb-3">
            Current Loop
          </h3>
          <StructuredCard
            loop={current}
            status="in_progress"
            date={dateOnly(current.started_at)}
            isCurrent
          />
        </section>
      )}

      {s.queue.length > 0 && (
        <section className="mb-6">
          <h3 className="text-[11px] font-semibold uppercase tracking-wider text-muted-foreground mb-3">
            Queued ({s.queue.length})
          </h3>
          <div className="space-y-2">
            {s.queue.map((q: QueuedLoop) => (
              <StructuredCard
                key={q.id}
                loop={q}
                status="queued"
                date={dateOnly(q.queued_at)}
              />
            ))}
          </div>
        </section>
      )}

      {s.history.length > 0 && (
        <section>
          <h3 className="text-[11px] font-semibold uppercase tracking-wider text-muted-foreground mb-3 flex items-center gap-1.5">
            <History size={13} />
            History ({s.history.length})
          </h3>
          <div className="space-y-2">
            {s.history.map((h: HistoryLoop) => (
              <StructuredCard
                key={`${h.id}-${h.attempt}`}
                loop={h}
                status={h.outcome}
                date={dateOnly(h.completed_at)}
              />
            ))}
          </div>
        </section>
      )}

      {isEmpty && (
        <div className="flex flex-col items-center justify-center py-16 text-center">
          <Repeat size={32} className="text-muted-foreground/30 mb-3" />
          <h3 className="text-sm font-semibold text-foreground mb-1.5">
            No loops recorded
          </h3>
          <p className="text-xs text-muted-foreground max-w-xs leading-relaxed">
            This project uses the structured{" "}
            <code className="font-mono text-[11px] bg-muted px-1 py-0.5 rounded">
              execution.yaml
            </code>{" "}
            store. Promote a loop from the Epics tab to begin.
          </p>
        </div>
      )}
    </div>
  );
}

type Mode = "structured" | "legacy" | "empty";

export function LoopsPanel({ projectPath }: LoopsPanelProps) {
  const [loading, setLoading] = useState(true);
  const [mode, setMode] = useState<Mode>("empty");
  const [exec, setExec] = useState<LoadedExecution | null>(null);
  const [status, setStatus] = useState<LoopStatus | null>(null);
  const [preview, setPreview] = useState<MigrationPreview | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [toggling, setToggling] = useState<string | null>(null);

  const reload = useCallback(async () => {
    setError(null);
    try {
      const loaded = await api.getExecutionState(projectPath);
      if (loaded.file_present) {
        setExec(loaded);
        setStatus(null);
        setPreview(null);
        setMode("structured");
        return;
      }
      // No execution.yaml yet → legacy loops.md (or empty).
      setExec(null);
      const loops = await api.getLoops(projectPath);
      setStatus(loops);
      // Offer migration whenever loops.md exists, including empty and
      // next-step-only legacy files.
      try {
        const p = await api.getMigrationPreview(projectPath);
        const available =
          p.loops_md_present && !p.execution_yaml_present;
        setPreview(available ? p : null);
        setMode(
          available || loops.current || loops.history.length > 0
            ? "legacy"
            : "empty",
        );
      } catch {
        setPreview(null);
        setMode(
          loops.current || loops.history.length > 0 ? "legacy" : "empty",
        );
      }
    } catch (err) {
      setError(String(err));
    }
  }, [projectPath]);

  useEffect(() => {
    let cancelled = false;
    setLoading(true);
    reload().finally(() => {
      if (!cancelled) setLoading(false);
    });
    return () => {
      cancelled = true;
    };
  }, [reload]);

  const handleToggle = useCallback(
    async (stepText: string) => {
      setToggling(stepText);
      try {
        const nowChecked = await api.toggleLoopStep(projectPath, stepText);
        // Optimistic local update so the check flips immediately.
        setStatus((prev) =>
          prev
            ? {
                ...prev,
                next_steps: prev.next_steps.map((s) =>
                  s.text === stepText ? { ...s, checked: nowChecked } : s,
                ),
              }
            : prev,
        );
      } catch (err) {
        toast.error("Failed to toggle step", { description: String(err) });
      } finally {
        setToggling(null);
      }
    },
    [projectPath],
  );

  if (loading) {
    return <LoadingSpinner label="Loading loops..." />;
  }

  if (error) {
    return (
      <div className="text-destructive text-sm p-3">Failed to load loops: {error}</div>
    );
  }

  if (mode === "structured" && exec) {
    return <StructuredView loaded={exec} />;
  }

  if (mode === "empty" || !status) {
    return (
      <div className="flex flex-col items-center justify-center py-16 text-center">
        <Repeat size={32} className="text-muted-foreground/30 mb-3" />
        <h3 className="text-sm font-semibold text-foreground mb-1.5">
          No loops recorded
        </h3>
        <p className="text-xs text-muted-foreground max-w-xs leading-relaxed">
          Development loops are written by AI agents to{" "}
          <code className="font-mono text-[11px] bg-muted px-1 py-0.5 rounded">
            .loopdeck/loops.md
          </code>
          . Current progress and next steps will appear here automatically.
        </p>
      </div>
    );
  }

  // Legacy mode (loops.md only) — render the legacy view + offer migration.
  return (
    <div className="max-w-2xl">
      {preview && (
        <MigrationCard projectPath={projectPath} preview={preview} onMigrated={reload} />
      )}

      {status.current && (
        <section className="mb-6">
          <h3 className="text-[11px] font-semibold uppercase tracking-wider text-muted-foreground mb-3">
            Current Loop
          </h3>
          <LoopCard loop={status.current} isCurrent />

          {status.next_steps.length > 0 && (
            <div className="mt-3 p-4 rounded-lg bg-surface border border-border">
              <h4 className="text-[10px] font-semibold uppercase tracking-wider text-muted-foreground mb-2">
                Next Steps
              </h4>
              <ul className="space-y-1.5">
                {status.next_steps.map((step, i) => {
                  const isToggling = toggling === step.text;
                  return (
                    <li
                      key={i}
                      className={`flex items-start gap-2 text-sm leading-relaxed ${
                        step.checked
                          ? "text-muted-foreground line-through"
                          : "text-foreground"
                      }`}
                    >
                      <button
                        onClick={() => handleToggle(step.text)}
                        disabled={isToggling}
                        title={step.checked ? "Mark as not done" : "Mark as done"}
                        className="shrink-0 mt-[3px] text-muted-foreground transition-colors hover:text-foreground disabled:opacity-50"
                      >
                        {isToggling ? (
                          <Loader2 size={14} className="animate-spin" />
                        ) : step.checked ? (
                          <CheckCircle2 size={14} className="text-[var(--success)]" />
                        ) : (
                          <Circle size={14} />
                        )}
                      </button>
                      <div className="flex-1 min-w-0">
                        <Markdown>{step.text}</Markdown>
                      </div>
                    </li>
                  );
                })}
              </ul>
            </div>
          )}
        </section>
      )}

      {status.history.length > 0 && (
        <section>
          <h3 className="text-[11px] font-semibold uppercase tracking-wider text-muted-foreground mb-3 flex items-center gap-1.5">
            <History size={13} />
            History ({status.history.length})
          </h3>
          <div className="space-y-2">
            {status.history.map((loop, i) => (
              <LoopCard key={i} loop={loop} />
            ))}
          </div>
        </section>
      )}

      {!status.current && status.history.length === 0 && (
        <div className="flex flex-col items-center justify-center py-12 text-center">
          <Repeat size={28} className="text-muted-foreground/30 mb-3" />
          <h3 className="text-sm font-semibold text-foreground mb-1.5">
            No loop records to display
          </h3>
          <p className="text-xs text-muted-foreground max-w-xs leading-relaxed">
            This legacy file is empty or contains only next steps. You can still
            migrate it to structured execution state.
          </p>
        </div>
      )}
    </div>
  );
}
