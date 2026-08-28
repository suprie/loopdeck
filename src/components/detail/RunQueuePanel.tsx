import { useCallback, useEffect, useState } from "react";
import { Activity, AlertTriangle, Loader2, Play, RotateCcw, Square as StopIcon, Zap } from "lucide-react";
import { toast } from "sonner";
import type { AppError, ContentBlock, RunBudgets, RunPhase, RunPlan, RunReport, StallPolicy } from "../../types";
import * as api from "../../lib/tauri";
import {
  STATUS_COLOR,
  STATUS_LABEL,
} from "../../lib/nightRun";
import { MorningReportView } from "./MorningReportView";
import { useStreamingState } from "../../store/streamingState";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "../ui/select";

// Poll interval for the live run-queue view. Cheap local IPC (reads
// run-plan.yaml), so polling unconditionally — even before a plan exists —
// is simpler than gating the interval on plan presence.
const POLL_MS = 5000;

interface RunQueuePanelProps {
  projectPath: string;
  /** Execution IDs currently checked in the phase picker, in selection order. */
  selectedIds: string[];
  /** Stable execution ID -> loop title, for human-readable phase rows. */
  idToTitle: Record<string, string>;
  /** Called after a run plan is successfully queued, so the caller can clear
   * its picker selection. */
  onQueued: () => void;
}

/** Phase picker action bar + live run-queue status view (prd-run-queue Phase 5). */
export function RunQueuePanel({
  projectPath,
  selectedIds,
  idToTitle,
  onQueued,
}: RunQueuePanelProps) {
  const [plan, setPlan] = useState<RunPlan | null>(null);
  const [stallPolicy, setStallPolicy] = useState<StallPolicy>("continue_independent");
  const [draftPrAuthorized, setDraftPrAuthorized] = useState(true);
  const [phaseTokenCap, setPhaseTokenCap] = useState("500000");
  const [phaseMinutes, setPhaseMinutes] = useState("90");
  const [runHours, setRunHours] = useState("8");
  const [queuing, setQueuing] = useState(false);
  const [starting, setStarting] = useState(false);
  const [cancelling, setCancelling] = useState(false);
  const [interviewingId, setInterviewingId] = useState<string | null>(null);
  const [skippingId, setSkippingId] = useState<string | null>(null);
  const [retryingId, setRetryingId] = useState<string | null>(null);
  const [retryingAll, setRetryingAll] = useState(false);
  const [answeringId, setAnsweringId] = useState<string | null>(null);
  const [runActive, setRunActive] = useState(false);
  const [report, setReport] = useState<RunReport | null>(null);
  const live = useStreamingState((s) => s.byPath[projectPath] ?? null);

  const loadPlan = useCallback(async () => {
    try {
      const loaded = await api.getRunStatus(projectPath);
      setPlan(loaded.plan);
      setRunActive(loaded.active);
      if (!loaded.active && useStreamingState.getState().get(projectPath).busy) {
        useStreamingState.getState().patch(projectPath, { busy: false, retrying: null });
      }
    } catch (err) {
      console.warn("getRunStatus failed", err);
    }
  }, [projectPath]);

  // Load the morning report when the run is finished (not active, plan exists).
  const loadReport = useCallback(async () => {
    if (!plan || runActive) return;
    try {
      const r = await api.getRunReport(projectPath);
      setReport(r);
    } catch (err) {
      console.warn("getRunReport failed", err);
    }
  }, [projectPath, plan, runActive]);

  useEffect(() => {
    loadPlan();
    const id = setInterval(loadPlan, POLL_MS);
    return () => clearInterval(id);
  }, [loadPlan]);

  useEffect(() => {
    loadReport();
  }, [loadReport]);

  const handleQueue = async () => {
    setQueuing(true);
    try {
      const budgets: RunBudgets = {
        per_phase_token_cap: Number(phaseTokenCap),
        per_phase_wall_clock_secs: Number(phaseMinutes) * 60,
        total_run_wall_clock_secs: Number(runHours) * 60 * 60,
      };
      if (Object.values(budgets).some((value) => !Number.isSafeInteger(value) || value! <= 0)) {
        throw new Error("Budget values must be positive whole numbers");
      }
      const created = await api.createRunPlan(
        projectPath,
        selectedIds,
        stallPolicy,
        draftPrAuthorized,
        budgets,
      );
      setPlan(created);
      onQueued();
      toast.success("Run plan queued", {
        description: `${created.phases.length} phase${created.phases.length !== 1 ? "s" : ""} — answer each phase's pre-flight interview to unlock Start run.`,
      });
    } catch (err) {
      const appErr = err as AppError;
      toast.error("Failed to queue run", { description: appErr.message ?? String(err) });
    } finally {
      setQueuing(false);
    }
  };

  const handleStartRun = async () => {
    setStarting(true);
    try {
      useStreamingState.getState().beginTurn(projectPath);
      await api.queueRun(projectPath);
      toast.success("Overnight run started");
      await loadPlan();
    } catch (err) {
      useStreamingState.getState().clear(projectPath);
      const appErr = err as AppError;
      toast.error("Failed to start run", { description: appErr.message ?? String(err) });
    } finally {
      setStarting(false);
    }
  };

  const handleRetry = async (executionId: string) => {
    setRetryingId(executionId);
    try {
      const updated = await api.requeueRunPhase(projectPath, executionId);
      setPlan(updated);
      useStreamingState.getState().beginTurn(projectPath);
      await api.queueRun(projectPath);
      toast.success("Parked phase restarted unattended");
      await loadPlan();
    } catch (err) {
      useStreamingState.getState().clear(projectPath);
      const appErr = err as AppError;
      toast.error("Failed to retry phase", { description: appErr.message ?? String(err) });
    } finally {
      setRetryingId(null);
    }
  };

  const handleRetryFailed = async () => {
    setRetryingAll(true);
    try {
      const updated = await api.requeueFailedRunPhases(projectPath);
      setPlan(updated);
      useStreamingState.getState().beginTurn(projectPath);
      await api.queueRun(projectPath);
      toast.success("Failed phases restarted as one combined run");
      await loadPlan();
    } catch (err) {
      useStreamingState.getState().clear(projectPath);
      const appErr = err as AppError;
      toast.error("Failed to retry phases", { description: appErr.message ?? String(err) });
    } finally {
      setRetryingAll(false);
    }
  };

  const handleAnswerParked = async (
    executionId: string,
    answers: Parameters<typeof api.agentAnswerQuestion>[2],
  ) => {
    setAnsweringId(executionId);
    try {
      const updated = await api.answerParkedQuestion(projectPath, executionId, answers);
      setPlan(updated);
      toast.success("Answers pinned — phase requeued");
      await loadReport();
    } catch (err) {
      const appErr = err as AppError;
      toast.error("Failed to answer parked question", {
        description: appErr.message ?? String(err),
      });
    } finally {
      setAnsweringId(null);
    }
  };

  const handleCancel = async () => {
    setCancelling(true);
    try {
      await api.cancelRun(projectPath);
      toast.success("Cancel requested");
      await loadPlan();
    } catch (err) {
      const appErr = err as AppError;
      toast.error("Failed to cancel run", { description: appErr.message ?? String(err) });
    } finally {
      setCancelling(false);
    }
  };

  const handleAnswer = async (executionId: string) => {
    setInterviewingId(executionId);
    try {
      const updated = await api.runPhaseInterview(projectPath, executionId);
      setPlan(updated);
    } catch (err) {
      const appErr = err as AppError;
      toast.error("Interview turn failed", { description: appErr.message ?? String(err) });
    } finally {
      setInterviewingId(null);
    }
  };

  const handleSkip = async (executionId: string) => {
    setSkippingId(executionId);
    try {
      const updated = await api.skipPhaseInterview(projectPath, executionId);
      setPlan(updated);
    } catch (err) {
      const appErr = err as AppError;
      toast.error("Failed to skip interview", { description: appErr.message ?? String(err) });
    } finally {
      setSkippingId(null);
    }
  };

  const isRunning = runActive;
  const retryableCount =
    plan?.phases.filter((p) =>
      ["parked", "failed", "interrupted", "killed"].includes(p.status),
    ).length ?? 0;
  const hasQueuedPhase = plan?.phases.some((p) => p.status === "queued") ?? false;
  const hasPendingInterview =
    plan?.phases.some((p) => p.status === "queued" && p.interview_status === "pending") ?? false;
  const canStart = !isRunning && hasQueuedPhase && !hasPendingInterview;
  const startDisabledReason = isRunning
    ? "A run is already in progress"
    : hasPendingInterview
      ? "Answer or skip every queued phase's pre-flight interview first"
      : !hasQueuedPhase
        ? "No queued phases to run"
        : undefined;

  return (
    <>
      {selectedIds.length > 0 && (
        <div className="mb-4 flex flex-wrap items-center gap-2 rounded-lg border border-border bg-card p-3">
          <span className="text-xs font-medium text-foreground">
            {selectedIds.length} phase{selectedIds.length !== 1 ? "s" : ""} selected
          </span>
          <Select value={stallPolicy} onValueChange={(v) => setStallPolicy(v as StallPolicy)}>
            <SelectTrigger className="h-7 w-48 text-xs">
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              <SelectItem value="continue_independent">Continue independent phases</SelectItem>
              <SelectItem value="halt">Halt on stall</SelectItem>
            </SelectContent>
          </Select>
          <label className="flex items-center gap-1.5 text-xs text-muted-foreground">
            <input
              type="checkbox"
              checked={draftPrAuthorized}
              onChange={(e) => setDraftPrAuthorized(e.target.checked)}
              className="size-3.5"
            />
            Open draft PR automatically
          </label>
          <label className="flex items-center gap-1 text-xs text-muted-foreground">
            Tokens/phase
            <input
              type="number"
              min="1"
              value={phaseTokenCap}
              onChange={(event) => setPhaseTokenCap(event.target.value)}
              className="h-7 w-24 rounded border border-border bg-background px-1.5 text-xs text-foreground"
            />
          </label>
          <label className="flex items-center gap-1 text-xs text-muted-foreground">
            Minutes/phase
            <input
              type="number"
              min="1"
              value={phaseMinutes}
              onChange={(event) => setPhaseMinutes(event.target.value)}
              className="h-7 w-14 rounded border border-border bg-background px-1.5 text-xs text-foreground"
            />
          </label>
          <label className="flex items-center gap-1 text-xs text-muted-foreground">
            Hours/run
            <input
              type="number"
              min="1"
              value={runHours}
              onChange={(event) => setRunHours(event.target.value)}
              className="h-7 w-12 rounded border border-border bg-background px-1.5 text-xs text-foreground"
            />
          </label>
          <button
            onClick={handleQueue}
            disabled={queuing}
            className="ml-auto flex items-center gap-1.5 rounded-md bg-[var(--primary)] px-2.5 py-1 text-[11px] font-medium text-[var(--primary-foreground)] transition-opacity hover:opacity-90 disabled:opacity-50"
          >
            {queuing ? <Loader2 size={12} className="animate-spin" /> : <Zap size={12} />}
            Queue overnight run
          </button>
          <p className="basis-full text-[11px] leading-relaxed text-muted-foreground">
            Unattended mode automatically allows safe project-scoped actions and plan execution.
            Destructive actions remain denied by the safety floor.
          </p>
        </div>
      )}

      {plan && (
        <div className="mb-4 rounded-lg border border-border bg-card p-3">
          <div className="mb-2 flex items-center justify-between gap-2">
            <div className="flex items-center gap-2">
              <span className="text-xs font-semibold text-foreground">Run queue</span>
              <span className="text-[10px] text-muted-foreground">
                {plan.phases.length} phase{plan.phases.length !== 1 ? "s" : ""} ·{" "}
                {plan.stall_policy === "halt" ? "halt on stall" : "continue independent"}
                {plan.environment.worktree_kept ? " · worktree kept for review" : ""}
              </span>
            </div>
            {isRunning ? (
              <button
                onClick={handleCancel}
                disabled={cancelling}
                className="flex items-center gap-1.5 rounded-md px-2 py-1 text-[11px] font-medium text-[var(--destructive)] transition-colors hover:bg-accent disabled:opacity-50"
              >
                {cancelling ? (
                  <Loader2 size={11} className="animate-spin" />
                ) : (
                  <StopIcon size={11} />
                )}
                Cancel run
              </button>
            ) : (
              <div className="flex items-center gap-1.5">
                {retryableCount > 0 && (
                  <button
                    onClick={handleRetryFailed}
                    disabled={retryingAll}
                    title={`Requeue all ${retryableCount} parked/failed phase(s) and restart the run as one combined session`}
                    className="flex items-center gap-1.5 rounded-md border border-border px-2.5 py-1 text-[11px] font-medium text-foreground transition-colors hover:bg-accent disabled:cursor-not-allowed disabled:opacity-50"
                  >
                    {retryingAll ? (
                      <Loader2 size={11} className="animate-spin" />
                    ) : (
                      <RotateCcw size={11} />
                    )}
                    Retry failed ({retryableCount})
                  </button>
                )}
                <button
                  onClick={handleStartRun}
                  disabled={!canStart || starting}
                  title={startDisabledReason}
                  className="flex items-center gap-1.5 rounded-md bg-[var(--primary)] px-2.5 py-1 text-[11px] font-medium text-[var(--primary-foreground)] transition-opacity hover:opacity-90 disabled:cursor-not-allowed disabled:opacity-40"
                >
                  {starting ? (
                    <Loader2 size={11} className="animate-spin" />
                  ) : (
                    <Play size={11} />
                  )}
                  Start run
                </button>
              </div>
            )}
          </div>

          <ul className="space-y-1">
            {plan.phases.map((phase) => (
              <RunPhaseRow
                key={phase.execution_id}
                phase={phase}
                title={idToTitle[phase.execution_id] ?? phase.execution_id}
                interviewing={interviewingId === phase.execution_id}
                skipping={skippingId === phase.execution_id}
                retrying={retryingId === phase.execution_id}
                onAnswer={() => handleAnswer(phase.execution_id)}
                onSkip={() => handleSkip(phase.execution_id)}
                onRetry={() => handleRetry(phase.execution_id)}
              />
            ))}
          </ul>

          {live?.streamingBlocks && live.streamingBlocks.length > 0 && (
            <LiveRunActivity blocks={live.streamingBlocks} busy={runActive} />
          )}
        </div>
      )}

      {/* Morning report — rendered when the run is finished and report data is
          loaded. Shared view (extracted per prd-night-run-surfaces Phase 3):
          the drawer's morning-report variant renders the same component. */}
      {report && !runActive && (
        <MorningReportView
          report={report}
          idToTitle={idToTitle}
          onRetry={handleRetry}
          retryingId={retryingId}
          onAnswerParked={handleAnswerParked}
          answeringId={answeringId}
        />
      )}
    </>
  );
}

function RunPhaseRow({
  phase,
  title,
  interviewing,
  skipping,
  retrying,
  onAnswer,
  onSkip,
  onRetry,
}: {
  phase: RunPhase;
  title: string;
  interviewing: boolean;
  skipping: boolean;
  retrying: boolean;
  onAnswer: () => void;
  onSkip: () => void;
  onRetry: () => void;
}) {
  const needsInterview = phase.status === "queued" && phase.interview_status === "pending";
  const busy = interviewing || skipping;

  return (
    <li className="rounded px-1.5 py-1 text-xs">
      <div className="flex items-center gap-2">
      <span
        className="w-20 shrink-0 rounded px-1.5 py-0.5 text-center text-[10px] font-medium uppercase tracking-wider"
        style={{ color: STATUS_COLOR[phase.status] }}
      >
        {STATUS_LABEL[phase.status]}
      </span>
      <span className="flex-1 truncate text-foreground" title={phase.execution_id}>
        {title}
      </span>
      {phase.token_usage > 0 && (
        <span className="shrink-0 text-[10px] text-muted-foreground">
          {phase.token_usage.toLocaleString()} tok · {phase.wall_clock_secs}s
        </span>
      )}
      {needsInterview ? (
        <span className="flex shrink-0 items-center gap-1">
          <button
            onClick={onAnswer}
            disabled={busy}
            className="rounded px-1.5 py-0.5 text-[10px] font-medium text-muted-foreground transition-colors hover:bg-accent hover:text-foreground disabled:opacity-50"
          >
            {interviewing ? <Loader2 size={10} className="animate-spin" /> : "Answer"}
          </button>
          <button
            onClick={onSkip}
            disabled={busy}
            className="rounded px-1.5 py-0.5 text-[10px] font-medium text-muted-foreground transition-colors hover:bg-accent hover:text-foreground disabled:opacity-50"
          >
            {skipping ? <Loader2 size={10} className="animate-spin" /> : "Skip"}
          </button>
        </span>
      ) : ["parked", "failed", "interrupted", "killed"].includes(phase.status) ? (
        <button
          onClick={onRetry}
          disabled={retrying}
          className="flex shrink-0 items-center gap-1 rounded px-1.5 py-0.5 text-[10px] font-medium text-[var(--warning)] transition-colors hover:bg-accent disabled:opacity-50"
        >
          {retrying ? <Loader2 size={10} className="animate-spin" /> : <RotateCcw size={10} />}
          Retry unattended
        </button>
      ) : (
        phase.status === "queued" && (
          <span className="shrink-0 text-[10px] text-muted-foreground">
            {phase.interview_status}
          </span>
        )
      )}
      </div>
      {phase.park_payload && (
        <div className="mt-1 flex items-start gap-1.5 rounded bg-amber-500/5 px-2 py-1.5 text-[11px] leading-relaxed text-[var(--warning)]">
          <AlertTriangle size={11} className="mt-0.5 shrink-0" />
          <span className="break-words">{phase.park_payload}</span>
        </div>
      )}
    </li>
  );
}

function LiveRunActivity({ blocks, busy }: { blocks: ContentBlock[]; busy: boolean }) {
  const visible = blocks.slice(-8);
  return (
    <div className="mt-3 rounded-md border border-border bg-background/60 p-2.5">
      <div className="mb-2 flex items-center gap-1.5 text-[10px] font-semibold uppercase tracking-wider text-muted-foreground">
        {busy ? <Loader2 size={11} className="animate-spin" /> : <Activity size={11} />}
        {busy ? "Agent running live" : "Latest agent activity"}
      </div>
      <div className="max-h-52 space-y-1.5 overflow-y-auto font-mono text-[11px] leading-relaxed">
        {visible.map((block, index) => {
          if (block.type === "tool_use") {
            return (
              <div key={index} className="text-violet-400">
                › {block.name} <span className="text-muted-foreground">{summarize(block.input, 180)}</span>
              </div>
            );
          }
          const text = block.type === "text" ? block.text : block.thinking;
          return (
            <div key={index} className={block.type === "thinking" ? "text-muted-foreground italic" : "text-foreground"}>
              {summarize(text, 500)}
            </div>
          );
        })}
      </div>
    </div>
  );
}

function summarize(value: string, max: number) {
  const compact = value.replace(/\s+/g, " ").trim();
  return compact.length > max ? `${compact.slice(0, max)}…` : compact;
}
