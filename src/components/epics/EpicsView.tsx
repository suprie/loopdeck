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
import type { Epic, Prd, PrdLoop } from "../../types";
import * as api from "../../lib/tauri";
import { useAppStore } from "../../store/appStore";
import { PageHeader } from "../layout/AppShell";
import { Progress } from "../ui/progress";

// ── Types ────────────────────────────────────────────────────────────────────

interface EnrichedEpic extends Epic {
  projectName: string;
  projectPath: string;
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

/** Total + done counts across all phases of all PRDs in an epic. */
function epicProgress(epic: Epic): { done: number; total: number } {
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
  const [byMilestone, setByMilestone] = useState<[string, EnrichedEpic[]][]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);

  // ── Expanded detail ──
  const [expandedEpic, setExpandedEpic] = useState<string | null>(null);
  const [expandedPrd, setExpandedPrd] = useState<string | null>(null);
  const [collapsedMilestones, setCollapsedMilestones] = useState<Set<string>>(
    new Set(),
  );

  // ── Load all epics from all projects, grouped by milestone ─────────────

  useEffect(() => {
    let cancelled = false;

    async function load() {
      setLoading(true);
      setError(null);

      // Map milestone → list of enriched epics. Insertion order preserved by
      // iterating the backend's BTreeMap (already milestone-sorted).
      const map = new Map<string, EnrichedEpic[]>();

      for (const p of projects) {
        if (cancelled) break;
        try {
          const grouped = await api.getEpicsByMilestone(p.path);
          for (const [milestone, epics] of Object.entries(grouped)) {
            const enriched = epics.map((e) => ({
              ...e,
              projectName: p.name,
              projectPath: p.path,
            }));
            const existing = map.get(milestone);
            if (existing) existing.push(...enriched);
            else map.set(milestone, enriched);
          }
        } catch {
          // No docs/epics/ — skip.
        }
      }

      if (!cancelled) {
        // Sort within each milestone by project name then epic slug.
        for (const list of map.values()) {
          list.sort(
            (a, b) =>
              a.projectName.localeCompare(b.projectName) ||
              a.slug.localeCompare(b.slug),
          );
        }
        setByMilestone([...map.entries()]);
        setLoading(false);
      }
    }

    if (projects.length > 0) {
      load();
    } else {
      setLoading(false);
      setByMilestone([]);
    }

    return () => {
      cancelled = true;
    };
  }, [projects]);

  const totalEpics = useMemo(
    () => byMilestone.reduce((n, [, es]) => n + es.length, 0),
    [byMilestone],
  );

  const toggleMilestone = (m: string) => {
    setCollapsedMilestones((prev) => {
      const next = new Set(prev);
      if (next.has(m)) next.delete(m);
      else next.add(m);
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

        {/* Milestone sections */}
        {!loading && !error && totalEpics > 0 && (
          <div className="mx-auto w-full max-w-3xl space-y-6 px-8 py-8">
            {byMilestone.map(([milestone, epics]) => {
              const collapsed = collapsedMilestones.has(milestone);
              return (
                <section key={milestone}>
                  <button
                    onClick={() => toggleMilestone(milestone)}
                    className="mb-3 flex items-center gap-1.5 text-[10px] font-semibold uppercase tracking-wider text-muted-foreground hover:text-foreground transition-colors"
                  >
                    <ChevronDown
                      className={`size-3 transition-transform ${collapsed ? "-rotate-90" : ""}`}
                    />
                    {milestone}
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
