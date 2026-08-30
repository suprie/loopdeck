import { useEffect, useState } from "react";
import { Moon } from "lucide-react";
import type { Epic, RunPhase, RunPlan } from "../../types";
import {
  budgetGauges,
  formatDuration,
  formatTokens,
  gaugePercent,
  STATUS_COLOR,
  STATUS_LABEL,
} from "../../lib/nightRun";
import { buildIdToTitle } from "./EpicsPanel";
import * as api from "../../lib/tauri";
import { ParkedQuestionInbox } from "./ParkedQuestionInbox";

export function NightRunTab({ projectPath, plan }: { projectPath: string; plan: RunPlan }) {
  const [idToTitle, setIdToTitle] = useState<Record<string, string>>({});

  useEffect(() => {
    let disposed = false;
    api
      .getEpics(projectPath)
      .then((epics: Epic[]) => {
        if (!disposed) setIdToTitle(buildIdToTitle(epics));
      })
      .catch((err) => console.warn("getEpics failed", err));
    return () => {
      disposed = true;
    };
  }, [projectPath]);

  const title = (phase: RunPhase) => idToTitle[phase.execution_id] ?? phase.execution_id;

  return (
    <section className="mx-auto mb-4 w-full max-w-3xl shrink-0 rounded-xl border border-border bg-card p-4 shadow-[var(--shadow-sm)]">
      <div className="mb-5 flex items-center gap-2">
        <Moon size={14} className="text-[var(--primary)]" />
        <h2 className="text-sm font-semibold tracking-tight">Night run</h2>
        <span className="text-[10px] text-muted-foreground">
          {plan.phases.length} phase{plan.phases.length !== 1 ? "s" : ""} ·{" "}
          {plan.stall_policy === "halt" ? "halt on stall" : "continue independent"}
          {plan.environment.worktree_kept ? " · worktree kept for review" : ""}
        </span>
      </div>

      <details className="mt-3 text-xs">
        <summary className="cursor-pointer text-muted-foreground hover:text-foreground">
          Run details: phases and budgets
        </summary>
        <div className="mt-3 space-y-4 border-t border-border pt-3">
      {/* Phase-chip rail: one chip per phase, colored by status via the same
          map RunQueuePanel's phase rows use. Tooltip carries the loop title. */}
      <div className="rounded-xl border border-border bg-card p-4 shadow-[var(--shadow-sm)]">
        <div className="mb-3 text-[10px] font-semibold uppercase tracking-wider text-muted-foreground">
          Phases
        </div>
        <div className="flex flex-wrap items-center gap-1.5">
          {plan.phases.map((phase, i) => {
            const color = STATUS_COLOR[phase.status];
            return (
              <span
                key={phase.execution_id}
                title={`${i + 1}. ${title(phase)} — ${STATUS_LABEL[phase.status]}`}
                className="inline-flex items-center gap-1.5 rounded-full border px-2.5 py-1 text-[11px] font-medium"
                style={{
                  color,
                  borderColor: color,
                  backgroundColor: `color-mix(in srgb, ${color} 10%, transparent)`,
                }}
              >
                <span className="font-mono text-[10px] opacity-70">{i + 1}</span>
                {STATUS_LABEL[phase.status]}
              </span>
            );
          })}
        </div>
      </div>

      {/* Budget gauges: tokens/phase, wall-clock/phase, total run. Caps fall
          back to the limits.rs defaults mirrored in lib/nightRun.ts. */}
      <div className="mt-4 rounded-xl border border-border bg-card p-4 shadow-[var(--shadow-sm)]">
        <div className="mb-3 text-[10px] font-semibold uppercase tracking-wider text-muted-foreground">
          Budgets
        </div>
        <div className="space-y-4">
          {budgetGauges(plan).map((gauge) => {
            const pct = gaugePercent(gauge.used, gauge.cap);
            const fill =
              pct >= 90
                ? "var(--destructive)"
                : pct >= 75
                  ? "var(--warning)"
                  : "var(--primary)";
            return (
              <div key={gauge.id}>
                <div className="mb-1 flex items-baseline justify-between gap-2 text-xs">
                  <span className="font-medium">{gauge.label}</span>
                  <span className="tabular-nums text-muted-foreground">
                    {gauge.unit === "tokens"
                      ? `${formatTokens(gauge.used)} / ${formatTokens(gauge.cap)} tok`
                      : `${formatDuration(gauge.used)} / ${formatDuration(gauge.cap)}`}{" "}
                    · {Math.round(pct)}%
                  </span>
                </div>
                <div
                  className="h-1.5 overflow-hidden rounded-full bg-muted"
                  role="progressbar"
                  aria-valuenow={gauge.used}
                  aria-valuemin={0}
                  aria-valuemax={gauge.cap}
                  aria-label={`${gauge.label} budget`}
                >
                  <div
                    className="h-full rounded-full transition-[width]"
                    style={{ width: `${pct}%`, backgroundColor: fill }}
                  />
                </div>
              </div>
            );
          })}
        </div>
      </div>
        </div>
      </details>

      <div className="mt-3">
        <ParkedQuestionInbox
          projectPath={projectPath}
          plan={plan}
          idToTitle={idToTitle}
        />
      </div>
    </section>
  );
}
