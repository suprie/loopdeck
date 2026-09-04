import { useEffect, useMemo, useState } from "react";
import { Loader2, Moon } from "lucide-react";
import { toast } from "sonner";
import * as api from "../../lib/tauri";
import { dependencyLabel, hasQueueablePhases } from "../../lib/nightRun";
import { buildIdToTitle } from "./EpicsPanel";
import { AskUserQuestionCard } from "./AskUserQuestionCard";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
} from "../ui/dialog";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "../ui/select";
import { cn } from "../../lib/utils";
import { useStreamingState } from "../../store/streamingState";
import type {
  AppError,
  AskUserQuestionAnswers,
  Epic,
  NamedAgentConfig,
  PendingQuestionEntry,
  PhaseAgentAssignment,
  PrdLoop,
  RunBudgets,
  RunPlan,
  StallPolicy,
} from "../../types";

/** Step indicator for the wizard header. */
const STEPS = ["Phases", "Interviews", "Consent"] as const;

/** Queueable (id-bearing, unchecked, not-in-history) loops flattened with
 *  their epic/PRD/phase breadcrumbs, in authored order — the picker list. */
interface PickerEntry {
  loop: PrdLoop;
  group: string;
}

function pickerEntries(epics: Epic[]): PickerEntry[] {
  const entries: PickerEntry[] = [];
  for (const epic of epics) {
    for (const prd of epic.prds) {
      for (const phase of prd.phases) {
        for (const loop of phase.loops) {
          if (loop.id && !loop.checked && !loop.done_in_history) {
            entries.push({ loop, group: `${epic.title} · ${prd.slug} · ${phase.name}` });
          }
        }
      }
    }
  }
  return entries;
}

interface PlanTonightWizardProps {
  projectPath: string;
  epics: Epic[];
  open: boolean;
  onOpenChange: (open: boolean) => void;
  /** Fired after `queueRun` resolves — the drawer closes the wizard and
   *  auto-switches to the Agent tab, which now renders the night variant
   *  (the plan it polls is active/queued). Per the run's pre-answered
   *  clarification: "close wizard, auto-switch." */
  onStarted?: () => void;
}

/**
 * The "Plan tonight" 3-step wizard (prd-night-run-surfaces Phase 2, item 1):
 * phase picker (with dependency labels) + stall policy + budgets → live
 * pre-flight interviews → consent summary + required checkbox. Reuses the
 * run-plan IPC surface — `createRunPlan`, `runPhaseInterview`,
 * `runBatchPhaseInterviews`, and `skipPhaseInterview`.
 *
 * Per the run's pre-answered clarifications:
 * - `createRunPlan` fires on the step 1→2 transition with the draft-PR
 *   consent checkbox pre-checked; step 3's required checkbox then gates only
 *   the final queue-run action. Abandoning the wizard mid-way leaves the
 *   created plan queued-but-unstarted (same state the current panel leaves).
 * - Step 2 runs live interviews inline: `runBatchPhaseInterviews` streams all
 *   pending loops in one context-rich turn (with `runPhaseInterview` retained
 *   for one-off overrides). The agent
 *   turn server-side and parks any `AskUserQuestion` on the shared
 *   pending-question slot, which this wizard polls for and renders as the
 *   same `AskUserQuestionCard` chat shows (answer fields = the "text
 *   inputs"). Submitting answers via `agentAnswerQuestion` — the shared-slot
 *   answer path, functionally the wizard-side equivalent of the night
 *   variant's `answerParkedQuestion` — unblocks and resolves the interview,
 *   pinning the answers into the phase.
 */
export function PlanTonightWizard({
  projectPath,
  epics,
  open,
  onOpenChange,
  onStarted,
}: PlanTonightWizardProps) {
  const [step, setStep] = useState<1 | 2 | 3>(1);
  const [selected, setSelected] = useState<string[]>([]);
  const [stallPolicy, setStallPolicy] = useState<StallPolicy>("continue_independent");
  const [draftPrAuthorized, setDraftPrAuthorized] = useState(true);
  const [phaseTokenCap, setPhaseTokenCap] = useState("500000");
  const [phaseMinutes, setPhaseMinutes] = useState("90");
  const [runHours, setRunHours] = useState("8");
  const [creating, setCreating] = useState(false);
  const [starting, setStarting] = useState(false);
  const [plan, setPlan] = useState<RunPlan | null>(null);
  const [interviewingId, setInterviewingId] = useState<string | null>(null);
  const [batchInterviewing, setBatchInterviewing] = useState(false);
  const [skippingId, setSkippingId] = useState<string | null>(null);
  const [pendingQuestion, setPendingQuestion] = useState<PendingQuestionEntry | null>(null);
  const [consentConfirmed, setConsentConfirmed] = useState(false);
  // Per-phase staffing (prd-role-foundations Phase 4): executionId → roster
  // agent id. Empty string / missing = the default agent.
  const [roster, setRoster] = useState<NamedAgentConfig[]>([]);
  const [agentByPhase, setAgentByPhase] = useState<Record<string, string>>({});

  const idToTitle = useMemo(() => buildIdToTitle(epics), [epics]);
  const entries = useMemo(() => pickerEntries(epics), [epics]);

  // Load the roster once per open for the per-phase agent picker.
  useEffect(() => {
    if (!open) return;
    api
      .listAgentConfigs()
      .then(setRoster)
      .catch((err) => console.warn("listAgentConfigs failed", err));
  }, [open]);

  // Fresh wizard per open. A previously created (unstarted) plan stays on
  // disk queued-but-unstarted; finishing step 1 again simply replaces it —
  // the same replace semantics RunQueuePanel's Queue button has.
  useEffect(() => {
    if (open) return;
    setStep(1);
    setSelected([]);
    setDraftPrAuthorized(true);
    setCreating(false);
    setPlan(null);
    setInterviewingId(null);
    setBatchInterviewing(false);
    setSkippingId(null);
    setPendingQuestion(null);
    setConsentConfirmed(false);
    setAgentByPhase({});
  }, [open]);

  // While a live interview turn runs, poll the shared pending-question slot
  // so a parked AskUserQuestion renders inline below the running phase row.
  // (The interview's agent turn runs behind a no-op server channel, so the
  // question surfaces through the backend slot, not a streaming event.)
  useEffect(() => {
    if (interviewingId === null && !batchInterviewing) {
      setPendingQuestion(null);
      return;
    }
    let stopped = false;
    const tick = async () => {
      try {
        const list = await api.listPendingQuestions();
        if (!stopped) setPendingQuestion(list.find((e) => e.path === projectPath) ?? null);
      } catch (err) {
        console.warn("listPendingQuestions failed", err);
      }
    };
    tick();
    const interval = setInterval(tick, 1000);
    return () => {
      stopped = true;
      clearInterval(interval);
    };
  }, [interviewingId, batchInterviewing, projectPath]);

  const toggleSelected = (id: string) => {
    setSelected((prev) => (prev.includes(id) ? prev.filter((x) => x !== id) : [...prev, id]));
  };

  const setPhaseAgent = (id: string, agentId: string) => {
    setAgentByPhase((prev) => ({ ...prev, [id]: agentId }));
  };

  // Sparse staffing payload: only explicitly-assigned phases cross the IPC
  // boundary; unlisted phases stay on the default agent.
  const assignmentsFromSelection = (): PhaseAgentAssignment[] =>
    selected
      .filter((id) => agentByPhase[id])
      .map((id) => ({ execution_id: id, agent_id: agentByPhase[id] }));

  const budgetsFromInputs = (): RunBudgets => {
    const budgets: RunBudgets = {
      per_phase_token_cap: Number(phaseTokenCap),
      per_phase_wall_clock_secs: Number(phaseMinutes) * 60,
      total_run_wall_clock_secs: Number(runHours) * 60 * 60,
    };
    if (Object.values(budgets).some((v) => !Number.isSafeInteger(v) || v <= 0)) {
      throw new Error("Budget values must be positive whole numbers");
    }
    return budgets;
  };

  // Step 1 → 2: create the run plan now, so step 2's interviews have a plan
  // to run against (per the run's pre-answered clarification).
  const handleNextFromPhases = async () => {
    setCreating(true);
    try {
      const created = await api.createRunPlan(
        projectPath,
        selected,
        stallPolicy,
        draftPrAuthorized,
        budgetsFromInputs(),
        assignmentsFromSelection(),
      );
      setPlan(created);
      setStep(2);
    } catch (err) {
      const appErr = err as AppError;
      toast.error("Failed to queue run plan", { description: appErr.message ?? String(err) });
    } finally {
      setCreating(false);
    }
  };

  const handleInterview = async (executionId: string) => {
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

  const handleInterviewAll = async () => {
    const pendingIds = plan?.phases
      .filter((phase) => phase.status === "queued" && phase.interview_status === "pending")
      .map((phase) => phase.execution_id) ?? [];
    if (pendingIds.length === 0) return;

    setBatchInterviewing(true);
    try {
      const updated = await api.runBatchPhaseInterviews(projectPath, pendingIds);
      setPlan(updated);
    } catch (err) {
      const appErr = err as AppError;
      toast.error("Combined interview failed", { description: appErr.message ?? String(err) });
    } finally {
      setBatchInterviewing(false);
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

  // Inline parked-card submit: answer the shared pending-question slot, which
  // unblocks the in-flight interview turn — `runPhaseInterview` then resolves
  // with the answers pinned into the phase.
  const handleAnswerPending = async (answers: AskUserQuestionAnswers) => {
    if (!pendingQuestion) return;
    const { requestId } = pendingQuestion;
    try {
      await api.agentAnswerQuestion(projectPath, requestId, answers);
    } catch (err) {
      // The turn may have ended while the user deliberated — either way the
      // entry is stale for this interview. Same stance as the drawer's
      // StuckQuestionCallout.
      console.warn("agentAnswerQuestion failed", err);
    }
    setPendingQuestion(null);
  };

  const hasPendingInterview =
    plan?.phases.some((p) => p.status === "queued" && p.interview_status === "pending") ?? false;

  // The final action (prd-night-run-surfaces Phase 2, item 2): start the
  // queued run via the same `queueRun` IPC RunQueuePanel's Start button and
  // the executor (`run_executor.rs` via `queue_run`) already consume. No new
  // payload crosses the boundary — the phase/budget/consent shape was fixed
  // by `createRunPlan` on the 1→2 transition and persisted to
  // `.loopdeck/run-plan.yaml`; `queue_run` takes only the project path and
  // re-reads that file, refusing while any phase's interview is still
  // `pending` (the same gate as `canStart` in RunQueuePanel).
  const handleStartRun = async () => {
    setStarting(true);
    try {
      useStreamingState.getState().beginTurn(projectPath);
      await api.queueRun(projectPath);
      toast.success("Overnight run started");
      onStarted?.();
    } catch (err) {
      useStreamingState.getState().clear(projectPath);
      const appErr = err as AppError;
      toast.error("Failed to start run", { description: appErr.message ?? String(err) });
    } finally {
      setStarting(false);
    }
  };

  if (!hasQueueablePhases(epics)) return null;

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="flex max-h-[85vh] w-[640px] max-w-[92vw] flex-col gap-0 overflow-hidden p-0">
        <DialogHeader className="shrink-0 border-b border-border px-6 py-4 text-left">
          <DialogTitle className="flex items-center gap-2 font-display text-base">
            <Moon size={14} className="text-[var(--primary)]" />
            Plan tonight
          </DialogTitle>
          <DialogDescription className="text-xs">
            Queue an overnight run: pick phases, answer pre-flight interviews, consent, start.
          </DialogDescription>
          {/* Step indicator */}
          <ol className="mt-2 flex items-center gap-2 text-[11px]">
            {STEPS.map((label, i) => {
              const n = i + 1;
              const active = step === n;
              const done = step > n;
              return (
                <li key={label} className="flex items-center gap-2">
                  <span
                    className={cn(
                      "flex size-5 items-center justify-center rounded-full border text-[10px] font-semibold",
                      active
                        ? "border-[var(--primary)] text-[var(--primary)]"
                        : done
                          ? "border-[var(--success)] text-[var(--success)]"
                          : "border-border text-muted-foreground",
                    )}
                  >
                    {n}
                  </span>
                  <span className={active ? "font-medium text-foreground" : "text-muted-foreground"}>
                    {label}
                  </span>
                  {n < STEPS.length && <span className="text-muted-foreground">·</span>}
                </li>
              );
            })}
          </ol>
        </DialogHeader>

        <div className="min-h-0 flex-1 overflow-y-auto px-6 py-4">
          {step === 1 && (
            <StepPhases
              entries={entries}
              selected={selected}
              idToTitle={idToTitle}
              roster={roster}
              agentByPhase={agentByPhase}
              onSetAgent={setPhaseAgent}
              stallPolicy={stallPolicy}
              setStallPolicy={setStallPolicy}
              draftPrAuthorized={draftPrAuthorized}
              setDraftPrAuthorized={setDraftPrAuthorized}
              phaseTokenCap={phaseTokenCap}
              setPhaseTokenCap={setPhaseTokenCap}
              phaseMinutes={phaseMinutes}
              setPhaseMinutes={setPhaseMinutes}
              runHours={runHours}
              setRunHours={setRunHours}
              onToggle={toggleSelected}
            />
          )}

          {step === 2 && plan && (
            <StepInterviews
              plan={plan}
              idToTitle={idToTitle}
              interviewingId={interviewingId}
              batchInterviewing={batchInterviewing}
              skippingId={skippingId}
              pendingQuestion={pendingQuestion}
              onInterview={handleInterview}
              onInterviewAll={handleInterviewAll}
              onSkip={handleSkip}
              onAnswerPending={handleAnswerPending}
            />
          )}

          {step === 3 && plan && (
            <StepConsent
              plan={plan}
              idToTitle={idToTitle}
              consentConfirmed={consentConfirmed}
              setConsentConfirmed={setConsentConfirmed}
              hasPendingInterview={hasPendingInterview}
            />
          )}
        </div>

        {/* Action bar */}
        <div className="flex shrink-0 items-center justify-between gap-2 border-t border-border px-6 py-3">
          <button
            type="button"
            onClick={() => (step === 1 ? onOpenChange(false) : setStep((s) => (s - 1) as 1 | 2))}
            className="rounded-md px-3 py-1.5 text-xs text-muted-foreground transition-colors hover:bg-accent hover:text-foreground"
          >
            {step === 1 ? "Cancel" : "Back"}
          </button>

          {step === 1 && (
            <button
              type="button"
              onClick={handleNextFromPhases}
              disabled={selected.length === 0 || creating}
              className="flex items-center gap-1.5 rounded-md bg-[var(--primary)] px-3 py-1.5 text-xs font-medium text-[var(--primary-foreground)] transition-opacity hover:opacity-90 disabled:cursor-not-allowed disabled:opacity-40"
            >
              {creating && <Loader2 size={12} className="animate-spin" />}
              Next: interviews
            </button>
          )}

          {step === 2 && (
            <button
              type="button"
              onClick={() => setStep(3)}
              className="rounded-md bg-[var(--primary)] px-3 py-1.5 text-xs font-medium text-[var(--primary-foreground)] transition-opacity hover:opacity-90"
            >
              Next: consent
            </button>
          )}

          {step === 3 && (
            <button
              type="button"
              onClick={handleStartRun}
              disabled={!consentConfirmed || hasPendingInterview || starting}
              title={
                hasPendingInterview
                  ? "Answer or skip every queued phase's pre-flight interview first"
                  : !consentConfirmed
                    ? "Confirm consent to start the run"
                    : undefined
              }
              className="flex items-center gap-1.5 rounded-md bg-[var(--primary)] px-3 py-1.5 text-xs font-medium text-[var(--primary-foreground)] transition-opacity hover:opacity-90 disabled:cursor-not-allowed disabled:opacity-40"
            >
              {starting && <Loader2 size={12} className="animate-spin" />}
              Start overnight run
            </button>
          )}
        </div>
      </DialogContent>
    </Dialog>
  );
}

// ── Step 1: phase picker + stall policy + budgets ────────────────────────────

function StepPhases({
  entries,
  selected,
  idToTitle,
  roster,
  agentByPhase,
  onSetAgent,
  stallPolicy,
  setStallPolicy,
  draftPrAuthorized,
  setDraftPrAuthorized,
  phaseTokenCap,
  setPhaseTokenCap,
  phaseMinutes,
  setPhaseMinutes,
  runHours,
  setRunHours,
  onToggle,
}: {
  entries: PickerEntry[];
  selected: string[];
  idToTitle: Record<string, string>;
  roster: NamedAgentConfig[];
  agentByPhase: Record<string, string>;
  onSetAgent: (id: string, agentId: string) => void;
  stallPolicy: StallPolicy;
  setStallPolicy: (v: StallPolicy) => void;
  draftPrAuthorized: boolean;
  setDraftPrAuthorized: (v: boolean) => void;
  phaseTokenCap: string;
  setPhaseTokenCap: (v: string) => void;
  phaseMinutes: string;
  setPhaseMinutes: (v: string) => void;
  runHours: string;
  setRunHours: (v: string) => void;
  onToggle: (id: string) => void;
}) {
  let lastGroup = "";
  return (
    <div className="space-y-4">
      <div>
        <div className="mb-2 text-[10px] font-semibold uppercase tracking-wider text-muted-foreground">
          Phases — check in run order
        </div>
        <ul className="space-y-0.5">
          {entries.map(({ loop, group }) => {
            const index = selected.indexOf(loop.id!);
            const showGroup = group !== lastGroup;
            lastGroup = group;
            return (
              <li key={loop.id}>
                {showGroup && (
                  <div className="mt-3 px-1 text-[10px] font-medium uppercase tracking-wider text-muted-foreground first:mt-0">
                    {group}
                  </div>
                )}
                <label className="flex cursor-pointer items-start gap-2 rounded px-1 py-1 text-xs hover:bg-accent">
                  <input
                    type="checkbox"
                    checked={index !== -1}
                    onChange={() => onToggle(loop.id!)}
                    className="mt-0.5 size-3.5"
                  />
                  <span className="min-w-0 flex-1">
                    <span className="block text-foreground">{loop.title}</span>
                    {index !== -1 && (
                      <span className="mt-0.5 block text-[10px] text-muted-foreground">
                        <span className="mr-1 font-mono">{index + 1}.</span>
                        {dependencyLabel(index, selected, idToTitle)}
                      </span>
                    )}
                  </span>
                </label>
                {/* Per-phase staffing (prd-role-foundations Phase 4): which
                    roster agent runs this phase. Kept outside the label so
                    picker clicks never toggle the checkbox. */}
                {index !== -1 && roster.length > 0 && (
                  <div className="mb-1 ml-6 flex items-center gap-1.5 text-[10px] text-muted-foreground">
                    <span className="shrink-0">runs with</span>
                    <Select
                      value={agentByPhase[loop.id!] || "default"}
                      onValueChange={(v) => onSetAgent(loop.id!, v === "default" ? "" : v)}
                    >
                      <SelectTrigger className="h-5 w-40 gap-1 rounded px-1.5 text-[10px]">
                        <SelectValue />
                      </SelectTrigger>
                      <SelectContent>
                        <SelectItem value="default">Default agent</SelectItem>
                        {roster.map((agent) => (
                          <SelectItem key={agent.id} value={agent.id}>
                            {agent.name}
                          </SelectItem>
                        ))}
                      </SelectContent>
                    </Select>
                  </div>
                )}
              </li>
            );
          })}
        </ul>
      </div>

      <div className="space-y-2.5 rounded-lg border border-border bg-card p-3">
        <div className="text-[10px] font-semibold uppercase tracking-wider text-muted-foreground">
          Run settings
        </div>
        <div className="flex flex-wrap items-center gap-x-4 gap-y-2">
          <Select value={stallPolicy} onValueChange={(v) => setStallPolicy(v as StallPolicy)}>
            <SelectTrigger className="h-7 w-52 text-xs">
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
        </div>
        <div className="flex flex-wrap items-center gap-3 text-xs text-muted-foreground">
          <label className="flex items-center gap-1">
            Tokens/phase
            <input
              type="number"
              min="1"
              value={phaseTokenCap}
              onChange={(e) => setPhaseTokenCap(e.target.value)}
              className="h-7 w-24 rounded border border-border bg-background px-1.5 text-xs text-foreground"
            />
          </label>
          <label className="flex items-center gap-1">
            Minutes/phase
            <input
              type="number"
              min="1"
              value={phaseMinutes}
              onChange={(e) => setPhaseMinutes(e.target.value)}
              className="h-7 w-14 rounded border border-border bg-background px-1.5 text-xs text-foreground"
            />
          </label>
          <label className="flex items-center gap-1">
            Hours/run
            <input
              type="number"
              min="1"
              value={runHours}
              onChange={(e) => setRunHours(e.target.value)}
              className="h-7 w-12 rounded border border-border bg-background px-1.5 text-xs text-foreground"
            />
          </label>
        </div>
        <p className="text-[11px] leading-relaxed text-muted-foreground">
          Unattended mode automatically allows safe project-scoped actions and plan execution.
          Destructive actions remain denied by the safety floor.
        </p>
      </div>
    </div>
  );
}

// ── Step 2: live pre-flight interviews ───────────────────────────────────────

const INTERVIEW_STATUS_LABEL: Record<string, string> = {
  pending: "interview pending",
  answered: "interview answered",
  skipped: "interview skipped",
};

function StepInterviews({
  plan,
  idToTitle,
  interviewingId,
  batchInterviewing,
  skippingId,
  pendingQuestion,
  onInterview,
  onInterviewAll,
  onSkip,
  onAnswerPending,
}: {
  plan: RunPlan;
  idToTitle: Record<string, string>;
  interviewingId: string | null;
  batchInterviewing: boolean;
  skippingId: string | null;
  pendingQuestion: PendingQuestionEntry | null;
  onInterview: (executionId: string) => void;
  onInterviewAll: () => void;
  onSkip: (executionId: string) => void;
  onAnswerPending: (answers: AskUserQuestionAnswers) => void;
}) {
  const pendingCount = plan.phases.filter(
    (phase) => phase.status === "queued" && phase.interview_status === "pending",
  ).length;
  return (
    <div>
      <div className="mb-2 flex items-center justify-between gap-2">
        <div className="text-[10px] font-semibold uppercase tracking-wider text-muted-foreground">
          Pre-flight interviews
        </div>
        {pendingCount > 1 && (
          <button
            type="button"
            onClick={onInterviewAll}
            disabled={batchInterviewing || interviewingId !== null || skippingId !== null}
            className="flex items-center gap-1.5 rounded-md bg-[var(--primary)] px-2 py-1 text-[10px] font-medium text-[var(--primary-foreground)] transition-opacity hover:opacity-90 disabled:cursor-not-allowed disabled:opacity-50"
          >
            {batchInterviewing && <Loader2 size={10} className="animate-spin" />}
            Answer all ({pendingCount})
          </button>
        )}
      </div>
      <p className="mb-3 text-[11px] leading-relaxed text-muted-foreground">
        Answer all runs one shared interview with the context for every selected phase, then pins
        its answers to the relevant loops. You can still run or skip individual interviews.
      </p>
      {batchInterviewing && (
        <div className="mb-3 rounded bg-accent/40 px-2.5 py-2">
          <div className="flex items-center gap-1.5 text-[11px] text-muted-foreground">
            <Loader2 size={11} className="animate-spin" />
            Combined interview running — all questions will appear in one card.
          </div>
          {pendingQuestion && (
            <div className="mt-2">
              <AskUserQuestionCard questions={pendingQuestion.questions} onSubmit={onAnswerPending} />
            </div>
          )}
        </div>
      )}
      <ul className="space-y-1">
        {plan.phases.map((phase) => {
          const title = idToTitle[phase.execution_id] ?? phase.execution_id;
          const needsInterview = phase.status === "queued" && phase.interview_status === "pending";
          const interviewing = interviewingId === phase.execution_id;
          const skipping = skippingId === phase.execution_id;
          return (
            <li key={phase.execution_id} className="rounded px-1.5 py-1 text-xs">
              <div className="flex items-center gap-2">
                <span className="flex-1 truncate text-foreground" title={phase.execution_id}>
                  {title}
                </span>
                {needsInterview ? (
                  <span className="flex shrink-0 items-center gap-1">
                    <button
                      onClick={() => onInterview(phase.execution_id)}
                      disabled={interviewing || skipping || interviewingId !== null || batchInterviewing}
                      className="rounded px-1.5 py-0.5 text-[10px] font-medium text-muted-foreground transition-colors hover:bg-accent hover:text-foreground disabled:opacity-50"
                    >
                      {interviewing ? <Loader2 size={10} className="animate-spin" /> : "Run interview"}
                    </button>
                    <button
                      onClick={() => onSkip(phase.execution_id)}
                      disabled={interviewing || skipping || interviewingId !== null || batchInterviewing}
                      className="rounded px-1.5 py-0.5 text-[10px] font-medium text-muted-foreground transition-colors hover:bg-accent hover:text-foreground disabled:opacity-50"
                    >
                      {skipping ? <Loader2 size={10} className="animate-spin" /> : "Skip"}
                    </button>
                  </span>
                ) : (
                  <span className="shrink-0 text-[10px] text-muted-foreground">
                    {INTERVIEW_STATUS_LABEL[phase.interview_status] ?? phase.interview_status}
                  </span>
                )}
              </div>
              {/* Running turn: live status + the parked question card, inline. */}
              {interviewing && (
                <div className="mt-1.5 rounded bg-accent/40 px-2.5 py-2">
                  <div className="flex items-center gap-1.5 text-[11px] text-muted-foreground">
                    <Loader2 size={11} className="animate-spin" />
                    Interview turn running — a parked question will appear here.
                  </div>
                  {pendingQuestion && (
                    <div className="mt-2">
                      <AskUserQuestionCard
                        questions={pendingQuestion.questions}
                        onSubmit={onAnswerPending}
                      />
                    </div>
                  )}
                </div>
              )}
              {/* Pinned answers from a resolved interview. */}
              {!interviewing &&
                phase.interview.map((pinned) => (
                  <div
                    key={pinned.question}
                    className="mt-1 rounded bg-[var(--success)]/5 px-2 py-1 text-[11px] leading-relaxed"
                  >
                    <span className="text-muted-foreground">{pinned.question}</span>{" "}
                    <span className="font-medium text-foreground">{pinned.answer}</span>
                  </div>
                ))}
            </li>
          );
        })}
      </ul>
    </div>
  );
}

// ── Step 3: consent summary + required checkbox ──────────────────────────────

function StepConsent({
  plan,
  idToTitle,
  consentConfirmed,
  setConsentConfirmed,
  hasPendingInterview,
}: {
  plan: RunPlan;
  idToTitle: Record<string, string>;
  consentConfirmed: boolean;
  setConsentConfirmed: (v: boolean) => void;
  hasPendingInterview: boolean;
}) {
  return (
    <div className="space-y-4">
      <div>
        <div className="mb-2 text-[10px] font-semibold uppercase tracking-wider text-muted-foreground">
          Tonight&apos;s run
        </div>
        <ul className="space-y-0.5 text-xs">
          {plan.phases.map((phase, i) => (
            <li key={phase.execution_id} className="flex items-baseline gap-2">
              <span className="font-mono text-[10px] text-muted-foreground">{i + 1}.</span>
              <span className="min-w-0 flex-1 truncate text-foreground" title={phase.execution_id}>
                {idToTitle[phase.execution_id] ?? phase.execution_id}
              </span>
              <span className="shrink-0 text-[10px] text-muted-foreground">
                {phase.assigned_agent_name ?? "default agent"}
              </span>
              <span className="shrink-0 text-[10px] text-muted-foreground">
                {INTERVIEW_STATUS_LABEL[phase.interview_status] ?? phase.interview_status}
              </span>
            </li>
          ))}
        </ul>
      </div>

      <div className="grid grid-cols-2 gap-x-4 gap-y-1.5 rounded-lg border border-border bg-card p-3 text-xs">
        <SummaryRow label="Stall policy">
          {plan.stall_policy === "halt" ? "Halt on stall" : "Continue independent phases"}
        </SummaryRow>
        <SummaryRow label="Draft PRs">
          {plan.consent.draft_pr_authorized ? "Authorized" : "Not authorized"}
        </SummaryRow>
        <SummaryRow label="Tokens / phase">
          {(plan.budgets.per_phase_token_cap ?? 0).toLocaleString()}
        </SummaryRow>
        <SummaryRow label="Wall clock / phase">
          {Math.floor((plan.budgets.per_phase_wall_clock_secs ?? 0) / 60)} min
        </SummaryRow>
        <SummaryRow label="Total run">
          {Math.floor((plan.budgets.total_run_wall_clock_secs ?? 0) / 3600)} h
        </SummaryRow>
        <SummaryRow label="Phases">{plan.phases.length}</SummaryRow>
      </div>

      {hasPendingInterview && (
        <p className="rounded bg-amber-500/5 px-2.5 py-2 text-[11px] leading-relaxed text-[var(--warning)]">
          Some phases still have a pending interview — go back and run or skip each one before
          starting the run.
        </p>
      )}

      <label className="flex cursor-pointer items-start gap-2 text-xs leading-relaxed">
        <input
          type="checkbox"
          checked={consentConfirmed}
          onChange={(e) => setConsentConfirmed(e.target.checked)}
          className="mt-0.5 size-3.5"
        />
        <span>
          I consent to this run executing unattended overnight: safe project-scoped actions are
          auto-allowed, {plan.consent.draft_pr_authorized ? "draft PRs open automatically, " : ""}
          and budget kills stop phases without asking me first.
        </span>
      </label>
    </div>
  );
}

function SummaryRow({ label, children }: { label: string; children: React.ReactNode }) {
  return (
    <div className="flex items-baseline justify-between gap-2">
      <span className="text-muted-foreground">{label}</span>
      <span className="font-medium text-foreground">{children}</span>
    </div>
  );
}
