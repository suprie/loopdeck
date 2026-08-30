import { useEffect, useState } from "react";
import { Moon, AlertTriangle, Loader2, RotateCcw } from "lucide-react";
import { toast } from "sonner";
import * as api from "../../lib/tauri";
import { AskUserQuestionCard } from "./AskUserQuestionCard";
import {
  budgetGauges,
  formatDuration,
  formatTokens,
  gaugePercent,
  parkedInbox,
  STATUS_COLOR,
  STATUS_LABEL,
} from "../../lib/nightRun";
import type { AppError, AskUserQuestionAnswers, Epic, RunPhase, RunPlan } from "../../types";
import { buildIdToTitle } from "./EpicsPanel";
import { useStreamingState } from "../../store/streamingState";

/**
 * The drawer's night variant (prd-night-run-surfaces Phase 1, item 1):
 * a phase-chip rail and the 3 budget gauges, rendered in place of the Agent
 * tab while the project has a run in flight or queued. Sourced entirely from
 * the real `RunPlan`/`RunPhase`/`RunBudgets` types, reusing
 * `RunQueuePanel`'s status maps and parked-question parser rather than
 * re-deriving shapes.
 *
 * Below the rail/gauges: the inline parked-question inbox (Phase 1, item 2),
 * one card per currently-parked phase, mirroring `RunQueuePanel`'s
 * "Parked questions" inbox — structured `__QUESTIONS__` payloads answer via
 * the shared `AskUserQuestionCard` (submit = "Answer & requeue" →
 * `answerParkedQuestion`), raw payloads get a plain "Answer & requeue"
 * button → `requeueRunPhase` + `queueRun`, same as RunQueuePanel's Retry.
 *
 * The automatic variant-switch-on-drawer-open (item 3) lands in its own loop.
 */
export function NightRunTab({ projectPath, plan }: { projectPath: string; plan: RunPlan }) {
  const [idToTitle, setIdToTitle] = useState<Record<string, string>>({});
  const [answeringId, setAnsweringId] = useState<string | null>(null);
  const [requeueingId, setRequeueingId] = useState<string | null>(null);
  // Phases answered/requeued from this tab, hidden optimistically until the
  // drawer's 5s `useRunStatus` poll delivers the updated plan.
  const [resolved, setResolved] = useState<Set<string>>(new Set());

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

  const resolve = (executionId: string) =>
    setResolved((prev) => {
      const next = new Set(prev);
      next.add(executionId);
      return next;
    });

  // Same flow as RunQueuePanel's handleAnswerParked: pin the answers into the
  // phase's interview and requeue it in one IPC call.
  const handleAnswer = async (executionId: string, answers: AskUserQuestionAnswers) => {
    setAnsweringId(executionId);
    try {
      await api.answerParkedQuestion(projectPath, executionId, answers);
      useStreamingState.getState().beginTurn(projectPath);
      await api.queueRun(projectPath);
      resolve(executionId);
      toast.success("Answers pinned — run resumed");
    } catch (err) {
      useStreamingState.getState().clear(projectPath);
      const appErr = err as AppError;
      toast.error("Failed to answer parked question", {
        description: appErr.message ?? String(err),
      });
    } finally {
      setAnsweringId(null);
    }
  };

  // Raw-payload fallback, same flow as RunQueuePanel's handleRetry: requeue
  // the phase (there are no structured answers to pin), then resume the run.
  const handleRequeue = async (executionId: string) => {
    setRequeueingId(executionId);
    try {
      useStreamingState.getState().beginTurn(projectPath);
      await api.requeueRunPhase(projectPath, executionId);
      await api.queueRun(projectPath);
      resolve(executionId);
      toast.success("Parked phase restarted unattended");
    } catch (err) {
      useStreamingState.getState().clear(projectPath);
      const appErr = err as AppError;
      toast.error("Failed to requeue phase", {
        description: appErr.message ?? String(err),
      });
    } finally {
      setRequeueingId(null);
    }
  };

  const cards = parkedInbox(plan).filter((c) => !resolved.has(c.phase.execution_id));

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

      {/* Inline parked-question inbox (Phase 1, item 2): stacked below the
          rail/gauges, one card per currently-parked phase — same shape as
          RunQueuePanel's "Parked questions" inbox. */}
      {cards.length > 0 && (
        <div className="mt-3 rounded-lg border border-amber-500/30 bg-amber-500/5 p-3">
          <div className="mb-2 text-[10px] font-semibold uppercase tracking-wider text-muted-foreground">
            Parked questions ({cards.length})
          </div>
          <ul className="space-y-2">
            {cards.map(({ phase, questions }) => (
              <li key={phase.execution_id} className="rounded bg-amber-500/5 px-2.5 py-2">
                <div className="mb-1 flex items-center gap-1.5 text-[11px] font-medium text-foreground">
                  <AlertTriangle size={11} className="shrink-0 text-[var(--warning)]" />
                  {title(phase)}
                </div>
                {questions ? (
                  <AskUserQuestionCard
                    questions={questions}
                    disabled={answeringId === phase.execution_id}
                    submitLabel="Answer & requeue"
                    onSubmit={(answers) => handleAnswer(phase.execution_id, answers)}
                  />
                ) : (
                  <>
                    <p className="break-words pl-4 text-[11px] leading-relaxed text-muted-foreground">
                      {phase.park_payload}
                    </p>
                    <div className="mt-1.5 flex justify-end">
                      <button
                        onClick={() => handleRequeue(phase.execution_id)}
                        disabled={requeueingId === phase.execution_id}
                        className="flex shrink-0 items-center gap-1 rounded px-1.5 py-0.5 text-[10px] font-medium text-[var(--warning)] transition-colors hover:bg-accent disabled:opacity-50"
                      >
                        {requeueingId === phase.execution_id ? (
                          <Loader2 size={10} className="animate-spin" />
                        ) : (
                          <RotateCcw size={10} />
                        )}
                        Answer &amp; requeue
                      </button>
                    </div>
                  </>
                )}
              </li>
            ))}
          </ul>
        </div>
      )}
    </section>
  );
}
