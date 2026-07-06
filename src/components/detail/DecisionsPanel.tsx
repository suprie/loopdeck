import { useState, useEffect } from "react";
import { Lightbulb } from "lucide-react";
import type { Decision } from "../../types";
import * as api from "../../lib/tauri";
import { LoadingSpinner } from "../shared/LoadingSpinner";
import "./DecisionsPanel.css";

interface DecisionsPanelProps {
  projectPath: string;
}

const STATUS_COLORS: Record<string, string> = {
  proposed: "var(--color-warning)",
  accepted: "var(--color-success)",
  superseded: "var(--color-text-secondary)",
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
      <div className="decisions-panel">
        <div className="decisions-panel__error">
          Failed to load decisions: {error}
        </div>
      </div>
    );
  }

  if (decisions.length === 0) {
    return (
      <div className="decisions-panel">
        <div className="decisions-panel__empty">
          <Lightbulb size={32} className="decisions-panel__empty-icon" />
          <h3>No decisions recorded</h3>
          <p>
            Architectural decisions are written by AI agents to{" "}
            <code>.loopdeck/decisions.md</code>. They will appear here
            automatically once recorded.
          </p>
        </div>
      </div>
    );
  }

  return (
    <div className="decisions-panel">
      <div className="decisions-panel__count">
        {decisions.length} decision{decisions.length !== 1 ? "s" : ""}
      </div>
      <div className="decisions-panel__list">
        {decisions.map((decision, i) => (
          <div key={i} className="decision-card">
            <div className="decision-card__header">
              <span className="decision-card__date">{decision.date}</span>
              <span
                className="decision-card__status"
                style={{ color: STATUS_COLORS[decision.status] ?? "var(--color-text-secondary)" }}
              >
                {decision.status}
              </span>
            </div>
            <h3 className="decision-card__title">{decision.title}</h3>
            <p className="decision-card__context">{decision.context}</p>
            {decision.consequences && (
              <div className="decision-card__consequences">
                <span className="decision-card__consequences-label">Consequences</span>
                <p>{decision.consequences}</p>
              </div>
            )}
          </div>
        ))}
      </div>
    </div>
  );
}
