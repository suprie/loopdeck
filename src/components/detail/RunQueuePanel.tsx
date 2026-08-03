import { useCallback, useEffect, useState } from "react";
import { AlertTriangle, Loader2, Play, Square as StopIcon, Zap } from "lucide-react";
import { toast } from "sonner";
import type { AppError, RunBudgets, RunPhase, RunPhaseStatus, RunPlan, StallPolicy } from "../../types";
import * as api from "../../lib/tauri";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "../ui/select";

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
  interrupted: "var(--warning)",
  killed: "var(--destructive)",
};

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

  const loadPlan = useCallback(async () => {
    try {
      const loaded = await api.getRunStatus(projectPath);
      setPlan(loaded);
    } catch (err) {
      console.warn("getRunStatus failed", err);
    }
  }, [projectPath]);

  useEffect(() => {
    loadPlan();
    const id = setInterval(loadPlan, POLL_MS);
    return () => clearInterval(id);
  }, [loadPlan]);

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
      await api.queueRun(projectPath);
      toast.success("Overnight run started");
      await loadPlan();
    } catch (err) {
      const appErr = err as AppError;
      toast.error("Failed to start run", { description: appErr.message ?? String(err) });
    } finally {
      setStarting(false);
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

  const isRunning = plan?.phases.some((p) => p.status === "running") ?? false;
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
            Authorize draft PR
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
                onAnswer={() => handleAnswer(phase.execution_id)}
                onSkip={() => handleSkip(phase.execution_id)}
              />
            ))}
          </ul>
        </div>
      )}
    </>
  );
}

function RunPhaseRow({
  phase,
  title,
  interviewing,
  skipping,
  onAnswer,
  onSkip,
}: {
  phase: RunPhase;
  title: string;
  interviewing: boolean;
  skipping: boolean;
  onAnswer: () => void;
  onSkip: () => void;
}) {
  const needsInterview = phase.status === "queued" && phase.interview_status === "pending";
  const busy = interviewing || skipping;

  return (
    <li className="flex items-center gap-2 rounded px-1.5 py-1 text-xs">
      <span
        className="w-20 shrink-0 rounded px-1.5 py-0.5 text-center text-[10px] font-medium uppercase tracking-wider"
        style={{ color: STATUS_COLOR[phase.status] }}
      >
        {STATUS_LABEL[phase.status]}
      </span>
      <span className="flex-1 truncate text-foreground" title={phase.execution_id}>
        {title}
      </span>
      {phase.park_payload && (
        <span title={phase.park_payload} className="shrink-0">
          <AlertTriangle size={11} style={{ color: "var(--warning)" }} />
        </span>
      )}
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
      ) : (
        phase.status === "queued" && (
          <span className="shrink-0 text-[10px] text-muted-foreground">
            {phase.interview_status}
          </span>
        )
      )}
    </li>
  );
}
