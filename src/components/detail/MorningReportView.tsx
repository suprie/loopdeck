import { AlertTriangle, CheckCircle2, ExternalLink, Loader2, RotateCcw, Shield, Skull, XCircle } from "lucide-react";
import type { AskUserQuestionAnswers, PhaseVerdict, RunReport } from "../../types";
import { parseParkedQuestions } from "../../lib/nightRun";
import { AskUserQuestionCard } from "./AskUserQuestionCard";

// ── Morning report rendering (prd-wake-up Phase 2, shared per
// prd-night-run-surfaces Phase 3) ────────────────────────────────────────────
// Extracted from RunQueuePanel.tsx so the drawer's morning-report variant and
// the legacy EpicsPanel mount render the exact same report — single source,
// per the run's pre-answered clarification ("Extract shared").

export const VERDICT_LABEL: Record<PhaseVerdict, string> = {
  pass: "PASS",
  warn: "WARN",
  block: "BLOCK",
  killed: "KILLED",
  failed: "FAILED",
  parked: "PARKED",
  running: "RUNNING",
};

export const VERDICT_COLOR: Record<PhaseVerdict, string> = {
  pass: "var(--success)",
  warn: "var(--warning)",
  block: "var(--destructive)",
  killed: "var(--destructive)",
  failed: "var(--destructive)",
  parked: "var(--warning)",
  running: "var(--primary)",
};

export function MorningReportView({
  report,
  idToTitle,
  onRetry,
  retryingId,
  onAnswerParked,
  answeringId,
}: {
  report: RunReport;
  idToTitle: Record<string, string>;
  /** Raw-payload parked card + kill/fail rows: `requeueRunPhase` + `queueRun`
   *  (the night variant's inline-card requeue flow). */
  onRetry: (executionId: string) => void;
  retryingId: string | null;
  /** Structured `__QUESTIONS__` parked card: `answerParkedQuestion` (the
   *  night variant's inline-card answer flow). */
  onAnswerParked: (executionId: string, answers: AskUserQuestionAnswers) => void;
  answeringId: string | null;
}) {
  const parked = report.phases.filter((p) => p.verdict === "parked");
  const killed = report.phases.filter((p) => p.verdict === "killed");
  const passed = report.phases.filter((p) => p.verdict === "pass");
  const title = (executionId: string) => idToTitle[executionId] ?? executionId;

  return (
    <div className="mb-4 rounded-lg border border-border bg-card p-3">
      {/* Summary bar */}
      <div className="mb-3 flex items-center gap-2 text-xs">
        <span className="font-semibold text-foreground">Morning Report</span>
        <span className="text-[10px] text-muted-foreground">
          {report.plan.phases.length} phase{report.plan.phases.length !== 1 ? "s" : ""}
        </span>
        <span className="ml-auto flex items-center gap-3 text-[10px] text-muted-foreground">
          <span className="flex items-center gap-1">
            <CheckCircle2 size={11} className="text-[var(--success)]" />
            {passed.length} passed
          </span>
          <span className="flex items-center gap-1">
            <AlertTriangle size={11} className="text-[var(--warning)]" />
            {parked.length} parked
          </span>
          <span className="flex items-center gap-1">
            <XCircle size={11} className="text-[var(--destructive)]" />
            {killed.length} killed
          </span>
          {report.audit.auto_allow_count > 0 && (
            <span className="flex items-center gap-1">
              <Shield size={11} />
              {report.audit.auto_allow_count} auto-allowed
            </span>
          )}
        </span>
      </div>

      {/* Per-phase verdict table */}
      <div className="overflow-x-auto">
        <table className="w-full text-xs">
          <thead>
            <tr className="border-b border-border text-[10px] uppercase tracking-wider text-muted-foreground">
              <th className="px-1.5 py-1 text-left font-medium">Phase</th>
              <th className="px-1.5 py-1 text-left font-medium">Verdict</th>
              <th className="px-1.5 py-1 text-right font-medium">Tokens</th>
              <th className="px-1.5 py-1 text-right font-medium">Wall Time</th>
              <th className="px-1.5 py-1 text-left font-medium">Details</th>
            </tr>
          </thead>
          <tbody>
            {report.phases.map((phase) => (
              <tr key={phase.execution_id} className="border-b border-border/50">
                <td className="max-w-48 truncate px-1.5 py-1.5 text-foreground" title={phase.execution_id}>
                  {title(phase.execution_id)}
                </td>
                <td className="px-1.5 py-1.5">
                  <span
                    className="rounded px-1.5 py-0.5 text-[10px] font-semibold"
                    style={{ color: VERDICT_COLOR[phase.verdict] }}
                  >
                    {VERDICT_LABEL[phase.verdict]}
                  </span>
                </td>
                <td className="px-1.5 py-1.5 text-right tabular-nums text-muted-foreground">
                  {phase.token_usage > 0 ? phase.token_usage.toLocaleString() : "—"}
                </td>
                <td className="px-1.5 py-1.5 text-right tabular-nums text-muted-foreground">
                  {phase.wall_clock_secs > 0 ? `${Math.floor(phase.wall_clock_secs / 60)}m ${phase.wall_clock_secs % 60}s` : "—"}
                </td>
                <td className="max-w-64 px-1.5 py-1.5 text-muted-foreground">
                  {phase.draft_pr_url ? (
                    <a
                      href={phase.draft_pr_url}
                      target="_blank"
                      rel="noopener noreferrer"
                      className="inline-flex items-center gap-1 text-[var(--primary)] hover:underline"
                    >
                      <ExternalLink size={10} />
                      Draft PR
                    </a>
                  ) : phase.reason ? (
                    <span className="truncate block" title={phase.reason}>
                      {phase.reason.length > 80 ? `${phase.reason.slice(0, 80)}…` : phase.reason}
                    </span>
                  ) : (
                    "—"
                  )}
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      </div>

      {/* Parked-question inbox — same actions as the night variant's inline
          card (prd-night-run-surfaces Phase 3, item 3): structured
          __QUESTIONS__ payloads submit via `answerParkedQuestion`, raw
          payloads requeue via `requeueRunPhase` + `queueRun`. */}
      {parked.length > 0 && (
        <div className="mt-3 border-t border-border pt-3">
          <div className="mb-2 text-[10px] font-semibold uppercase tracking-wider text-muted-foreground">
            Parked questions ({parked.length})
          </div>
          <ul className="space-y-2">
            {parked.map((phase) => {
              const parsed = parseParkedQuestions(phase.reason);
              return (
                <li key={phase.execution_id} className="rounded bg-amber-500/5 px-2 py-1.5">
                  <div className="mb-1 flex items-center justify-between">
                    <span className="text-[11px] font-medium text-foreground">
                      {title(phase.execution_id)}
                    </span>
                    {parsed.questions ? null : (
                      <button
                        onClick={() => onRetry(phase.execution_id)}
                        disabled={retryingId === phase.execution_id}
                        className="flex shrink-0 items-center gap-1 rounded px-1.5 py-0.5 text-[10px] font-medium text-[var(--warning)] transition-colors hover:bg-accent disabled:opacity-50"
                      >
                        {retryingId === phase.execution_id ? <Loader2 size={10} className="animate-spin" /> : <RotateCcw size={10} />}
                        Answer &amp; requeue
                      </button>
                    )}
                  </div>
                  {parsed.questions ? (
                    <AskUserQuestionCard
                      questions={parsed.questions}
                      disabled={answeringId === phase.execution_id}
                      submitLabel="Answer & requeue"
                      onSubmit={(answers) => onAnswerParked(phase.execution_id, answers)}
                    />
                  ) : phase.reason ? (
                    <p className="break-words text-[11px] text-muted-foreground">{phase.reason}</p>
                  ) : null}
                </li>
              );
            })}
          </ul>
        </div>
      )}

      {/* Kill callout rows — killed phases with their verbatim reasons,
          individually requeueable like the parked cards above. */}
      {killed.length > 0 && (
        <div className="mt-3 border-t border-border pt-3">
          <div className="mb-2 text-[10px] font-semibold uppercase tracking-wider text-[var(--destructive)]">
            Killed phases ({killed.length})
          </div>
          <ul className="space-y-1">
            {killed.map((phase) => (
              <li key={phase.execution_id} className="rounded bg-red-500/5 px-2 py-1.5">
                <div className="flex items-center justify-between gap-2">
                  <span className="flex min-w-0 items-center gap-1.5 text-[11px] font-medium text-foreground">
                    <Skull size={11} className="shrink-0 text-[var(--destructive)]" />
                    <span className="truncate">{title(phase.execution_id)}</span>
                  </span>
                  <button
                    onClick={() => onRetry(phase.execution_id)}
                    disabled={retryingId === phase.execution_id}
                    className="flex shrink-0 items-center gap-1 rounded px-1.5 py-0.5 text-[10px] font-medium text-[var(--destructive)] transition-colors hover:bg-accent disabled:opacity-50"
                  >
                    {retryingId === phase.execution_id ? <Loader2 size={10} className="animate-spin" /> : <RotateCcw size={10} />}
                    Retry
                  </button>
                </div>
                {phase.reason && (
                  <p className="mt-0.5 break-words pl-4 text-[11px] text-muted-foreground">{phase.reason}</p>
                )}
              </li>
            ))}
          </ul>
        </div>
      )}

      {/* Collapsible audit-log tail — the audit slice's floor denials,
          collapsed by default so a noisy night doesn't bury the verdicts. */}
      {report.audit.floor_denials.length > 0 && (
        <details className="mt-3 border-t border-border pt-3">
          <summary className="cursor-pointer select-none text-[10px] font-semibold uppercase tracking-wider text-muted-foreground">
            Audit tail — {report.audit.floor_denials.length} floor denial
            {report.audit.floor_denials.length !== 1 ? "s" : ""}
          </summary>
          <ul className="mt-2 space-y-1">
            {report.audit.floor_denials.map((denial, i) => (
              <li key={i} className="rounded bg-red-500/5 px-2 py-1 font-mono text-[10px] text-muted-foreground">
                {denial}
              </li>
            ))}
          </ul>
        </details>
      )}
    </div>
  );
}
