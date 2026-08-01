import { useState, useEffect } from "react";
import {
  Loader2,
  Play,
  Square,
  MessageCircleQuestion,
  SkipForward,
  AlertTriangle,
} from "lucide-react";
import { toast } from "sonner";
import type { RunPlan, RunPhaseStatus, AppError } from "../../types";
import * as api from "../../lib/tauri";

const STATUS_LABEL: Record<RunPhaseStatus, string> = {
  queued: "queued",
  running: "running",
  parked: "parked",
  completed: "completed",
  failed: "failed",
  interrupted: "interrupted",
  killed: "killed",
};

const STATUS_COLOR: Record<RunPhaseStatus, string> = {
  queued: "var(--muted-foreground)",
  running: "var(--primary)",
  parked: "var(--warning)",
  completed: "var(--success)",
  failed: "var(--destructive)",
  interrupted: "var(--muted-foreground)",
  killed: "var(--muted-foreground)",
};

const POLL_INTERVAL_MS = 3000;

interface RunQueuePanelProps {
  projectPath: string;
  plan: RunPlan;
  onPlanChange: (plan: RunPlan | null) => void;
}

/**
 * Live status view + pre-flight interview gate for a project's queued
 * overnight run (`prd-run-queue` Phase 5). Polls `getRunStatus` while
 * mounted — a run is hours long, driven by a detached background executor,
 * not a response this component owns.
 *
 * Interview answers happen through the same `AskUserQuestion` card the chat
 * already shows: `runPhaseInterview` awaits the whole turn, and if the agent
 * asks a genuine clarifying question it parks in the shared per-project
 * question slot — `ProjectDetail`'s tab-agnostic `StuckQuestionCallout`
 * (mounted above every tab, including this one) renders and answers it. No
 * bespoke question card is needed here.
 */
export function RunQueuePanel({ projectPath, plan, onPlanChange }: RunQueuePanelProps) {
  const [interviewing, setInterviewing] = useState<string | null>(null);
  const [skipping, setSkipping] = useState<string | null>(null);
  const [starting, setStarting] = useState(false);
  const [cancelling, setCancelling] = useState(false);

  useEffect(() => {
    const id = setInterval(async () => {
      try {
        const latest = await api.getRunStatus(projectPath);
        onPlanChange(latest);
      } catch {
        // Transient read failure — the next tick retries.
      }
    }, POLL_INTERVAL_MS);
    return () => clearInterval(id);
  }, [projectPath, onPlanChange]);

  const handleInterview = async (executionId: string) => {
    setInterviewing(executionId);
    try {
      const updated = await api.runPhaseInterview(projectPath, executionId);
      onPlanChange(updated);
    } catch (err) {
      const appErr = err as AppError;
      toast.error("Interview turn failed", {
        description: appErr.message ?? String(err),
      });
    } finally {
      setInterviewing(null);
    }
  };

  const handleSkip = async (executionId: string) => {
    setSkipping(executionId);
    try {
      const updated = await api.skipPhaseInterview(projectPath, executionId);
      onPlanChange(updated);
    } catch (err) {
      const appErr = err as AppError;
      toast.error("Failed to skip interview", {
        description: appErr.message ?? String(err),
      });
    } finally {
      setSkipping(null);
    }
  };

  const queuedPhases = plan.phases.filter((p) => p.status === "queued");
  const pendingInterview = queuedPhases.filter(
    (p) => p.interview_status === "pending",
  );
  const runInProgress = plan.phases.some(
    (p) => p.status === "running" || p.status === "parked",
  );
  const canStart =
    queuedPhases.length > 0 && pendingInterview.length === 0 && !runInProgress;

  const handleStart = async () => {
    setStarting(true);
    try {
      await api.queueRun(projectPath);
      onPlanChange(await api.getRunStatus(projectPath));
      toast.success("Overnight run started");
    } catch (err) {
      const appErr = err as AppError;
      toast.error("Failed to start run", {
        description: appErr.message ?? String(err),
      });
    } finally {
      setStarting(false);
    }
  };

  const handleCancel = async () => {
    setCancelling(true);
    try {
      await api.cancelRun(projectPath);
      toast.success("Run cancelled");
    } catch (err) {
      const appErr = err as AppError;
      toast.error("Failed to cancel run", {
        description: appErr.message ?? String(err),
      });
    } finally {
      setCancelling(false);
    }
  };

  return (
    <div className="mb-4 rounded-xl border border-border bg-card p-4">
      <div className="mb-3 flex items-center justify-between">
        <h3 className="text-sm font-semibold text-foreground">Overnight run</h3>
        {runInProgress ? (
          <button
            onClick={handleCancel}
            disabled={cancelling}
            className="flex items-center gap-1.5 rounded-md px-2 py-1 text-[11px] font-medium text-destructive transition-colors hover:bg-accent disabled:opacity-50"
          >
            {cancelling ? (
              <Loader2 size={11} className="animate-spin" />
            ) : (
              <Square size={11} />
            )}
            Cancel run
          </button>
        ) : (
          <button
            onClick={handleStart}
            disabled={!canStart || starting}
            title={
              pendingInterview.length > 0
                ? "Answer or skip every phase's pre-flight interview first"
                : queuedPhases.length === 0
                  ? "No queued phases left to run"
                  : "Start the overnight run"
            }
            className="flex items-center gap-1.5 rounded-md bg-[var(--primary)] px-2.5 py-1 text-[11px] font-medium text-[var(--primary-foreground)] transition-colors hover:opacity-90 disabled:opacity-40"
          >
            {starting ? (
              <Loader2 size={11} className="animate-spin" />
            ) : (
              <Play size={11} />
            )}
            Start run
          </button>
        )}
      </div>

      <ul className="space-y-1.5">
        {plan.phases.map((phase) => {
          const isInterviewing = interviewing === phase.execution_id;
          const isSkipping = skipping === phase.execution_id;
          return (
            <li
              key={phase.execution_id}
              className="flex items-center gap-2 rounded px-1.5 py-1 text-xs"
            >
              <span
                className="shrink-0 rounded px-1.5 py-0.5 text-[10px] font-medium uppercase tracking-wider"
                style={{ color: STATUS_COLOR[phase.status] }}
              >
                {STATUS_LABEL[phase.status]}
              </span>
              <span className="flex-1 truncate font-mono text-[11px] text-foreground">
                {phase.execution_id}
              </span>
              {phase.status === "queued" && phase.interview_status === "pending" && (
                <>
                  <button
                    onClick={() => handleInterview(phase.execution_id)}
                    disabled={isInterviewing || isSkipping}
                    title="Run this phase's pre-flight clarifying-question turn"
                    className="flex shrink-0 items-center gap-1 rounded px-1.5 py-0.5 text-[10px] font-medium text-muted-foreground transition-colors hover:bg-accent hover:text-foreground disabled:opacity-50"
                  >
                    {isInterviewing ? (
                      <Loader2 size={10} className="animate-spin" />
                    ) : (
                      <MessageCircleQuestion size={10} />
                    )}
                    Answer
                  </button>
                  <button
                    onClick={() => handleSkip(phase.execution_id)}
                    disabled={isInterviewing || isSkipping}
                    title="Skip — judged unambiguous"
                    className="flex shrink-0 items-center gap-1 rounded px-1.5 py-0.5 text-[10px] font-medium text-muted-foreground transition-colors hover:bg-accent hover:text-foreground disabled:opacity-50"
                  >
                    {isSkipping ? (
                      <Loader2 size={10} className="animate-spin" />
                    ) : (
                      <SkipForward size={10} />
                    )}
                    Skip
                  </button>
                </>
              )}
              {phase.status === "queued" && phase.interview_status !== "pending" && (
                <span className="shrink-0 text-[10px] text-muted-foreground">
                  interview {phase.interview_status}
                </span>
              )}
              {phase.park_payload && (
                <span
                  className="flex shrink-0 items-center gap-1 text-[10px] text-muted-foreground"
                  title={phase.park_payload}
                >
                  <AlertTriangle size={10} style={{ color: "var(--warning)" }} />
                  <span className="max-w-[16rem] truncate">{phase.park_payload}</span>
                </span>
              )}
            </li>
          );
        })}
      </ul>
    </div>
  );
}
