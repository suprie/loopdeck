import { useState, useEffect, useMemo } from "react";
import {
  Layers,
  Loader2,
  AlertTriangle,
  ChevronDown,
  GitBranch,
  CircleDot,
  CheckCircle2,
  Circle,
} from "lucide-react";
import type { Epic, Prd, PrdLoop, ProgressCount } from "../../types";
import * as api from "../../lib/tauri";
import { EPIC_STATUS_LABEL, EPIC_STATUS_RANK, sortEpics } from "../../lib/utils";
import { useAppStore } from "../../store/appStore";
import { PageHeader } from "../layout/AppShell";
import { Progress } from "../ui/progress";

// ── Types ────────────────────────────────────────────────────────────────────

interface EnrichedEpic extends Epic {
  projectName: string;
  projectPath: string;
  /** Derived from execution.yaml plus authored fallback for loops without a
   * structured record; undefined for projects still in legacy/empty mode. */
  derivedProgress?: ProgressCount;
}

// ── Helpers ──────────────────────────────────────────────────────────────────

const EPIC_STATUS_COLOR: Record<string, string> = {
  proposed: "var(--warning)",
  in_progress: "var(--primary)",
  completed: "var(--success)",
  abandoned: "var(--muted-foreground)",
};

const EPIC_STATUS_BG: Record<string, string> = {
  proposed: "bg-[color-mix(in_oklab,var(--warning)_15%,transparent)]",
  in_progress: "bg-[color-mix(in_oklab,var(--primary)_12%,transparent)]",
  completed: "bg-[color-mix(in_oklab,var(--success)_12%,transparent)]",
  abandoned: "bg-muted/50",
};

/**
 * Total + done counts across all phases of all PRDs in an epic. Prefers the
 * backend-derived count when the project has structured state. The backend
 * uses structured completion where a record exists and authored completion
 * for checklist items that have not been migrated into execution state.
 */
function epicProgress(epic: EnrichedEpic): { done: number; total: number } {
  if (epic.derivedProgress) {
    return { done: epic.derivedProgress.completed, total: epic.derivedProgress.total };
  }
  let done = 0;
  let total = 0;
  for (const prd of epic.prds) {
    for (const phase of prd.phases) {
      for (const l of phase.loops) {
        total += 1;
        if (l.checked || l.done_in_history) done += 1;
      }
    }
  }
  return { done, total };
}

// ── Component ────────────────────────────────────────────────────────────────

export function EpicsView() {
  const projects = useAppStore((s) => s.projects);

  // ── Data ──
  const [byStatus, setByStatus] = useState<[string, EnrichedEpic[]][]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);

  // ── Expanded detail ──
  const [expandedEpic, setExpandedEpic] = useState<string | null>(null);
  const [expandedPrd, setExpandedPrd] = useState<string | null>(null);
  const [collapsedStatuses, setCollapsedStatuses] = useState<Set<string>>(
    new Set(),
  );

  // ── Load all epics from all projects, grouped by status (primary sort),
  // milestone within each status group (secondary sort) ──────────────────

  useEffect(() => {
    let cancelled = false;

    async function load() {
      setLoading(true);
      setError(null);

      const allEpics: EnrichedEpic[] = [];

      for (const p of projects) {
        if (cancelled) break;
        try {
          const grouped = await api.getEpicsByMilestone(p.path);
          // Best-effort: legacy/empty-mode projects have no execution.yaml,
          // so this stays undefined and epicProgress() falls back to checkboxes.
          const snapshot = await api.getProgressSnapshot(p.path).catch(() => null);
          for (const epics of Object.values(grouped)) {
            for (const e of epics) {
              allEpics.push({
                ...e,
                projectName: p.name,
                projectPath: p.path,
                derivedProgress:
                  snapshot?.execution_file_present ? snapshot.epics[e.slug] : undefined,
              });
            }
          }
        } catch {
          // No docs/epics/ — skip.
        }
      }

      if (!cancelled) {
        // Group by status.
        const map = new Map<string, EnrichedEpic[]>();
        for (const epic of allEpics) {
          const existing = map.get(epic.status);
          if (existing) existing.push(epic);
          else map.set(epic.status, [epic]);
        }
        // Sort within each status group: milestone ascending (0.4.0 above
        // 0.5.0), then project name, then epic slug.
        // Every epic in a group shares the same status, so sortEpics reduces
        // to milestone-ascending then slug here — same rule the per-project
        // EpicsPanel uses, kept in one place.
        const sorted: [string, EnrichedEpic[]][] = [...map.entries()]
          .sort(([a], [b]) => (EPIC_STATUS_RANK[a] ?? 1) - (EPIC_STATUS_RANK[b] ?? 1))
          .map(([status, list]) => [status, sortEpics(list)]);
        setByStatus(sorted);
        setLoading(false);
      }
    }

    if (projects.length > 0) {
      load();
    } else {
      setLoading(false);
      setByStatus([]);
    }

    return () => {
      cancelled = true;
    };
  }, [projects]);

  const totalEpics = useMemo(
    () => byStatus.reduce((n, [, es]) => n + es.length, 0),
    [byStatus],
  );

  const toggleStatus = (status: string) => {
    setCollapsedStatuses((prev) => {
      const next = new Set(prev);
      if (next.has(status)) next.delete(status);
      else next.add(status);
      return next;
    });
  };

  // ── Render ─────────────────────────────────────────────────────────────

  return (
    <div className="flex-1 flex flex-col min-h-0">
      <PageHeader
        title="Epics"
        subtitle="Spec-layer plans tracked across projects"
        actions={
          !loading && totalEpics > 0 ? (
            <span className="text-[11px] text-muted-foreground">
              {totalEpics} epic{totalEpics !== 1 ? "s" : ""}
            </span>
          ) : undefined
        }
      />

      {/* ── Body ── */}
      <div className="flex-1 min-h-0 overflow-y-auto">
        {/* Loading */}
        {loading && (
          <div className="flex flex-col items-center justify-center py-20 gap-4 text-muted-foreground">
            <Loader2 className="size-8 animate-spin" />
            <span className="text-sm">Loading epics…</span>
          </div>
        )}

        {/* Error */}
        {error && !loading && (
          <div className="flex items-center justify-center py-20">
            <div className="flex flex-col items-center gap-3 text-center max-w-sm">
              <AlertTriangle className="size-8 text-destructive/60" />
              <p className="text-sm text-destructive">{error}</p>
            </div>
          </div>
        )}

        {/* Empty */}
        {!loading && !error && totalEpics === 0 && (
          <div className="flex flex-col items-center justify-center py-20 text-center">
            <Layers size={40} className="text-muted-foreground/20 mb-4" />
            <h3 className="text-sm font-semibold text-foreground mb-1.5">
              No epics yet
            </h3>
            <p className="text-xs text-muted-foreground max-w-xs leading-relaxed">
              Epics are authored under{" "}
              <code className="font-mono text-[11px] bg-muted px-1 py-0.5 rounded">
                docs/epics/&lt;slug&gt;/
              </code>{" "}
              as git-tracked spec files. Each holds a README plus co-located
              PRDs with phase checklists.
            </p>
          </div>
        )}

        {/* Status sections */}
        {!loading && !error && totalEpics > 0 && (
          <div className="mx-auto w-full max-w-3xl space-y-6 px-8 py-8">
            {byStatus.map(([status, epics]) => {
              const collapsed = collapsedStatuses.has(status);
              return (
                <section key={status}>
                  <button
                    onClick={() => toggleStatus(status)}
                    className="mb-3 flex items-center gap-1.5 text-[10px] font-semibold uppercase tracking-wider text-muted-foreground hover:text-foreground transition-colors"
                  >
                    <ChevronDown
                      className={`size-3 transition-transform ${collapsed ? "-rotate-90" : ""}`}
                    />
                    {EPIC_STATUS_LABEL[status] ?? status}
                    <span className="font-normal text-muted-foreground/60">
                      · {epics.length}
                    </span>
                  </button>

                  {!collapsed && (
                    <div className="space-y-3">
                      {epics.map((epic) => (
                        <EpicCard
                          key={`${epic.projectPath}-${epic.slug}`}
                          epic={epic}
                          expanded={expandedEpic === `${epic.projectPath}/${epic.slug}`}
                          onToggle={() =>
                            setExpandedEpic((prev) =>
                              prev === `${epic.projectPath}/${epic.slug}`
                                ? null
                                : `${epic.projectPath}/${epic.slug}`,
                            )
                          }
                          expandedPrd={expandedPrd}
                          onTogglePrd={(key) =>
                            setExpandedPrd((prev) => (prev === key ? null : key))
                          }
                        />
                      ))}
                    </div>
                  )}
                </section>
              );
            })}
          </div>
        )}
      </div>
    </div>
  );
}

// ── Epic card ────────────────────────────────────────────────────────────────

function EpicCard({
  epic,
  expanded,
  onToggle,
  expandedPrd,
  onTogglePrd,
}: {
  epic: EnrichedEpic;
  expanded: boolean;
  onToggle: () => void;
  expandedPrd: string | null;
  onTogglePrd: (key: string) => void;
}) {
  const { done, total } = epicProgress(epic);
  const pct = total > 0 ? Math.round((done / total) * 100) : 0;

  // Lazily backfill `order:` on any PRD missing it, once, the first time the
  // epic is expanded. The list is already shown correctly via the backend's
  // README/filename fallback sort — this only persists that order onto disk
  // so it becomes explicit and user-editable. Idempotent; fire-and-forget.
  useEffect(() => {
    if (!expanded) return;
    if (epic.prds.every((prd) => prd.order != null)) return;
    api.migratePrdOrder(epic.projectPath, epic.slug).catch(() => {});
  }, [expanded, epic.projectPath, epic.slug, epic.prds]);

  return (
    <div
      className={`rounded-xl border bg-card p-5 shadow-[var(--shadow-sm)] transition-colors ${
        EPIC_STATUS_BG[epic.status] ?? "border-border"
      } ${expanded ? "ring-1 ring-border" : ""}`}
    >
      <button onClick={onToggle} className="w-full text-left">
        <div className="flex items-start justify-between gap-3">
          <div className="min-w-0 flex-1">
            {/* Project chip + milestone */}
            <div className="flex items-center gap-2 text-[10px] font-semibold uppercase tracking-wider text-muted-foreground">
              <GitBranch className="size-3" />
              {epic.projectName}
              {epic.milestone && (
                <span className="font-normal normal-case text-muted-foreground/60">
                  · {epic.milestone}
                </span>
              )}
            </div>

            {/* Title */}
            <h3 className="mt-2 text-sm font-semibold tracking-tight text-foreground leading-snug">
              {epic.title}
            </h3>

            {/* Description preview (collapsed) */}
            {!expanded && (
              <p className="mt-1.5 text-xs leading-relaxed text-muted-foreground line-clamp-2">
                {epic.description}
              </p>
            )}
          </div>

          {/* Status badge + chevron */}
          <div className="flex shrink-0 items-center gap-2">
            <span
              className="inline-flex items-center rounded px-1.5 py-0.5 text-[10px] font-medium uppercase tracking-wider"
              style={{
                color: EPIC_STATUS_COLOR[epic.status] ?? "var(--muted-foreground)",
              }}
            >
              {epic.status.replace("_", " ")}
            </span>
            <ChevronDown
              className={`size-3.5 text-muted-foreground transition-transform ${expanded ? "rotate-180" : ""}`}
            />
          </div>
        </div>

        {/* Progress */}
        {total > 0 && (
          <div className="mt-3 flex items-center gap-2">
            <Progress value={pct} className="h-1.5" />
            <span className="shrink-0 text-[10px] font-mono text-muted-foreground">
              {done}/{total}
            </span>
          </div>
        )}
      </button>

      {/* Expanded: PRD list */}
      {expanded && (
        <div className="mt-3 space-y-2 border-t border-border/50 pt-3">
          {epic.prds.length === 0 && (
            <p className="text-xs text-muted-foreground italic">
              No PRDs in this epic yet.
            </p>
          )}
          {epic.prds.map((prd) => {
            const key = `${epic.projectPath}/${epic.slug}/${prd.file}`;
            const prdExpanded = expandedPrd === key;
            return (
              <PrdRow
                key={key}
                prd={prd}
                expanded={prdExpanded}
                onToggle={() => onTogglePrd(key)}
              />
            );
          })}
        </div>
      )}
    </div>
  );
}

// ── PRD row (within an expanded epic) ────────────────────────────────────────

function PrdRow({ prd, expanded, onToggle }: { prd: Prd; expanded: boolean; onToggle: () => void }) {
  return (
    <div className="rounded-lg border border-border/60 bg-surface/40">
      <button
        onClick={onToggle}
        className="flex w-full items-center justify-between gap-2 px-3 py-2 text-left"
      >
        <div className="min-w-0 flex-1">
          <span className="text-xs font-medium text-foreground">{prd.slug}</span>
          <span className="ml-2 text-[10px] text-muted-foreground">
            {prd.phases.length} phase{prd.phases.length !== 1 ? "s" : ""}
          </span>
        </div>
        <span
          className="shrink-0 text-[10px] font-medium uppercase tracking-wider"
          style={{ color: "var(--muted-foreground)" }}
        >
          {prd.status}
        </span>
        <ChevronDown
          className={`size-3 text-muted-foreground transition-transform ${expanded ? "rotate-180" : ""}`}
        />
      </button>

      {expanded && (
        <div className="space-y-3 px-3 pb-3">
          {prd.phases.map((phase) => (
            <div key={phase.name}>
              <h4 className="mb-1.5 text-[10px] font-semibold uppercase tracking-wider text-muted-foreground">
                {phase.name}
              </h4>
              <ul className="space-y-1">
                {phase.loops.map((loop, i) => (
                  <LoopItem key={i} loop={loop} />
                ))}
              </ul>
            </div>
          ))}
        </div>
      )}
    </div>
  );
}

// ── Loop item (read-only in the cross-project view) ──────────────────────────

function LoopItem({ loop }: { loop: PrdLoop }) {
  const done = loop.checked || loop.done_in_history;
  return (
    <li className="flex items-start gap-2 text-xs text-foreground leading-relaxed">
      {done ? (
        <CheckCircle2 size={12} className="mt-0.5 shrink-0 text-[var(--success)]" />
      ) : (
        <Circle size={12} className="mt-0.5 shrink-0 text-muted-foreground" />
      )}
      <span className={done ? "text-muted-foreground line-through" : ""}>
        {loop.title}
      </span>
      {loop.id && (
        <code className="ml-1 mt-0.5 shrink-0 rounded bg-muted px-1 py-0.5 font-mono text-[10px] text-muted-foreground">
          {loop.id}
        </code>
      )}
      {loop.done_in_history && !loop.checked && (
        <CircleDot
          size={10}
          className="ml-1 mt-1 shrink-0 text-[var(--success)]/60"
          aria-label="matched in history"
        />
      )}
    </li>
  );
}
