import { useEffect, useState } from "react";
import { Moon, AlertTriangle } from "lucide-react";
import * as api from "../../lib/tauri";
import {
  budgetGauges,
  formatDuration,
  formatTokens,
  gaugePercent,
  parseParkedQuestions,
  STATUS_COLOR,
  STATUS_LABEL,
} from "../../lib/nightRun";
import type { Epic, RunPhase, RunPlan } from "../../types";
import { buildIdToTitle } from "./EpicsPanel";

/**
 * The drawer's night variant (prd-night-run-surfaces Phase 1, item 1):
 * a phase-chip rail and the 3 budget gauges, rendered in place of the Agent
 * tab while the project has a run in flight or queued. Sourced entirely from
 * the real `RunPlan`/`RunPhase`/`RunBudgets` types, reusing
 * `RunQueuePanel`'s status maps and parked-question parser rather than
 * re-deriving shapes.
 *
 * The interactive parked-question card with "Answer & requeue" (Phase 1,
 * item 2) and the automatic variant-switch-on-drawer-open (item 3) land in
 * their own loops — parked payloads render read-only here.
 */
export function NightRunTab({ projectPath, plan }: { projectPath: string; plan: RunPlan }) {
  const [idToTitle, setIdToTitle] = useState<Record<string, string>>({});

  // Loop titles for chip tooltips — same join EpicsPanel feeds RunQueuePanel.
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
  const parked = plan.phases.filter((p) => p.park_payload);

  return (
    <div className="mx-auto min-h-0 w-full max-w-2xl flex-1 overflow-y-auto pb-6">
      <div className="mb-5 flex items-center gap-2">
        <Moon size={14} className="text-[var(--primary)]" />
        <h2 className="text-sm font-semibold tracking-tight">Night run</h2>
        <span className="text-[10px] text-muted-foreground">
          {plan.phases.length} phase{plan.phases.length !== 1 ? "s" : ""} ·{" "}
          {plan.stall_policy === "halt" ? "halt on stall" : "continue independent"}
          {plan.environment.worktree_kept ? " · worktree kept for review" : ""}
        </span>
      </div>

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

      {/* Read-only parked payloads. The interactive "Answer & requeue" card
          (Phase 1, item 2) replaces this when it lands. */}
      {parked.length > 0 && (
        <div className="mt-4 rounded-xl border border-border bg-card p-4 shadow-[var(--shadow-sm)]">
          <div className="mb-2 text-[10px] font-semibold uppercase tracking-wider text-muted-foreground">
            Parked ({parked.length})
          </div>
          <ul className="space-y-2">
            {parked.map((phase) => (
              <ParkedPayloadRow
                key={phase.execution_id}
                phase={phase}
                title={title(phase)}
              />
            ))}
          </ul>
        </div>
      )}
    </div>
  );
}

/** One parked phase's payload — structured questions (via the shared
 *  `__QUESTIONS__` parser) rendered as text, raw reason as a fallback. */
function ParkedPayloadRow({ phase, title }: { phase: RunPhase; title: string }) {
  const parsed = parseParkedQuestions(phase.park_payload);
  return (
    <li className="rounded bg-amber-500/5 px-2.5 py-2">
      <div className="mb-1 flex items-center gap-1.5 text-[11px] font-medium text-foreground">
        <AlertTriangle size={11} className="shrink-0 text-[var(--warning)]" />
        {title}
      </div>
      {parsed.questions ? (
        <ul className="space-y-1 pl-4">
          {parsed.questions.map((q) => (
            <li key={q.question} className="text-[11px] leading-relaxed text-muted-foreground">
              <span className="font-medium text-foreground">{q.header}:</span> {q.question}
              <span className="block text-[10px]">
                {q.options.map((o) => o.label).join(" · ")}
              </span>
            </li>
          ))}
        </ul>
      ) : (
        <p className="break-words pl-4 text-[11px] leading-relaxed text-muted-foreground">
          {phase.park_payload}
        </p>
      )}
    </li>
  );
}
