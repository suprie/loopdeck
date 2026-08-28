import { useEffect, useState } from "react";
import { Sunrise } from "lucide-react";
import { toast } from "sonner";
import * as api from "../../lib/tauri";
import { MorningReportView } from "./MorningReportView";
import type { AppError, AskUserQuestionAnswers, Epic, RunReport } from "../../types";
import { buildIdToTitle } from "./EpicsPanel";
import { useStreamingState } from "../../store/streamingState";

const POLL_MS = 5000;

/**
 * The drawer's morning-report variant (prd-night-run-surfaces Phase 3, item
 * 1) — the third auto-select variant: rendered in place of the Agent tab when
 * a run has finished but its report still has unresolved parked questions
 * (`hasUnresolvedParkedQuestions`, the same flag the rail door / room card
 * indicator uses). Once every parked question is resolved, the drawer reverts
 * to the day variant — the report is an attention surface, not an archive.
 *
 * Renders the shared `MorningReportView` (extracted from `RunQueuePanel`) so
 * the verdict table, parked inbox, kill callouts, and audit tail are the exact
 * same component the legacy EpicsPanel mount shows.
 */
export function MorningReportTab({ projectPath }: { projectPath: string }) {
  const [idToTitle, setIdToTitle] = useState<Record<string, string>>({});
  const [report, setReport] = useState<RunReport | null>(null);
  const [answeringId, setAnsweringId] = useState<string | null>(null);
  const [requeueingId, setRequeueingId] = useState<string | null>(null);
  // Phases answered/requeued from this tab, hidden optimistically until the
  // 5s report poll delivers the updated verdicts (same pattern as NightRunTab).
  const [resolved, setResolved] = useState<Set<string>>(new Set());

  // Loop titles for table/parked rows — same join NightRunTab uses.
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

  // Poll the report while mounted. The drawer only mounts this variant while
  // the ready flag is lit (plan exists, run finished), so a failed fetch just
  // keeps the last known report — it clears entirely only when the drawer
  // unmounts the variant.
  useEffect(() => {
    let disposed = false;
    const load = async () => {
      try {
        const r = await api.getRunReport(projectPath);
        if (!disposed) setReport(r);
      } catch (err) {
        console.warn("getRunReport failed", err);
      }
    };
    load();
    const id = setInterval(load, POLL_MS);
    return () => {
      disposed = true;
      clearInterval(id);
    };
  }, [projectPath]);

  const resolve = (executionId: string) =>
    setResolved((prev) => {
      const next = new Set(prev);
      next.add(executionId);
      return next;
    });

  // Same flow as the night variant's inline parked card (Phase 1, item 2):
  // pin the answers into the phase's interview and requeue it in one IPC call.
  const handleAnswer = async (executionId: string, answers: AskUserQuestionAnswers) => {
    setAnsweringId(executionId);
    try {
      await api.answerParkedQuestion(projectPath, executionId, answers);
      resolve(executionId);
      toast.success("Answers pinned — phase requeued");
    } catch (err) {
      const appErr = err as AppError;
      toast.error("Failed to answer parked question", {
        description: appErr.message ?? String(err),
      });
    } finally {
      setAnsweringId(null);
    }
  };

  // Raw-payload fallback, same flow as the night variant: requeue the phase
  // (no structured answers to pin), then resume the run.
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

  // Optimistically drop resolved phases until the poll catches up. When all
  // phases are hidden, render nothing — the drawer's ready flag clears within
  // one poll and unmounts this variant for the day one.
  const visibleReport = report
    ? { ...report, phases: report.phases.filter((p) => !resolved.has(p.execution_id)) }
    : null;

  return (
    <div className="mx-auto min-h-0 w-full max-w-2xl flex-1 overflow-y-auto pb-6">
      <div className="mb-5 flex items-center gap-2">
        <Sunrise size={14} className="text-[var(--primary)]" />
        <h2 className="text-sm font-semibold tracking-tight">Morning report</h2>
        <span className="text-[10px] text-muted-foreground">
          {report
            ? `${report.plan.phases.length} phase${report.plan.phases.length !== 1 ? "s" : ""} · overnight run of ${new Date(report.plan.created).toLocaleDateString()}`
            : "loading…"}
        </span>
      </div>

      {visibleReport && visibleReport.phases.length > 0 && (
        <MorningReportView
          report={visibleReport}
          idToTitle={idToTitle}
          onRetry={handleRequeue}
          retryingId={requeueingId}
          onAnswerParked={handleAnswer}
          answeringId={answeringId}
        />
      )}
    </div>
  );
}
