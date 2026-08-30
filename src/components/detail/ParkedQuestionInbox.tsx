import { useState } from "react";
import { AlertTriangle, Loader2, RotateCcw } from "lucide-react";
import { toast } from "sonner";
import * as api from "../../lib/tauri";
import { AskUserQuestionCard } from "./AskUserQuestionCard";
import { parkedInbox } from "../../lib/nightRun";
import { useStreamingState } from "../../store/streamingState";
import type { AppError, AskUserQuestionAnswers, RunPhase, RunPlan } from "../../types";

/**
 * The inline parked-question inbox — one card per currently-parked phase
 * (prd-night-run-surfaces Phase 1, item 2). Extracted from NightRunTab so the
 * morning-report drawer (Phase 3, item 1) reuses the exact same card + requeue
 * wiring rather than restyling it.
 *
 * Both requeue paths, identical everywhere this inbox renders:
 * - structured `__QUESTIONS__` payloads answer via the shared
 *   `AskUserQuestionCard` (submit = "Answer & requeue" → `answerParkedQuestion`)
 * - raw payloads get a plain "Answer & requeue" button → `requeueRunPhase` +
 *   `queueRun` (RunQueuePanel's Retry flow)
 *
 * Cards hide optimistically once answered/requeued (the `resolved` set) until
 * the caller's `useRunStatus` poll delivers the updated plan. Renders null
 * when nothing is parked.
 */
export function ParkedQuestionInbox({
  projectPath,
  plan,
  idToTitle,
  onResolved,
}: {
  projectPath: string;
  plan: RunPlan;
  /** Stable execution ID -> loop title, for card headers. */
  idToTitle: Record<string, string>;
  /** Fired after a successful answer/requeue, so the surface that owns the
   *  plan/report can refetch its own copy. */
  onResolved?: (executionId: string) => void;
}) {
  const [answeringId, setAnsweringId] = useState<string | null>(null);
  const [requeueingId, setRequeueingId] = useState<string | null>(null);
  // Phases answered/requeued from this inbox, hidden optimistically until the
  // drawer's 5s `useRunStatus` poll delivers the updated plan.
  const [resolved, setResolved] = useState<Set<string>>(new Set());

  const resolve = (executionId: string) => {
    setResolved((prev) => {
      const next = new Set(prev);
      next.add(executionId);
      return next;
    });
    onResolved?.(executionId);
  };

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
  if (cards.length === 0) return null;

  const title = (phase: RunPhase) => idToTitle[phase.execution_id] ?? phase.execution_id;

  return (
    <div className="rounded-xl border border-border bg-card p-4 shadow-[var(--shadow-sm)]">
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
  );
}
