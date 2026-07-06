import { useState, useEffect } from "react";
import { Repeat, Circle, History, ArrowRight } from "lucide-react";
import type { LoopStatus, Loop } from "../../types";
import * as api from "../../lib/tauri";
import { LoadingSpinner } from "../shared/LoadingSpinner";

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
      <h3 className="text-sm font-semibold text-foreground leading-snug">
        {loop.goal}
      </h3>
    </div>
  );
}

export function LoopsPanel({ projectPath }: LoopsPanelProps) {
  const [status, setStatus] = useState<LoopStatus | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);

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
                {status.next_steps.map((step, i) => (
                  <li
                    key={i}
                    className="flex items-center gap-2 text-sm text-foreground leading-relaxed"
                  >
                    <Circle size={8} className="text-muted-foreground shrink-0 mt-0.5" />
                    {step}
                  </li>
                ))}
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
