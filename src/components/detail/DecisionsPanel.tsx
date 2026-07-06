import { useState, useEffect } from "react";
import { Lightbulb } from "lucide-react";
import type { Decision } from "../../types";
import * as api from "../../lib/tauri";
import { LoadingSpinner } from "../shared/LoadingSpinner";
import { Markdown } from "../shared/Markdown";

interface DecisionsPanelProps {
  projectPath: string;
}

const STATUS_COLORS: Record<string, string> = {
  proposed: "var(--warning)",
  accepted: "var(--success)",
  superseded: "var(--muted-foreground)",
};

const STATUS_BG: Record<string, string> = {
  proposed: "bg-[color-mix(in_oklab,var(--warning)_12%,transparent)]",
  accepted: "bg-[color-mix(in_oklab,var(--success)_12%,transparent)]",
  superseded: "bg-muted",
};

export function DecisionsPanel({ projectPath }: DecisionsPanelProps) {
  const [decisions, setDecisions] = useState<Decision[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let cancelled = false;
    async function load() {
      try {
        const data = await api.getDecisions(projectPath);
        if (!cancelled) {
          setDecisions(data);
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
    return <LoadingSpinner label="Loading decisions..." />;
  }

  if (error) {
    return (
      <div className="text-destructive text-sm p-3">
        Failed to load decisions: {error}
      </div>
    );
  }

  if (decisions.length === 0) {
    return (
      <div className="flex flex-col items-center justify-center py-16 text-center">
        <Lightbulb size={32} className="text-muted-foreground/30 mb-3" />
        <h3 className="text-sm font-semibold text-foreground mb-1.5">
          No decisions recorded
        </h3>
        <p className="text-xs text-muted-foreground max-w-xs leading-relaxed">
          Architectural decisions are written by AI agents to{" "}
          <code className="font-mono text-[11px] bg-muted px-1 py-0.5 rounded">
            .loopdeck/decisions.md
          </code>
          . They will appear here automatically once recorded.
        </p>
      </div>
    );
  }

  return (
    <div className="max-w-2xl">
      <div className="text-xs text-muted-foreground mb-4">
        {decisions.length} decision{decisions.length !== 1 ? "s" : ""}
      </div>
      <div className="space-y-3">
        {decisions.map((decision, i) => (
          <div
            key={i}
            className="rounded-xl border border-border bg-card p-4 hover:border-primary/40 transition-all"
          >
            <div className="flex items-center justify-between mb-2">
              <span className="text-[11px] font-mono text-muted-foreground">
                {decision.date}
              </span>
              <span
                className={`text-[10px] font-semibold uppercase tracking-wider px-1.5 py-0.5 rounded ${STATUS_BG[decision.status] ?? "bg-muted"}`}
                style={{
                  color: STATUS_COLORS[decision.status] ?? "var(--muted-foreground)",
                }}
              >
                {decision.status}
              </span>
            </div>
            <h3 className="text-sm font-semibold text-foreground mb-2">
              {decision.title}
            </h3>
            <div className="text-xs text-foreground/80 leading-relaxed mb-2">
              <Markdown>{decision.context}</Markdown>
            </div>
            {decision.consequences && (
              <div className="mt-2 p-3 rounded-lg bg-surface border-l-[3px] border-l-[var(--primary)]">
                <span className="text-[10px] font-semibold uppercase tracking-wider text-[var(--primary)] block mb-1">
                  Consequences
                </span>
                <div className="text-xs text-muted-foreground leading-relaxed">
                  <Markdown>{decision.consequences}</Markdown>
                </div>
              </div>
            )}
          </div>
        ))}
      </div>
    </div>
  );
}
