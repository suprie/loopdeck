import { useCallback, useEffect, useState } from "react";
import {
  BadgeCheck,
  CircleAlert,
  GitBranch,
  GitPullRequest,
  ListChecks,
  PartyPopper,
  RefreshCw,
  RotateCcw,
  ShieldQuestion,
} from "lucide-react";
import type {
  DeliveryReportResponse,
  ExternalWorktree,
  GateBlock,
  MismatchKind,
  RetryOutcome,
  RetryState,
  RubricResult,
} from "../../types";
import * as api from "../../lib/tauri";
import { LoadingSpinner } from "../shared/LoadingSpinner";
import { toast } from "sonner";

/**
 * Delivery verification & discrepancy report —
 * `prd-verified-delivery-reconciliation` Phase 1. Shows, before any delivery
 * mutation, what the persisted links (branch, PRD, rubric, PR) claim for each
 * loop, where those claims disagree with live Git / checklist state, and
 * which delivery gates would block right now. The "Run verification" action
 * triggers a fresh PRD-rubric run (agent verifier turn) and retains the
 * result. The bottom section lists external legacy worktrees — detected and
 * classified only, never moved or deleted here (Phase 2).
 */
export function DeliveryReportTab({ projectPath }: { projectPath: string }) {
  const [report, setReport] = useState<DeliveryReportResponse | null>(null);
  const [external, setExternal] = useState<ExternalWorktree[] | null>(null);
  const [loading, setLoading] = useState(true);
  const [runningRubric, setRunningRubric] = useState(false);
  const [retrying, setRetrying] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const refresh = useCallback(async () => {
    try {
      const [delivery, worktrees] = await Promise.all([
        api.getDeliveryReport(projectPath),
        api.detectExternalWorktrees(projectPath),
      ]);
      setReport(delivery);
      setExternal(worktrees);
      setError(null);
    } catch (err) {
      setError(String(err));
    } finally {
      setLoading(false);
    }
  }, [projectPath]);

  useEffect(() => {
    setLoading(true);
    void refresh();
  }, [refresh]);

  const handleRunRubric = async () => {
    setRunningRubric(true);
    try {
      const rubric = await api.runDeliveryRubric(projectPath);
      toast.success(
        `Rubric ${rubric.verdict.toUpperCase()} — ${rubric.criteria.filter((c) => c.status === "pass").length}/${rubric.criteria.length} criteria pass`,
      );
      await refresh();
    } catch (err) {
      toast.error(`Rubric run failed: ${String(err)}`);
    } finally {
      setRunningRubric(false);
    }
  };

  const handleRetry = async () => {
    setRetrying(true);
    try {
      const outcome = await api.retryDelivery(projectPath);
      toast.success(retryToast(outcome));
      await refresh();
    } catch (err) {
      toast.error(`Delivery retry failed: ${String(err)}`);
    } finally {
      setRetrying(false);
    }
  };

  if (loading) return <LoadingSpinner label="Loading delivery report…" />;
  if (error) {
    return (
      <p className="text-sm text-[color:var(--destructive)]">Delivery report failed: {error}</p>
    );
  }

  const activeLoop = report?.loops.find((loop) => loop.in_progress);
  const historyLoops = report?.loops.filter((loop) => !loop.in_progress) ?? [];

  return (
    <section className="mb-8 space-y-4">
      <header className="flex items-center justify-between gap-3">
        <div>
          <h3 className="flex items-center gap-2 text-sm font-semibold">
            <ListChecks size={14} /> Delivery report
          </h3>
          <p className="text-xs text-[color:var(--muted-foreground)]">
            Reconciled links before any delivery mutation — branch, PRD rubric, PR.
          </p>
        </div>
        <div className="flex gap-2">
          <button
            className="flex items-center gap-1.5 rounded-md border border-border px-2.5 py-1 text-[11px] font-medium text-foreground transition-colors hover:bg-accent"
            onClick={() => void refresh()}
          >
            <RefreshCw size={12} /> Refresh
          </button>
          <button
            className="flex items-center gap-1.5 rounded-md border border-border px-2.5 py-1 text-[11px] font-medium text-foreground transition-colors hover:bg-accent disabled:cursor-not-allowed disabled:opacity-50"
            onClick={() => void handleRunRubric()}
            disabled={runningRubric || !activeLoop}
            title={
              activeLoop
                ? "Run the PRD rubric fresh for the active loop"
                : "No loop is in progress"
            }
          >
            {runningRubric ? (
              <LoadingSpinner label="" />
            ) : (
              <ShieldQuestion size={12} />
            )}
            {runningRubric ? "Verifying…" : "Run verification"}
          </button>
        </div>
      </header>

      {activeLoop && <LoopCard loop={activeLoop} expanded />}
      {report?.retry && <RetryCard retry={report.retry} retrying={retrying} onRetry={() => void handleRetry()} />}
      {report?.handoff && <HandoffBanner handoff={report.handoff} />}
      {historyLoops.length > 0 && (
        <details className="rounded-lg border border-[color:var(--border)] p-3">
          <summary className="cursor-pointer text-xs font-medium">
            Recent deliveries ({historyLoops.length})
          </summary>
          <div className="mt-3 space-y-3">
            {historyLoops.map((loop) => (
              <LoopCard key={`${loop.loop_id}-${loop.title}`} loop={loop} />
            ))}
          </div>
        </details>
      )}

      <ExternalWorktreesSection worktrees={external} />
    </section>
  );
}

function LoopCard({
  loop,
  expanded = false,
}: {
  loop: DeliveryReportResponse["loops"][number];
  expanded?: boolean;
}) {
  const verified = loop.mismatches.length === 0 && loop.links != null;
  const rubric = loop.links?.rubric;
  return (
    <div className="rounded-lg border border-[color:var(--border)] p-3 text-xs">
      <div className="flex items-start justify-between gap-2">
        <div>
          <p className="font-medium">{loop.title}</p>
          <p className="text-[color:var(--muted-foreground)]">{loop.loop_id}</p>
        </div>
        <span
          className="flex items-center gap-1"
          style={{ color: verified ? "var(--success)" : "var(--warning)" }}
        >
          {verified ? <BadgeCheck size={14} /> : <CircleAlert size={14} />}
          {verified ? "verified" : loop.links ? `${loop.mismatches.length} mismatch(es)` : "no links"}
        </span>
      </div>

      <div className="mt-2 flex flex-wrap gap-x-4 gap-y-1 text-[color:var(--muted-foreground)]">
        {loop.links?.branch && (
          <span className="flex items-center gap-1">
            <GitBranch size={11} /> {loop.links.branch}
          </span>
        )}
        {loop.links?.pr_url && (
          <a
            className="flex items-center gap-1 underline"
            href={loop.links.pr_url}
            target="_blank"
            rel="noreferrer"
          >
            <GitPullRequest size={11} /> {loop.links.pr_url}
          </a>
        )}
        {loop.links?.commit && <span className="font-mono">{loop.links.commit.slice(0, 10)}</span>}
      </div>

      {loop.mismatches.length > 0 && (
        <ul className="mt-2 space-y-1">
          {loop.mismatches.map((kind) => (
            <li key={kind} className="flex items-center gap-1.5 text-[color:var(--warning)]">
              <CircleAlert size={11} /> {MISMATCH_LABELS[kind]}
            </li>
          ))}
        </ul>
      )}

      {loop.in_progress && loop.gate_blocks.length > 0 && (
        <div className="mt-2 rounded border border-[color:var(--border)] p-2">
          <p className="mb-1 font-medium">Delivery gates blocking right now</p>
          <ul className="space-y-1 text-[color:var(--muted-foreground)]">
            {loop.gate_blocks.map((block) => (
              <li key={block}>· {GATE_LABELS[block]}</li>
            ))}
          </ul>
        </div>
      )}

      {rubric && (expanded || rubric.criteria.length > 0) && <RubricTable rubric={rubric} />}
    </div>
  );
}

function RubricTable({ rubric }: { rubric: RubricResult }) {
  const color =
    rubric.verdict === "pass"
      ? "var(--success)"
      : rubric.verdict === "warn"
        ? "var(--warning)"
        : "var(--destructive)";
  return (
    <div className="mt-2 overflow-x-auto">
      <p className="mb-1 font-medium">
        Rubric — <span style={{ color }}>{rubric.verdict.toUpperCase()}</span>{" "}
        <span className="text-[color:var(--muted-foreground)]">
          ({new Date(rubric.checked_at).toLocaleString()})
        </span>
      </p>
      <table className="w-full text-left">
        <thead className="text-[color:var(--muted-foreground)]">
          <tr>
            <th className="pr-2 font-normal">Criterion</th>
            <th className="pr-2 font-normal">Status</th>
          </tr>
        </thead>
        <tbody>
          {rubric.criteria.map((c, i) => (
            <tr key={i} className="border-t border-[color:var(--border)]">
              <td className="pr-2 py-0.5">{c.criterion}</td>
              <td
                className="pr-2 py-0.5"
                style={{
                  color:
                    c.status === "pass"
                      ? "var(--success)"
                      : c.status === "partial"
                        ? "var(--warning)"
                        : "var(--destructive)",
                }}
              >
                {c.status.toUpperCase()}
              </td>
            </tr>
          ))}
        </tbody>
      </table>
      {rubric.criteria.length === 0 && (
        <p className="text-[color:var(--muted-foreground)]">No criterion rows retained.</p>
      )}
    </div>
  );
}

function ExternalWorktreesSection({ worktrees }: { worktrees: ExternalWorktree[] | null }) {
  if (!worktrees) return null;
  return (
    <div className="rounded-lg border border-[color:var(--border)] p-3 text-xs">
      <h4 className="font-semibold">External worktrees ({worktrees.length})</h4>
      <p className="text-[color:var(--muted-foreground)]">
        Outside the managed `.loopdeck/runs/` directory. Detected and classified —
        never moved or deleted automatically.
      </p>
      {worktrees.length === 0 ? (
        <p className="mt-2 text-[color:var(--muted-foreground)]">
          None found — every linked worktree is managed.
        </p>
      ) : (
        <ul className="mt-2 space-y-1">
          {worktrees.map((wt) => (
            <li key={wt.path} className="flex flex-wrap items-center gap-x-2">
              <span className="rounded bg-[color-mix(in_oklab,var(--warning)_12%,transparent)] px-1.5 py-0.5 text-[color:var(--warning)]">
                {wt.label}
              </span>
              <span className="font-mono">{wt.path}</span>
              {wt.branch && (
                <span className="text-[color:var(--muted-foreground)]">({wt.branch})</span>
              )}
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}

function HandoffBanner({ handoff }: { handoff: DeliveryReportResponse["handoff"] }) {
  if (!handoff) return null;
  return (
    <div className="flex items-start gap-2 rounded-lg border border-[color:var(--border)] p-3 text-xs">
      <PartyPopper size={14} className="mt-0.5 shrink-0 text-[color:var(--success)]" />
      <div>
        <p className="font-medium">
          Clean handoff — delivered {new Date(handoff.delivered_at).toLocaleString()}
        </p>
        <p className="mt-1 text-[color:var(--muted-foreground)]">
          Branch <GitBranch className="inline" size={11} /> {handoff.delivered_branch} and its
          worktree are retained for review; the PR stays a draft until you merge it. The next run
          starts fresh from <span className="font-medium">{handoff.next_base}</span> — a new
          worktree is created only when the next loop starts.
        </p>
        <a
          className="mt-1 flex items-center gap-1 underline"
          href={handoff.pr_url}
          target="_blank"
          rel="noreferrer"
        >
          <GitPullRequest size={11} /> {handoff.pr_url}
        </a>
      </div>
    </div>
  );
}

function RetryCard({
  retry,
  retrying,
  onRetry,
}: {
  retry: RetryState;
  retrying: boolean;
  onRetry: () => void;
}) {
  const { record, next_action: nextAction } = retry;
  return (
    <div className="rounded-lg border border-[color:var(--warning)] p-3 text-xs">
      <div className="flex items-start justify-between gap-2">
        <div>
          <p className="font-medium">Recoverable delivery — {record.stage.replace("_", " ")}</p>
          <p className="text-[color:var(--muted-foreground)]">
            {record.pr_title || record.execution_ids.join(", ")} ·{" "}
            <GitBranch className="inline" size={11} /> {record.branch}
          </p>
        </div>
        <button
          className="flex items-center gap-1.5 rounded-md border border-border px-2.5 py-1 text-[11px] font-medium text-foreground transition-colors hover:bg-accent disabled:cursor-not-allowed disabled:opacity-50"
          onClick={onRetry}
          disabled={retrying}
        >
          {retrying ? <LoadingSpinner label="" /> : <RotateCcw size={12} />}
          {retrying ? "Retrying…" : "Retry delivery"}
        </button>
      </div>
      <p className="mt-2 text-[color:var(--warning)]">
        <CircleAlert size={11} className="inline" /> {record.reason}
      </p>
      <p className="mt-1 text-[color:var(--muted-foreground)]">Next safe action: {nextAction}</p>
    </div>
  );
}

function retryToast(outcome: RetryOutcome): string {
  switch (outcome.kind) {
    case "requeued":
      return `Requeued ${outcome.execution_ids.length} phase(s) for a fresh run — nothing had been committed.`;
    case "delivery_completed":
      return `Delivery completed — draft PR ready (resumed from ${outcome.resumed_from.replace("_", " ")}).`;
    case "still_blocked":
      return `Retry blocked at "${outcome.stage.replace("_", " ")}": ${outcome.reason}`;
  }
}

const MISMATCH_LABELS: Record<MismatchKind, string> = {
  branch_missing: "Recorded branch is missing from the local repo",
  commit_diverged: "Recorded commit is not reachable from the recorded branch",
  wrong_branch: "Live branch differs from the branch recorded for this loop",
  pr_link_missing: "Delivery has no pull-request link",
  rubric_missing: "No PRD rubric result was retained",
  rubric_not_passing: "Retained rubric result is not passing",
  checklist_incomplete: "Loop is completed but its PRD checklist item is unchecked",
  checklist_premature: "PRD checklist item is checked but no PR link was recorded",
};

const GATE_LABELS: Record<GateBlock, string> = {
  loop_not_pending: "The loop's checklist item is already complete",
  branch_mismatch: "Current worktree branch is not the branch recorded for the loop",
  prd_link_missing: "The loop is not linked to a PRD checklist item",
  rubric_missing: "No PRD rubric result was produced for this delivery",
  rubric_not_passing: "PRD rubric did not pass every criterion",
};
