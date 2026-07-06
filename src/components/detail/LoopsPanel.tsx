import { useState, useEffect } from "react";
import { Repeat, Circle, History, ArrowRight } from "lucide-react";
import type { LoopStatus, Loop } from "../../types";
import * as api from "../../lib/tauri";
import { LoadingSpinner } from "../shared/LoadingSpinner";
import "./LoopsPanel.css";

interface LoopsPanelProps {
  projectPath: string;
}

const LOOP_STATUS_COLORS: Record<string, string> = {
  in_progress: "var(--color-accent)",
  completed: "var(--color-success)",
  abandoned: "var(--color-text-secondary)",
};

function LoopCard({ loop, isCurrent }: { loop: Loop; isCurrent?: boolean }) {
  return (
    <div className={`loop-card ${isCurrent ? "loop-card--current" : ""}`}>
      <div className="loop-card__header">
        <span className="loop-card__date">{loop.started}</span>
        <span
          className="loop-card__status"
          style={{
            color: LOOP_STATUS_COLORS[loop.status] ?? "var(--color-text-secondary)",
          }}
        >
          {loop.status.replace("_", " ")}
        </span>
        {loop.completed && (
          <span className="loop-card__completed">
            <ArrowRight size={10} /> {loop.completed}
          </span>
        )}
      </div>
      <h3 className="loop-card__goal">{loop.goal}</h3>
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
      <div className="loops-panel">
        <div className="loops-panel__error">
          Failed to load loops: {error}
        </div>
      </div>
    );
  }

  if (!status || (!status.current && status.history.length === 0)) {
    return (
      <div className="loops-panel">
        <div className="loops-panel__empty">
          <Repeat size={32} className="loops-panel__empty-icon" />
          <h3>No loops recorded</h3>
          <p>
            Development loops are written by AI agents to{" "}
            <code>.loopdeck/loops.md</code>. Current progress and next steps
            will appear here automatically.
          </p>
        </div>
      </div>
    );
  }

  return (
    <div className="loops-panel">
      {/* Current loop */}
      {status.current && (
        <section className="loops-panel__section">
          <h3 className="loops-panel__section-title">Current Loop</h3>
          <LoopCard loop={status.current} isCurrent />

          {status.next_steps.length > 0 && (
            <div className="loops-panel__next-steps">
              <h4>Next Steps</h4>
              <ul>
                {status.next_steps.map((step, i) => (
                  <li key={i}>
                    <Circle size={8} />
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
        <section className="loops-panel__section">
          <h3 className="loops-panel__section-title">
            <History size={14} />
            History ({status.history.length})
          </h3>
          <div className="loops-panel__history">
            {status.history.map((loop, i) => (
              <LoopCard key={i} loop={loop} />
            ))}
          </div>
        </section>
      )}
    </div>
  );
}
