import { useState, useEffect, useCallback } from "react";
import { Repeat, Circle, CheckCircle2, History, ArrowRight, Loader2 } from "lucide-react";
import { toast } from "sonner";
import type { LoopStatus, Loop } from "../../types";
import * as api from "../../lib/tauri";
import { LoadingSpinner } from "../shared/LoadingSpinner";
import { Markdown } from "../shared/Markdown";

interface LoopsPanelProps {
  projectPath: string;
}

const LOOP_STATUS_COLORS: Record<string, string> = {
  in_progress: "var(--primary)",
  completed: "var(--success)",
  abandoned: "var(--muted-foreground)",
};

const LOOP_STATUS_BG: Record<string, string> = {
  in_progress: "bg-[color-mix(in_oklab,var(--primary)_12%,transparent)]",
  completed: "bg-[color-mix(in_oklab,var(--success)_12%,transparent)]",
  abandoned: "bg-muted",
};

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

export function LoopsPanel({ projectPath }: LoopsPanelProps) {
  const [status, setStatus] = useState<LoopStatus | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [toggling, setToggling] = useState<string | null>(null);

  useEffect(() => {
    let cancelled = false;
    async function load() {
      try {
        const data = await api.getLoops(projectPath);
        if (!cancelled) {
          setStatus(data);
          setError(null);
        }
      } catch (err) {
        if (!cancelled) {
          setError(String(err));
        }
      } finally {
        if (!cancelled) setLoading(false);
      }
    }
    load();
    return () => {
      cancelled = true;
    };
  }, [projectPath]);

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
      <div className="text-destructive text-sm p-3">
        Failed to load loops: {error}
      </div>
    );
  }

  if (!status || (!status.current && status.history.length === 0)) {
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

  return (
    <div className="max-w-2xl">
      {/* Current loop */}
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
                        step.checked ? "text-muted-foreground line-through" : "text-foreground"
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

      {/* History */}
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
    </div>
  );
}
