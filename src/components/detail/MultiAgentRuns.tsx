import { useCallback, useEffect, useMemo, useState } from "react";
import { Channel } from "@tauri-apps/api/core";
import { Bot, Loader2, Play, RotateCcw, Square, Workflow } from "lucide-react";
import * as api from "../../lib/tauri";
import { useAppStore } from "../../store/appStore";
import type {
  AskUserQuestionAnswers,
  ClaudeEvent,
  MultiAgentEvent,
  MultiAgentRun,
  MultiAgentRunStatus,
  MultiAgentSubRun,
  NamedAgentConfig,
} from "../../types";
import { AskUserQuestionCard } from "./AskUserQuestionCard";
import {
  PermissionApprovalCard,
  PlanApprovalCard,
} from "./Chat";

interface MultiAgentRunsProps {
  projectPath: string;
}

const STATUS_CLASS: Record<MultiAgentRunStatus, string> = {
  queued: "bg-muted text-muted-foreground",
  running: "bg-primary/10 text-primary",
  waiting: "bg-amber-500/10 text-amber-600 dark:text-amber-400",
  done: "bg-success/10 text-success",
  failed: "bg-destructive/10 text-destructive",
  cancelled: "bg-muted text-muted-foreground",
};
const MAX_ASSIGNED_AGENTS = 8;

function statusLabel(status: MultiAgentRunStatus): string {
  return status === "done" ? "completed" : status;
}

function modelLabel(subRun: MultiAgentSubRun): string {
  return subRun.model || "CLI default";
}

export function aggregateMultiAgentRunStatus(
  subRuns: MultiAgentSubRun[],
): MultiAgentRunStatus {
  if (subRuns.some((subRun) => subRun.status === "running")) return "running";
  if (subRuns.some((subRun) => subRun.status === "queued")) return "queued";
  if (subRuns.some((subRun) => subRun.status === "waiting")) return "waiting";
  if (subRuns.some((subRun) => subRun.status === "failed")) return "failed";
  if (subRuns.some((subRun) => subRun.status === "cancelled")) return "cancelled";
  return "done";
}

export function patchMultiAgentRun(
  runs: MultiAgentRun[],
  event: MultiAgentEvent,
): MultiAgentRun[] {
  return runs.map((run) => {
    if (run.id !== event.run_id) return run;
    if (event.sub_run) {
      const subRuns = run.sub_runs.map((subRun) =>
        subRun.agent_id === event.agent_id ? event.sub_run! : subRun,
      );
      return {
        ...run,
        status: aggregateMultiAgentRunStatus(subRuns),
        sub_runs: subRuns,
      };
    }
    const resultEvent = event.event;
    if (!resultEvent || resultEvent.type !== "result") return run;
    const subRuns = run.sub_runs.map((subRun) =>
      subRun.agent_id !== event.agent_id
        ? subRun
        : {
            ...subRun,
            status: resultEvent.is_error ? "failed" as const : "done" as const,
            result: resultEvent.result,
            error: resultEvent.is_error ? resultEvent.result : null,
            completed_at: new Date().toISOString(),
          },
    );
    return {
      ...run,
      status: aggregateMultiAgentRunStatus(subRuns),
      sub_runs: subRuns,
    };
  });
}

/** Select profiles and monitor each isolated worktree sub-run independently. */
export function MultiAgentRuns({ projectPath }: MultiAgentRunsProps) {
  const setError = useAppStore((state) => state.setError);
  const [profiles, setProfiles] = useState<NamedAgentConfig[]>([]);
  const [selectedIds, setSelectedIds] = useState<string[]>([]);
  const [runs, setRuns] = useState<MultiAgentRun[]>([]);
  const [loaded, setLoaded] = useState(false);
  const [starting, setStarting] = useState(false);
  const [controlling, setControlling] = useState<string | null>(null);
  const [pendingEvents, setPendingEvents] = useState<Record<string, ClaudeEvent | undefined>>({});

  const eventKey = (runId: string, agentId: string) => `${runId}:${agentId}`;

  const rememberControlEvent = useCallback((event: MultiAgentEvent) => {
    const payload = event.event;
    if (!payload) return;
    const key = eventKey(event.run_id, event.agent_id);
    const pending =
      payload.type === "ask_user_question" ||
      (payload.type === "permission_request" && payload.decision === "pending") ||
      (payload.type === "plan_approval" && payload.decision === "pending");
    const resolved =
      payload.type === "result" ||
      (payload.type === "permission_request" && payload.decision !== "pending") ||
      (payload.type === "plan_approval" && payload.decision !== "pending");
    if (pending || resolved) {
      setPendingEvents((current) => ({
        ...current,
        [key]: pending ? payload : undefined,
      }));
    }
  }, []);

  const refreshRuns = useCallback(async () => {
    try {
      setRuns(await api.agentListMultiAgentRuns(projectPath));
    } catch (error) {
      setError(String(error));
    }
  }, [projectPath, setError]);

  useEffect(() => {
    let cancelled = false;
    void Promise.all([api.listAgentConfigs(), api.agentListMultiAgentRuns(projectPath)])
      .then(([nextProfiles, nextRuns]) => {
        if (cancelled) return;
        setProfiles(nextProfiles);
        setRuns(nextRuns);
        const defaults = nextProfiles.filter((profile) => profile.is_default).map((profile) => profile.id);
        setSelectedIds(defaults.length > 0 ? defaults : nextProfiles.slice(0, 1).map((profile) => profile.id));
      })
      .catch((error) => {
        if (!cancelled) setError(String(error));
      })
      .finally(() => {
        if (!cancelled) setLoaded(true);
      });
    return () => {
      cancelled = true;
    };
  }, [projectPath, setError]);

  // A channel subscription belongs to the component instance that created it.
  // When the user navigates away and back during a parked sub-run, recover the
  // control card from the backend's worktree-keyed slots.
  useEffect(() => {
    const active = runs.flatMap((run) =>
      run.sub_runs
        .filter((subRun) =>
          Boolean(subRun.worktree) &&
          (subRun.status === "running" || subRun.status === "waiting"),
        )
        .map((subRun) => ({ run, subRun })),
    );
    for (const { run, subRun } of active) {
      const worktree = subRun.worktree!;
      void Promise.all([
        api.agentPendingQuestion(worktree),
        api.agentPendingPermission(worktree),
        api.agentPendingPlan(worktree),
      ]).then(([question, permission, plan]) => {
        const event: ClaudeEvent | undefined = question
          ? {
              type: "ask_user_question",
              request_id: question.requestId,
              tool_name: "AskUserQuestion",
              questions: question.questions,
            }
          : permission
            ? {
                type: "permission_request",
                request_id: permission.requestId,
                tool_name: permission.toolName,
                input: permission.input,
                decision: "pending",
                reason: "",
              }
            : plan
              ? {
                  type: "plan_approval",
                  request_id: plan.requestId,
                  plan: plan.plan,
                  decision: "pending",
                  reason: "",
                }
              : undefined;
        if (event) {
          setPendingEvents((current) => ({
            ...current,
            [eventKey(run.id, subRun.agent_id)]: event,
          }));
        }
      }).catch(() => {
        // The sub-run may finish between the manifest read and slot queries.
      });
    }
  }, [runs]);

  const toggleProfile = (id: string) => {
    setSelectedIds((current) =>
      current.includes(id)
        ? current.filter((selected) => selected !== id)
        : current.length >= MAX_ASSIGNED_AGENTS
          ? current
          : [...current, id],
    );
  };

  const assignedProfiles = useMemo(
    () => profiles.filter((profile) => selectedIds.includes(profile.id)),
    [profiles, selectedIds],
  );

  const start = useCallback(async () => {
    if (starting || selectedIds.length === 0) return;
    setStarting(true);
    try {
      const channel = new Channel<MultiAgentEvent>();
      channel.onmessage = (event) => {
        rememberControlEvent(event);
        setRuns((current) => patchMultiAgentRun(current, event));
        // Lifecycle snapshots are emitted only after the backend has durably
        // updated the manifest. The streaming Result arrives slightly earlier,
        // so refreshing on it can read the previous running state and leave the
        // aggregate header stale.
        if (event.sub_run && !["queued", "running", "waiting"].includes(event.sub_run.status)) {
          void refreshRuns();
        }
      };
      const run = await api.agentStartMultiLoopStreaming(projectPath, selectedIds, channel);
      setRuns((current) => [run, ...current.filter((existing) => existing.id !== run.id)]);
    } catch (error) {
      setError(String(error));
    } finally {
      setStarting(false);
    }
  }, [projectPath, refreshRuns, rememberControlEvent, selectedIds, setError, starting]);

  const control = useCallback(async (
    run: MultiAgentRun,
    subRun: MultiAgentSubRun,
    action: "interrupt" | "retry",
  ) => {
    const key = `${run.id}:${subRun.agent_id}:${action}`;
    setControlling(key);
    try {
      const updated = await api.agentControlMultiAgentRun(projectPath, run.id, subRun.agent_id, action);
      setRuns((current) => current.map((item) => item.id === updated.id ? updated : item));
    } catch (error) {
      setError(String(error));
    } finally {
      setControlling(null);
    }
  }, [projectPath, setError]);

  return (
    <section className="mb-3 rounded-lg border border-border bg-card/60 p-3 shrink-0">
      <div className="flex flex-wrap items-start justify-between gap-3">
        <div>
          <div className="flex items-center gap-1.5 text-xs font-semibold text-foreground">
            <Workflow className="size-3.5 text-primary" /> Multi-agent loop
          </div>
          <p className="mt-0.5 text-[11px] text-muted-foreground">
            Each selected profile runs the same loop in an isolated branch and worktree.
          </p>
        </div>
        <button
          onClick={() => void start()}
          disabled={!loaded || starting || selectedIds.length === 0}
          className="inline-flex h-8 items-center gap-1.5 rounded-md bg-primary px-3 text-xs font-medium text-primary-foreground disabled:opacity-50"
        >
          {starting ? <Loader2 className="size-3.5 animate-spin" /> : <Play className="size-3.5 fill-current" />}
          Run {selectedIds.length || ""} agent{selectedIds.length === 1 ? "" : "s"}
        </button>
      </div>

      {!loaded ? (
        <div className="mt-3 flex items-center gap-2 text-xs text-muted-foreground"><Loader2 className="size-3 animate-spin" /> Loading profiles…</div>
      ) : profiles.length === 0 ? (
        <p className="mt-3 rounded-md bg-muted/50 px-3 py-2 text-xs text-muted-foreground">
          Add a named agent profile in Settings before starting a multi-agent loop.
        </p>
      ) : (
        <div className="mt-3 flex flex-wrap gap-2">
          {profiles.map((profile) => {
            const selected = selectedIds.includes(profile.id);
            return (
              <label
                key={profile.id}
                className={`inline-flex cursor-pointer items-center gap-2 rounded-md border px-2.5 py-1.5 text-xs transition ${
                  selected ? "border-primary/50 bg-primary/10 text-foreground" : "border-border text-muted-foreground hover:bg-muted"
                }`}
              >
                <input
                  type="checkbox"
                  checked={selected}
                  disabled={!selected && selectedIds.length >= MAX_ASSIGNED_AGENTS}
                  onChange={() => toggleProfile(profile.id)}
                  className="accent-primary disabled:cursor-not-allowed"
                />
                <Bot className="size-3.5" />
                <span>{profile.name}</span>
              </label>
            );
          })}
        </div>
      )}

      {assignedProfiles.length > 0 ? (
        <p className="mt-2 text-[10px] text-muted-foreground">
          Assigned: {assignedProfiles.map((profile) => profile.name).join(", ")}
          {selectedIds.length >= MAX_ASSIGNED_AGENTS ? ` (maximum ${MAX_ASSIGNED_AGENTS})` : ""}
        </p>
      ) : null}

      {runs.slice(0, 3).map((run) => (
        <div key={run.id} className="mt-3 border-t border-border pt-3">
          <div className="mb-2 flex items-center gap-2 text-[11px] text-muted-foreground">
            <span className="font-mono">{run.id}</span>
            <Status status={run.status} />
          </div>
          <div className="grid gap-2 lg:grid-cols-2">
            {run.sub_runs.map((subRun) => (
              <SubRunCard
                key={subRun.id}
                run={run}
                subRun={subRun}
                controlling={controlling}
                pendingEvent={pendingEvents[eventKey(run.id, subRun.agent_id)]}
                onControlResolved={() =>
                  setPendingEvents((current) => ({
                    ...current,
                    [eventKey(run.id, subRun.agent_id)]: undefined,
                  }))
                }
                onControl={control}
              />
            ))}
          </div>
        </div>
      ))}
    </section>
  );
}

function Status({ status }: { status: MultiAgentRunStatus }) {
  return <span className={`rounded-full px-1.5 py-0.5 text-[10px] font-medium ${STATUS_CLASS[status]}`}>{statusLabel(status)}</span>;
}

function SubRunCard({
  run,
  subRun,
  controlling,
  pendingEvent,
  onControlResolved,
  onControl,
}: {
  run: MultiAgentRun;
  subRun: MultiAgentSubRun;
  controlling: string | null;
  pendingEvent?: ClaudeEvent;
  onControlResolved: () => void;
  onControl: (run: MultiAgentRun, subRun: MultiAgentSubRun, action: "interrupt" | "retry") => Promise<void>;
}) {
  const interruptKey = `${run.id}:${subRun.agent_id}:interrupt`;
  const retryKey = `${run.id}:${subRun.agent_id}:retry`;
  return (
    <article className="rounded-md border border-border bg-background/60 p-3">
      <div className="flex items-start justify-between gap-2">
        <div className="min-w-0">
          <p className="truncate text-xs font-semibold">{subRun.agent_name}</p>
          <p className="truncate text-[10px] text-muted-foreground">{subRun.harness === "codex" ? "Codex" : "Claude Code"} · {modelLabel(subRun)}</p>
        </div>
        <Status status={subRun.status} />
      </div>
      <dl className="mt-2 space-y-1 text-[10px] leading-relaxed text-muted-foreground">
        <div className="flex gap-1"><dt className="shrink-0">Branch:</dt><dd className="truncate font-mono" title={subRun.branch ?? ""}>{subRun.branch || "pending"}</dd></div>
        <div className="flex gap-1"><dt className="shrink-0">Worktree:</dt><dd className="truncate font-mono" title={subRun.worktree ?? ""}>{subRun.worktree || "pending"}</dd></div>
      </dl>
      {subRun.result ? <p className="mt-2 line-clamp-3 text-[11px] leading-relaxed text-foreground/80">{subRun.result}</p> : null}
      {subRun.error ? <p className="mt-2 line-clamp-3 text-[11px] leading-relaxed text-destructive">{subRun.error}</p> : null}
      {pendingEvent?.type === "ask_user_question" && subRun.worktree ? (
        <AskUserQuestionCard
          questions={pendingEvent.questions}
          onSubmit={(answers: AskUserQuestionAnswers) => {
            void api
              .agentAnswerQuestion(subRun.worktree!, pendingEvent.request_id, answers)
              .then(onControlResolved);
          }}
        />
      ) : null}
      {pendingEvent?.type === "permission_request" && subRun.worktree ? (
        <PermissionApprovalCard
          toolName={pendingEvent.tool_name}
          input={pendingEvent.input}
          onDecide={(decision) => {
            void api
              .agentAnswerPermission(subRun.worktree!, pendingEvent.request_id, decision)
              .then(onControlResolved);
          }}
        />
      ) : null}
      {pendingEvent?.type === "plan_approval" && subRun.worktree ? (
        <PlanApprovalCard
          plan={pendingEvent.plan}
          onDecide={(decision) => {
            void api
              .agentAnswerPlan(subRun.worktree!, pendingEvent.request_id, decision)
              .then(onControlResolved);
          }}
        />
      ) : null}
      <div className="mt-2 flex gap-1.5">
        {subRun.status === "running" || subRun.status === "waiting" ? (
          <button onClick={() => void onControl(run, subRun, "interrupt")} disabled={controlling === interruptKey} className="inline-flex h-7 items-center gap-1 rounded-md border border-destructive/40 px-2 text-[10px] text-destructive disabled:opacity-50">
            {controlling === interruptKey ? <Loader2 className="size-3 animate-spin" /> : <Square className="size-2.5 fill-current" />} Stop
          </button>
        ) : null}
        {subRun.status === "failed" || subRun.status === "cancelled" ? (
          <button onClick={() => void onControl(run, subRun, "retry")} disabled={controlling === retryKey} className="inline-flex h-7 items-center gap-1 rounded-md border border-border px-2 text-[10px] text-muted-foreground hover:bg-muted disabled:opacity-50">
            {controlling === retryKey ? <Loader2 className="size-3 animate-spin" /> : <RotateCcw className="size-3" />} Retry
          </button>
        ) : null}
      </div>
    </article>
  );
}
