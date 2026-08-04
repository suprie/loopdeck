import { useState, useEffect, useCallback, useMemo } from "react";
import {
  Layers,
  Zap,
  CheckCircle2,
  Circle,
  CheckSquare,
  Square,
  Loader2,
  Pencil,
  ChevronRight,
  AlertTriangle,
  FileDown,
  GitCommitHorizontal,
  Fingerprint,
} from "lucide-react";
import { toast } from "sonner";
import { useNavigate } from "@tanstack/react-router";
import type {
  Epic,
  PrdLoop,
  LoopStatus,
  AppError,
  ProgressSnapshot,
  ProgressCount,
  ExecutionStatus,
  DeliveryStatus,
} from "../../types";
import * as api from "../../lib/tauri";
import { LoadingSpinner } from "../shared/LoadingSpinner";
import { Tabs, TabsList, TabsTrigger, TabsContent } from "../ui/tabs";
import { Progress } from "../ui/progress";
import { PrdContent } from "./PrdContent";
import { RunQueuePanel } from "./RunQueuePanel";

// ── Derived progress helpers ─────────────────────────────────────────────────

const EXECUTION_STATUS_LABEL: Record<ExecutionStatus, string> = {
  planned: "planned",
  queued: "queued",
  in_progress: "in progress",
  completed: "completed",
  abandoned: "abandoned",
  unmatched: "unmatched",
};

const EXECUTION_STATUS_COLOR: Record<ExecutionStatus, string> = {
  planned: "var(--muted-foreground)",
  queued: "var(--warning)",
  in_progress: "var(--primary)",
  completed: "var(--success)",
  abandoned: "var(--muted-foreground)",
  unmatched: "var(--destructive)",
};

const DELIVERY_STATUS_LABEL: Record<DeliveryStatus, string> = {
  planned: "planned",
  implemented: "implemented",
  committed: "committed",
  in_review: "in review",
  shipped: "shipped",
};

/** Small pill showing a loop's derived execution state, plus a delivery chip
 * once the loop is completed (implemented/committed/in review/shipped) — the
 * PRD's precise progress-language distinction between "implemented" and
 * "committed" needs its own label, not just the execution pill. */
function DerivedStatusBadge({
  execution,
  delivery,
}: {
  execution: ExecutionStatus;
  delivery: DeliveryStatus;
}) {
  return (
    <span className="inline-flex shrink-0 items-center gap-1">
      <span
        className="rounded px-1.5 py-0.5 text-[10px] font-medium uppercase tracking-wider"
        style={{ color: EXECUTION_STATUS_COLOR[execution] }}
      >
        {EXECUTION_STATUS_LABEL[execution]}
      </span>
      {delivery !== "planned" && (
        <span
          className="inline-flex items-center gap-0.5 text-[10px] text-muted-foreground"
          title={`Delivery: ${DELIVERY_STATUS_LABEL[delivery]}`}
        >
          <GitCommitHorizontal size={10} />
          {DELIVERY_STATUS_LABEL[delivery]}
        </span>
      )}
    </span>
  );
}

/** Fraction label for a derived `ProgressCount`, e.g. "2/5". */
function fraction(count: ProgressCount | undefined): string | null {
  if (!count || count.total === 0) return null;
  return `${count.completed}/${count.total}`;
}

interface EpicsPanelProps {
  projectPath: string;
}

export function EpicsPanel({ projectPath }: EpicsPanelProps) {
  const navigate = useNavigate();
  const [epics, setEpics] = useState<Epic[]>([]);
  const [loopStatus, setLoopStatus] = useState<LoopStatus | null>(null);
  const [progress, setProgress] = useState<ProgressSnapshot | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [promoting, setPromoting] = useState<string | null>(null);
  const [toggling, setToggling] = useState<string | null>(null);
  const [assigningId, setAssigningId] = useState<string | null>(null);
  const [exporting, setExporting] = useState(false);
  /** Relative path of the PRD expanded into Content/Checklist tabs, or null. */
  const [expandedSpec, setExpandedSpec] = useState<string | null>(null);
  /** Stable execution IDs checked in the overnight-run phase picker, in
   * selection order (prd-run-queue Phase 5). */
  const [selectedForRun, setSelectedForRun] = useState<string[]>([]);

  const toggleSelectedForRun = (id: string) => {
    setSelectedForRun((prev) =>
      prev.includes(id) ? prev.filter((x) => x !== id) : [...prev, id],
    );
  };

  // Stable ID -> title, so the run-queue view can show human-readable phase
  // names instead of raw execution IDs.
  const idToTitle = useMemo(() => {
    const map: Record<string, string> = {};
    for (const epic of epics) {
      for (const prd of epic.prds) {
        for (const phase of prd.phases) {
          for (const loop of phase.loops) {
            if (loop.id) map[loop.id] = loop.title;
          }
        }
      }
    }
    return map;
  }, [epics]);

  // Derived progress is a best-effort enrichment: a project still in legacy
  // mode (no execution.yaml) has no snapshot, and the panel falls back to
  // checkbox-derived counts. A fetch failure here must not break the epics view.
  const loadProgress = useCallback(async () => {
    try {
      const snapshot = await api.getProgressSnapshot(projectPath);
      setProgress(snapshot.execution_file_present ? snapshot : null);
    } catch {
      setProgress(null);
    }
  }, [projectPath]);

  const load = useCallback(async () => {
    try {
      const [epicData, loops] = await Promise.all([
        api.getEpics(projectPath),
        api.getLoops(projectPath),
      ]);
      setEpics(epicData);
      setLoopStatus(loops);
      setError(null);
    } catch (err) {
      setError(String(err));
    } finally {
      setLoading(false);
    }
    loadProgress();
  }, [projectPath, loadProgress]);

  useEffect(() => {
    let cancelled = false;
    setLoading(true);
    async function run() {
      try {
        const [epicData, loops] = await Promise.all([
          api.getEpics(projectPath),
          api.getLoops(projectPath),
        ]);
        if (!cancelled) {
          setEpics(epicData);
          setLoopStatus(loops);
          setError(null);
        }
      } catch (err) {
        if (!cancelled) setError(String(err));
      } finally {
        if (!cancelled) setLoading(false);
      }
    }
    run();
    loadProgress();
    return () => {
      cancelled = true;
    };
  }, [projectPath, loadProgress]);

  const handleExportSummary = async () => {
    setExporting(true);
    try {
      const written = await api.exportExecutionSummary(projectPath);
      toast.success("Execution summary exported", {
        description: written,
      });
    } catch (err) {
      const appErr = err as AppError;
      toast.error("Failed to export summary", {
        description: appErr.message ?? String(err),
      });
    } finally {
      setExporting(false);
    }
  };

  const currentGoal = loopStatus?.current?.goal ?? null;
  const hasActiveLoop = !!loopStatus?.current;

  const handlePromote = async (
    epicSlug: string,
    prdFile: string,
    loop: PrdLoop,
  ) => {
    const key = `${epicSlug}/${prdFile}/${loop.title}`;
    setPromoting(key);
    try {
      await api.promoteEpicLoop(projectPath, epicSlug, prdFile, loop.title);
      toast.success("Loop promoted", { description: loop.title });
      // Refetch both so the active-loop highlight + done flags update.
      await load();
    } catch (err) {
      const appErr = err as AppError;
      if (appErr.kind === "conflict") {
        toast.error("A loop is already in progress", {
          description: "Complete or abandon the current loop before promoting another.",
        });
      } else {
        toast.error("Failed to promote loop", {
          description: appErr.message ?? String(err),
        });
      }
    } finally {
      setPromoting(null);
    }
  };

  const handleToggle = async (
    epicSlug: string,
    prdFile: string,
    loop: PrdLoop,
  ) => {
    const key = `${epicSlug}/${prdFile}/${loop.title}`;
    setToggling(key);
    try {
      const nowChecked = await api.togglePrdLoop(
        projectPath,
        epicSlug,
        prdFile,
        loop.title,
      );
      // Optimistic local update: flip the checked flag in place.
      setEpics((prev) =>
        prev.map((e) =>
          e.slug === epicSlug
            ? {
                ...e,
                prds: e.prds.map((p) =>
                  p.file === prdFile
                    ? {
                        ...p,
                        phases: p.phases.map((ph) => ({
                          ...ph,
                          loops: ph.loops.map((l) =>
                            l.title === loop.title
                              ? { ...l, checked: nowChecked }
                              : l,
                          ),
                        })),
                      }
                    : p,
                ),
              }
            : e,
        ),
      );
      // The discrepancy badge depends on checked vs. derived execution state —
      // refresh it so toggling doesn't leave a stale discrepancy warning.
      loadProgress();
    } catch (err) {
      toast.error("Failed to toggle", { description: String(err) });
    } finally {
      setToggling(null);
    }
  };

  const handleAssignId = async (
    epicSlug: string,
    prdFile: string,
    loop: PrdLoop,
  ) => {
    const key = `${epicSlug}/${prdFile}/${loop.title}`;
    setAssigningId(key);
    try {
      const id = await api.assignLoopId(projectPath, epicSlug, prdFile, loop.title);
      toast.success("ID assigned", { description: id });
      // The loop's id is now set, so re-derive picker/promote enablement —
      // refetch rather than patch, same as handlePromote.
      await load();
    } catch (err) {
      const appErr = err as AppError;
      toast.error("Failed to assign ID", {
        description: appErr.message ?? String(err),
      });
    } finally {
      setAssigningId(null);
    }
  };

  if (loading) {
    return <LoadingSpinner label="Loading epics..." />;
  }

  if (error) {
    return (
      <div className="text-destructive text-sm p-3">Failed to load epics: {error}</div>
    );
  }

  if (epics.length === 0) {
    return (
      <div className="flex flex-col items-center justify-center py-16 text-center">
        <Layers size={32} className="text-muted-foreground/30 mb-3" />
        <h3 className="text-sm font-semibold text-foreground mb-1.5">
          No epics authored
        </h3>
        <p className="text-xs text-muted-foreground max-w-xs leading-relaxed">
          Epics live under{" "}
          <code className="font-mono text-[11px] bg-muted px-1 py-0.5 rounded">
            docs/epics/&lt;slug&gt;/
          </code>{" "}
          as git-tracked spec files. Author a README plus PRDs to start planning
          with the Epic → PRD → Phase → Loop hierarchy.
        </p>
      </div>
    );
  }

  return (
    <div className="max-w-2xl">
      {/* Active-loop banner */}
      {hasActiveLoop && (
        <div className="mb-4 rounded-lg border border-[var(--primary)]/30 bg-[color-mix(in_oklab,var(--primary)_8%,transparent)] p-3">
          <div className="flex items-center gap-2 text-xs">
            <Zap size={12} className="text-[var(--primary)]" />
            <span className="font-semibold text-foreground">Loop in progress:</span>
            <span className="text-muted-foreground">{currentGoal}</span>
          </div>
          <p className="mt-1 pl-5 text-[10px] text-muted-foreground">
            Promote is disabled until the current loop is completed or abandoned.
          </p>
        </div>
      )}

      <div className="mb-4 flex items-center justify-between">
        <span className="text-xs text-muted-foreground">
          {epics.length} epic{epics.length !== 1 ? "s" : ""}
        </span>
        {progress && (
          <button
            onClick={handleExportSummary}
            disabled={exporting}
            title="Write a non-authoritative Markdown snapshot of derived execution progress to .loopdeck/execution-summary.md"
            className="flex items-center gap-1.5 rounded-md px-2 py-1 text-[11px] font-medium text-muted-foreground transition-colors hover:bg-accent hover:text-foreground disabled:opacity-50"
          >
            {exporting ? (
              <Loader2 size={11} className="animate-spin" />
            ) : (
              <FileDown size={11} />
            )}
            Export summary
          </button>
        )}
      </div>

      {/* Unmatched execution records: an execution.yaml ID with no current
          PRD checklist match — surfaced, never silently dropped. */}
      {progress && progress.unmatched.length > 0 && (
        <div className="mb-4 rounded-lg border border-[color-mix(in_oklab,var(--destructive)_35%,transparent)] bg-[color-mix(in_oklab,var(--destructive)_6%,transparent)] p-3">
          <div className="flex items-center gap-1.5 text-[11px] font-semibold text-foreground">
            <AlertTriangle size={12} style={{ color: "var(--destructive)" }} />
            Unmatched execution records
          </div>
          <p className="mt-1 text-[10px] text-muted-foreground">
            Recorded in execution.yaml but no PRD checklist item currently carries
            this ID (renamed, removed, or from another project's history).
          </p>
          <ul className="mt-2 space-y-1">
            {progress.unmatched.map((u) => (
              <li key={u.id} className="flex items-center gap-2 text-xs">
                <code className="rounded bg-muted px-1 py-0.5 font-mono text-[10px] text-muted-foreground">
                  {u.id}
                </code>
                <span className="text-foreground">{u.title}</span>
              </li>
            ))}
          </ul>
        </div>
      )}

      <RunQueuePanel
        projectPath={projectPath}
        selectedIds={selectedForRun}
        idToTitle={idToTitle}
        onQueued={() => setSelectedForRun([])}
      />

      <div className="space-y-4">
        {epics.map((epic) => {
          const epicCount = progress?.epics[epic.slug];
          const epicFraction = fraction(epicCount);
          return (
          <div
            key={epic.slug}
            className="rounded-xl border border-border bg-card p-4"
          >
            {/* Epic header */}
            <div className="mb-3 flex items-start justify-between gap-2">
              <div className="min-w-0 flex-1">
                <h3 className="text-sm font-semibold text-foreground">{epic.title}</h3>
                <div className="mt-0.5 flex items-center gap-2 text-[10px] text-muted-foreground">
                  <span className="font-mono">{epic.slug}</span>
                  <span>·</span>
                  <span className="uppercase tracking-wider">{epic.status.replace("_", " ")}</span>
                  <span>·</span>
                  <span>milestone {epic.milestone}</span>
                </div>
              </div>
              <button
                onClick={() =>
                  navigate({ to: "/spec/$relPath", params: { relPath: encodeURIComponent(`${epic.slug}/README.md`) } })
                }
                title="Edit epic README"
                className="flex size-6 shrink-0 items-center justify-center rounded-md text-muted-foreground transition-colors hover:bg-accent hover:text-foreground"
              >
                <Pencil className="size-3.5" />
              </button>
            </div>

            {/* Derived epic progress — from execution.yaml, not checkbox state */}
            {epicCount && epicFraction && (
              <div className="mb-3 flex items-center gap-2">
                <Progress
                  value={Math.round((epicCount.completed / epicCount.total) * 100)}
                  className="h-1.5"
                />
                <span
                  className="shrink-0 text-[10px] font-mono text-muted-foreground"
                  title="Completed loops / total loops with a stable ID, derived from execution.yaml"
                >
                  {epicFraction}
                </span>
              </div>
            )}

            {/* PRDs */}
            <div className="space-y-3">
              {epic.prds.map((prd) => {
                const prdKey = `${epic.slug}/${prd.file}`;
                const isExpanded = expandedSpec === prdKey;
                const loopCount = prd.phases.reduce((n, p) => n + p.loops.length, 0);
                const prdIsActive =
                  hasActiveLoop &&
                  prd.phases.some((p) => p.loops.some((l) => l.title === currentGoal));
                const prdCount = progress?.prds[`${epic.slug}/${prd.slug}`];
                const prdFraction = fraction(prdCount);
                return (
                <div key={prd.file} className="rounded-lg border border-border/60 p-3">
                  <div className="flex items-center justify-between">
                    {/* Tap to expand/collapse into Content + Checklist tabs */}
                    <button
                      onClick={() => setExpandedSpec(isExpanded ? null : prdKey)}
                      title={isExpanded ? "Collapse PRD" : "Expand PRD"}
                      className="flex min-w-0 items-center gap-1.5 rounded text-left transition-colors hover:text-foreground"
                    >
                      <ChevronRight
                        size={12}
                        className={`shrink-0 text-muted-foreground transition-transform ${
                          isExpanded ? "rotate-90" : ""
                        }`}
                      />
                      <span className="text-xs font-medium text-foreground">{prd.slug}</span>
                      <span className="text-[10px] text-muted-foreground">
                        {prdFraction ?? `${loopCount} loop${loopCount !== 1 ? "s" : ""}`}
                      </span>
                      {prdIsActive && (
                        <span className="inline-flex items-center gap-1 text-[10px] font-medium text-[var(--primary)]">
                          <Zap size={9} />
                          active
                        </span>
                      )}
                    </button>
                    <div className="flex items-center gap-2">
                      <button
                        onClick={() =>
                          navigate({ to: "/spec/$relPath", params: { relPath: encodeURIComponent(prdKey) } })
                        }
                        title={`Edit ${prd.file}`}
                        className="flex size-5 items-center justify-center rounded text-muted-foreground transition-colors hover:bg-accent hover:text-foreground"
                      >
                        <Pencil className="size-3" />
                      </button>
                      <span className="text-[10px] uppercase tracking-wider text-muted-foreground">
                        {prd.status}
                      </span>
                    </div>
                  </div>

                  {isExpanded ? (
                    <Tabs defaultValue="content" className="mt-2">
                      <TabsList className="h-7">
                        <TabsTrigger value="content" className="px-2.5 py-0.5 text-[11px]">
                          Content
                        </TabsTrigger>
                        <TabsTrigger value="checklist" className="px-2.5 py-0.5 text-[11px]">
                          Checklist
                        </TabsTrigger>
                      </TabsList>

                      {/* Content — full PRD markdown body, lazy-loaded */}
                      <TabsContent value="content">
                        <PrdContent projectPath={projectPath} relPath={prdKey} />
                      </TabsContent>

                      {/* Checklist — the existing toggle/promote phase list */}
                      <TabsContent value="checklist">
                        <div className="space-y-3">
                          {prd.phases.map((phase) => (
                            <div key={phase.name}>
                              <h4 className="mb-1.5 text-[10px] font-semibold uppercase tracking-wider text-muted-foreground">
                                {phase.name}
                              </h4>
                              <ul className="space-y-1">
                                {phase.loops.map((loop, i) => {
                                  const key = `${epic.slug}/${prd.file}/${loop.title}`;
                                  const done = loop.checked || loop.done_in_history;
                                  const isCurrent =
                                    hasActiveLoop && currentGoal === loop.title;
                                  const disabled =
                                    hasActiveLoop && !isCurrent;
                                  const isPromoting = promoting === key;
                                  const isToggling = toggling === key;
                                  const isAssigningId = assigningId === key;
                                  // Legacy ID-less items can't be promoted (Phase 1):
                                  // the stable ID is the spec→execution join key.
                                  const noId = !loop.id;
                                  const derived = loop.id ? progress?.loops[loop.id] : undefined;
                                  return (
                                    <li
                                      key={i}
                                      className={`flex items-start gap-2 rounded px-1.5 py-1 text-xs leading-relaxed ${
                                        isCurrent
                                          ? "bg-[color-mix(in_oklab,var(--primary)_10%,transparent)]"
                                          : ""
                                      }`}
                                    >
                                      <button
                                        onClick={() => handleToggle(epic.slug, prd.file, loop)}
                                        disabled={isToggling}
                                        title={loop.checked ? "Mark as not done" : "Mark as done"}
                                        className="mt-0.5 shrink-0 text-muted-foreground transition-colors hover:text-foreground disabled:opacity-50"
                                      >
                                        {isToggling ? (
                                          <Loader2 size={12} className="animate-spin" />
                                        ) : done ? (
                                          <CheckCircle2 size={12} className="text-[var(--success)]" />
                                        ) : (
                                          <Circle size={12} />
                                        )}
                                      </button>
                                      <span
                                        className={`flex-1 ${
                                          done ? "text-muted-foreground line-through" : "text-foreground"
                                        }`}
                                      >
                                        {loop.title}
                                      </span>

                                      {/* Derived execution/delivery state — from execution.yaml,
                                          independent of the authored checkbox above */}
                                      {derived && (
                                        <DerivedStatusBadge
                                          execution={derived.execution}
                                          delivery={derived.delivery}
                                        />
                                      )}
                                      {derived?.discrepancy && (
                                        <span
                                          className="mt-0.5 shrink-0"
                                          title={derived.discrepancy}
                                        >
                                          <AlertTriangle
                                            size={11}
                                            style={{ color: "var(--warning)" }}
                                          />
                                        </span>
                                      )}

                                      {/* Overnight-run picker checkbox — only actionable on loops
                                          with a stable ID (the join key create_run_plan needs), but
                                          always rendered (disabled + explained) so a missing ID reads
                                          as "why is this disabled" instead of "where did it go" —
                                          same disabled-with-tooltip pattern as promote below. */}
                                      {!done && (
                                        <button
                                          onClick={() => loop.id && toggleSelectedForRun(loop.id)}
                                          disabled={noId}
                                          title={
                                            noId
                                              ? "Add a stable ID `namespace/loop` before this loop can be queued for an overnight run"
                                              : selectedForRun.includes(loop.id!)
                                                ? "Remove from overnight-run selection"
                                                : "Add to overnight-run selection"
                                          }
                                          className={`shrink-0 transition-colors ${
                                            noId
                                              ? "cursor-not-allowed text-muted-foreground/40"
                                              : loop.id && selectedForRun.includes(loop.id)
                                                ? "text-[var(--primary)]"
                                                : "text-muted-foreground/40 hover:text-foreground"
                                          }`}
                                        >
                                          {loop.id && selectedForRun.includes(loop.id) ? (
                                            <CheckSquare size={12} />
                                          ) : (
                                            <Square size={12} />
                                          )}
                                        </button>
                                      )}

                                      {/* Assign ID — the only path off the disabled picker checkbox
                                          above for a legacy id-less loop; generates a stable
                                          `epic-slug/title-slug` id and unlocks the picker on success. */}
                                      {!done && noId && (
                                        <button
                                          onClick={() =>
                                            handleAssignId(epic.slug, prd.file, loop)
                                          }
                                          disabled={isAssigningId}
                                          title="Generate a stable ID for this loop so it can be queued for an overnight run"
                                          className="shrink-0 rounded px-1.5 py-0.5 text-[10px] font-medium text-muted-foreground transition-colors hover:bg-accent hover:text-foreground disabled:opacity-50"
                                        >
                                          {isAssigningId ? (
                                            <Loader2 size={10} className="animate-spin" />
                                          ) : (
                                            <span className="inline-flex items-center gap-1">
                                              <Fingerprint size={10} />
                                              Assign ID
                                            </span>
                                          )}
                                        </button>
                                      )}

                                      {/* Promote action — only on not-done loops with a stable ID */}
                                      {!done && (
                                        <button
                                          onClick={() => handlePromote(epic.slug, prd.file, loop)}
                                          disabled={disabled || isPromoting || noId}
                                          title={
                                            noId
                                              ? "Add a stable ID `namespace/loop` before this loop can be promoted"
                                              : isCurrent
                                                ? "Currently in progress"
                                                : disabled
                                                  ? "Another loop is in progress"
                                                  : "Promote to current loop"
                                          }
                                          className={`shrink-0 rounded px-1.5 py-0.5 text-[10px] font-medium transition-colors ${
                                            isCurrent
                                              ? "text-[var(--primary)]"
                                              : disabled || noId
                                                ? "cursor-not-allowed text-muted-foreground/40"
                                                : "text-muted-foreground hover:bg-accent hover:text-foreground"
                                          }`}
                                        >
                                          {isPromoting ? (
                                            <Loader2 size={10} className="animate-spin" />
                                          ) : isCurrent ? (
                                            "active"
                                          ) : (
                                            "promote"
                                          )}
                                        </button>
                                      )}
                                    </li>
                                  );
                                })}
                              </ul>
                            </div>
                          ))}
                          {prd.phases.length === 0 && (
                            <p className="text-[11px] italic text-muted-foreground">
                              No phases defined.
                            </p>
                          )}
                        </div>
                      </TabsContent>
                    </Tabs>
                  ) : null}
                </div>
                );
              })}
              {epic.prds.length === 0 && (
                <p className="text-xs italic text-muted-foreground">
                  No PRDs in this epic.
                </p>
              )}
            </div>
          </div>
          );
        })}
      </div>
    </div>
  );
}
