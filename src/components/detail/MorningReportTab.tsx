import { useCallback, useEffect, useState } from "react";
import { AlertTriangle, CheckCircle2, ExternalLink, Shield, Skull, Sun, XCircle } from "lucide-react";
import * as api from "../../lib/tauri";
import { VERDICT_COLOR, VERDICT_LABEL } from "../../lib/nightRun";
import { useAppStore } from "../../store/appStore";
import { buildIdToTitle } from "./EpicsPanel";
import { ParkedQuestionInbox } from "./ParkedQuestionInbox";
import type { Epic, RunPlan, RunReport } from "../../types";

// Same cadence the drawer's `useRunStatus` poll uses — cheap local IPC that
// reads run-plan.yaml. Keeps the open report honest after a requeue restarts
// the run ("stay on report, refetch it", per the run's pre-answered
// clarification): verdicts flip to RUNNING without the drawer yanking the
// user back to the night variant mid-read.
const POLL_MS = 5000;

/**
 * The drawer's morning-report surface (prd-night-run-surfaces Phase 3, item
 * 1), rendered in the Agent-tab slot once the run has finished — the same
 * swap `NightRunTab` uses while a run is active, per the run's pre-answered
 * clarification. Sourced entirely from the real `RunReport` /
 * `PhaseReportEntry` / `AuditSlice` types, reusing RunQueuePanel's
 * verdict-table rendering (the maps now shared via `lib/nightRun.ts`).
 *
 * Sections: per-phase verdict table, kill callout rows, the parked-questions
 * inbox reusing Phase 1's shared `ParkedQuestionInbox` (item 3: its "Answer &
 * requeue" runs the exact night-variant requeue wiring), and a collapsible
 * audit tail rendering the existing `AuditSlice` (auto-allow count + floor
 * denials — the slice carries no raw log lines, per the run's pre-answered
 * clarification, and the PRD's Non-Goals forbid a new endpoint to get any).
 *
 * Mounting marks the report seen (`appStore.morningReportSeen`), which clears
 * the rail door's "morning report ready" badge — "clear once opened".
 */
export function MorningReportTab({
  projectPath,
  plan,
}: {
  projectPath: string;
  /** Identity of the finished plan this report belongs to — the drawer's
   *  latched copy, which stays mounted even after a requeue reactivates the
   *  run (same plan id) until the drawer closes or a new plan appears. */
  plan: RunPlan;
}) {
  const [report, setReport] = useState<RunReport | null>(null);
  const [idToTitle, setIdToTitle] = useState<Record<string, string>>({});
  const [idToAgentName, setIdToAgentName] = useState<Record<string, string>>({});
  const markMorningReportSeen = useAppStore((s) => s.markMorningReportSeen);

  // Opening the report clears the rail door's "morning report ready" badge
  // (prd-night-run-surfaces Phase 3 open question, resolved "clear once
  // opened"). Keyed by plan id so a fresh run re-arms it.
  useEffect(() => {
    markMorningReportSeen(projectPath, plan.id);
  }, [markMorningReportSeen, projectPath, plan.id]);

  const load = useCallback(async () => {
    try {
      setReport(await api.getRunReport(projectPath));
    } catch (err) {
      console.warn("getRunReport failed", err);
    }
  }, [projectPath]);

  useEffect(() => {
    load();
    const id = setInterval(load, POLL_MS);
    return () => clearInterval(id);
  }, [load]);

  useEffect(() => {
    let disposed = false;
    api
      .getEpics(projectPath)
      .then((epics: Epic[]) => {
        if (!disposed) setIdToTitle(buildIdToTitle(epics));
      })
      .catch((err) => console.warn("getEpics failed", err));
    // Roster names for the per-role attribution chips (prd-role-foundations
    // Phase 4) — the report carries roster ids, the table shows names.
    api
      .listAgentConfigs()
      .then((agents) => {
        if (!disposed) {
          setIdToAgentName(Object.fromEntries(agents.map((a) => [a.id, a.name])));
        }
      })
      .catch((err) => console.warn("listAgentConfigs failed", err));
    return () => {
      disposed = true;
    };
  }, [projectPath]);

  if (!report) {
    return (
      <div className="flex flex-1 items-center justify-center text-xs text-muted-foreground">
        Loading morning report…
      </div>
    );
  }

  const parked = report.phases.filter((p) => p.verdict === "parked");
  const killed = report.phases.filter((p) => p.verdict === "killed");
  const passed = report.phases.filter((p) => p.verdict === "pass");

  return (
    <div className="mx-auto min-h-0 w-full max-w-2xl flex-1 overflow-y-auto pb-6">
      {/* Summary bar */}
      <div className="mb-5 flex items-center gap-2">
        <Sun size={14} className="text-[var(--warning)]" />
        <h2 className="text-sm font-semibold tracking-tight">Morning report</h2>
        <span className="text-[10px] text-muted-foreground">
          {report.plan.phases.length} phase{report.plan.phases.length !== 1 ? "s" : ""} ·{" "}
          {report.plan.stall_policy === "halt" ? "halt on stall" : "continue independent"}
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
          <span className="flex items-center gap-1">
            <Shield size={11} />
            {report.audit.auto_allow_count} auto-allowed
          </span>
        </span>
      </div>

      {/* Per-phase verdict table — same shape RunQueuePanel's MorningReport
          renders; maps shared via lib/nightRun.ts. */}
      <div className="rounded-xl border border-border bg-card p-4 shadow-[var(--shadow-sm)]">
        <div className="mb-3 text-[10px] font-semibold uppercase tracking-wider text-muted-foreground">
          Verdicts
        </div>
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
                  <td
                    className="max-w-48 truncate px-1.5 py-1.5 text-foreground"
                    title={phase.execution_id}
                  >
                    {idToTitle[phase.execution_id] ?? phase.execution_id}
                    {/* Per-role attribution (prd-role-foundations Phase 4):
                        which roster agent ran this phase. */}
                    {phase.assigned_agent && (
                      <span
                        className="ml-1.5 rounded bg-accent px-1 py-0.5 text-[9px] text-muted-foreground"
                        title={phase.assigned_agent}
                      >
                        {idToAgentName[phase.assigned_agent] ?? phase.assigned_agent}
                      </span>
                    )}
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
                    {phase.wall_clock_secs > 0
                      ? `${Math.floor(phase.wall_clock_secs / 60)}m ${phase.wall_clock_secs % 60}s`
                      : "—"}
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
      </div>

      {/* Kill callout rows: budget/stall kills pulled out of the table so the
          destructive outcomes can't hide in a row-scroll. */}
      {killed.length > 0 && (
        <div className="mt-4 rounded-xl border border-[color-mix(in_srgb,var(--destructive)_35%,transparent)] bg-card p-4">
          <div className="mb-2 text-[10px] font-semibold uppercase tracking-wider text-[var(--destructive)]">
            Killed by the run ({killed.length})
          </div>
          <ul className="space-y-1.5">
            {killed.map((phase) => (
              <li
                key={phase.execution_id}
                className="flex items-start gap-1.5 rounded bg-red-500/5 px-2.5 py-1.5 text-[11px]"
              >
                <Skull size={11} className="mt-0.5 shrink-0 text-[var(--destructive)]" />
                <span className="font-medium text-foreground">
                  {idToTitle[phase.execution_id] ?? phase.execution_id}
                </span>
                {phase.reason && (
                  <span className="min-w-0 flex-1 break-words text-muted-foreground">
                    {phase.reason}
                  </span>
                )}
              </li>
            ))}
          </ul>
        </div>
      )}

      {/* Parked-questions section (Phase 3, item 3): Phase 1's exact inline
          card + requeue wiring — both paths (answerParkedQuestion for
          structured payloads, requeueRunPhase + queueRun for raw). The poll
          above refetches the report after a requeue ("stay on report, refetch
          it"), so freshly-requeued phases show their new verdicts inline. */}
      <div className="mt-4">
        <ParkedQuestionInbox
          projectPath={projectPath}
          plan={report.plan}
          idToTitle={idToTitle}
          onResolved={load}
        />
      </div>

      {/* Collapsible audit-log tail: renders the existing AuditSlice —
          auto-allow count + itemized floor denials. The slice carries no raw
          log lines and the PRD's Non-Goals forbid a new endpoint, per the
          run's pre-answered clarification. */}
      <details className="mt-4 rounded-xl border border-border bg-card p-4 text-xs">
        <summary className="cursor-pointer select-none text-[10px] font-semibold uppercase tracking-wider text-muted-foreground">
          Audit tail · {report.audit.auto_allow_count} auto-allowed ·{" "}
          {report.audit.floor_denials.length} floor denial
          {report.audit.floor_denials.length !== 1 ? "s" : ""}
        </summary>
        <div className="mt-3 space-y-2">
          <p className="text-[11px] leading-relaxed text-muted-foreground">
            {report.audit.auto_allow_count} tool call
            {report.audit.auto_allow_count !== 1 ? "s" : ""} auto-allowed under unattended mode
            during the run window. The destructive floor still applied.
          </p>
          {report.audit.floor_denials.length > 0 ? (
            <ul className="space-y-1">
              {report.audit.floor_denials.map((denial, i) => (
                <li
                  key={i}
                  className="rounded bg-red-500/5 px-2 py-1 font-mono text-[10px] text-muted-foreground"
                >
                  {denial}
                </li>
              ))}
            </ul>
          ) : (
            <p className="text-[11px] text-muted-foreground">No floor denials.</p>
          )}
        </div>
      </details>
    </div>
  );
}
