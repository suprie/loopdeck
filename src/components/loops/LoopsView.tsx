import { useState, useEffect, useMemo } from "react";
import { Repeat, RotateCw, Check, Loader2, AlertTriangle, Play, CheckCircle2, Circle, ChevronDown } from "lucide-react";
import type { LoopStatus } from "../../types";
import * as api from "../../lib/tauri";
import { useAppStore } from "../../store/appStore";
import { PageHeader } from "../layout/AppShell";
import { cn } from "../../lib/utils";

// ── Types ────────────────────────────────────────────────────────────────────

interface EnrichedLoopStatus extends LoopStatus {
  projectName: string;
  projectPath: string;
}

// ── Component ────────────────────────────────────────────────────────────────

export function LoopsView() {
  const projects = useAppStore((s) => s.projects);

  // ── Data ──
  const [loopStatuses, setLoopStatuses] = useState<EnrichedLoopStatus[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);

  // ── Expanded cards ──
  const [expandedPath, setExpandedPath] = useState<string | null>(null);

  // ── Load all loop statuses from all projects ────────────────────────────

  useEffect(() => {
    let cancelled = false;

    async function load() {
      setLoading(true);
      setError(null);

      const all: EnrichedLoopStatus[] = [];

      for (const p of projects) {
        if (cancelled) break;
        try {
          const ls = await api.getLoops(p.path);
          all.push({
            ...ls,
            projectName: p.name,
            projectPath: p.path,
          });
        } catch {
          // No loops file — skip.
        }
      }

      if (!cancelled) {
        // Sort: projects with in-progress loops first, then by name.
        all.sort((a, b) => {
          const aActive = a.current?.status === "in_progress" ? 0 : 1;
          const bActive = b.current?.status === "in_progress" ? 0 : 1;
          if (aActive !== bActive) return aActive - bActive;
          return a.projectName.localeCompare(b.projectName);
        });
        setLoopStatuses(all);
        setLoading(false);
      }
    }

    if (projects.length > 0) {
      load();
    } else {
      setLoading(false);
      setLoopStatuses([]);
    }

    return () => { cancelled = true; };
  }, [projects]);

  // ── Stats ───────────────────────────────────────────────────────────────

  const stats = useMemo(() => {
    const active = loopStatuses.filter((l) => l.current?.status === "in_progress");
    const totalNextSteps = loopStatuses.reduce(
      (sum, l) => sum + l.next_steps.length,
      0,
    );
    const completedHistory = loopStatuses.reduce(
      (sum, l) => sum + l.history.length,
      0,
    );
    return { active: active.length, totalNextSteps, completedHistory };
  }, [loopStatuses]);

  const hasData = loopStatuses.length > 0;

  // ── Render ─────────────────────────────────────────────────────────────

  return (
    <div className="flex-1 flex flex-col min-h-0">
      {/* Header */}
      <PageHeader
        title="Loops"
        subtitle="Active work threads with structured steps"
      />

      {/* Stats bar */}
      {hasData && !loading && (
        <div className="shrink-0 px-6 py-2.5 border-b border-border/50 bg-surface/30 flex items-center gap-4 text-[11px] text-muted-foreground flex-wrap">
          <span>
            <strong className="text-foreground">{stats.active}</strong> active
            loop{stats.active !== 1 ? "s" : ""}
          </span>
          <span className="text-border">|</span>
          <span>
            <strong className="text-foreground">{stats.totalNextSteps}</strong>{" "}
            pending next step{stats.totalNextSteps !== 1 ? "s" : ""}
          </span>
          <span className="text-border">|</span>
          <span>
            <strong className="text-foreground">{stats.completedHistory}</strong>{" "}
            completed loop{stats.completedHistory !== 1 ? "s" : ""}
          </span>
          <span className="text-border">|</span>
          <span>
            <strong className="text-foreground">{loopStatuses.length}</strong>{" "}
            project{loopStatuses.length !== 1 ? "s" : ""}
          </span>
        </div>
      )}

      {/* ── Body ── */}
      <div className="flex-1 min-h-0 overflow-y-auto">
        {/* Loading */}
        {loading && (
          <div className="flex flex-col items-center justify-center py-20 gap-4 text-muted-foreground">
            <Loader2 className="size-8 animate-spin" />
            <span className="text-sm">Loading loops…</span>
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
        {!loading && !error && !hasData && (
          <div className="flex flex-col items-center justify-center py-20 text-center">
            <Repeat size={40} className="text-muted-foreground/20 mb-4" />
            <h3 className="text-sm font-semibold text-foreground mb-1.5">
              No loops yet
            </h3>
            <p className="text-xs text-muted-foreground max-w-xs leading-relaxed">
              Development loops from{" "}
              <code className="font-mono text-[11px] bg-muted px-1 py-0.5 rounded">
                .loopdeck/loops.md
              </code>{" "}
              files will appear here. Start an agent to begin your first
              development loop.
            </p>
          </div>
        )}

        {/* Loop cards */}
        {!loading && !error && hasData && (
          <div className="mx-auto w-full max-w-3xl space-y-4 px-8 py-8">
            {loopStatuses.map((ls) => {
              const isExpanded = expandedPath === ls.projectPath;
              const current = ls.current;
              const isActive = current?.status === "in_progress";

              return (
                <div
                  key={ls.projectPath}
                  className={cn(
                    "rounded-xl border bg-card p-5 shadow-[var(--shadow-sm)] transition-colors",
                    isActive
                      ? "border-[color-mix(in_oklab,var(--success)_30%,transparent)] bg-[color-mix(in_oklab,var(--success)_6%,transparent)]"
                      : "border-border",
                  )}
                >
                  <button
                    onClick={() =>
                      setExpandedPath(isExpanded ? null : ls.projectPath)
                    }
                    className="w-full text-left px-4 py-3"
                  >
                    {/* Header row */}
                    <div className="flex items-start justify-between gap-3">
                      <div className="min-w-0 flex-1">
                        {/* Project chip + status */}
                        <div className="flex items-center gap-2">
                          <RotateCw
                            className={cn(
                              "size-3.5 shrink-0",
                              isActive ? "text-primary" : "text-muted-foreground",
                            )}
                          />
                          <span className="text-[10px] font-semibold uppercase tracking-wider text-muted-foreground">
                            {ls.projectName}
                          </span>
                          {isActive ? (
                            <span className="inline-flex items-center gap-1 text-[10px] font-medium text-[var(--success)]">
                              <Play className="size-2.5 fill-current" />
                              Active
                            </span>
                          ) : current ? (
                            <span className="inline-flex items-center gap-1 text-[10px] font-medium text-muted-foreground">
                              <CheckCircle2 className="size-2.5" />
                              {current.status === "completed" ? "Completed" : "Paused"}
                            </span>
                          ) : (
                            <span className="inline-flex items-center gap-1 text-[10px] font-medium text-muted-foreground">
                              <Circle className="size-2.5" />
                              No current loop
                            </span>
                          )}
                        </div>

                        {/* Current loop goal */}
                        {current ? (
                          <h3 className="mt-2 line-clamp-2 text-sm font-semibold tracking-tight text-foreground">
                            {current.goal}
                          </h3>
                        ) : (
                          <p className="mt-2 text-sm italic text-muted-foreground">
                            No active development loop
                          </p>
                        )}

                        {/* Next-steps count (collapsed) */}
                        {!isExpanded && ls.next_steps.length > 0 && (
                          <p className="mt-1 text-[11px] text-muted-foreground">
                            {ls.next_steps.filter((s) => s.checked).length}/
                            {ls.next_steps.length} done ·{" "}
                            {ls.next_steps.filter((s) => !s.checked).length} pending
                          </p>
                        )}
                      </div>

                      {/* Chevron */}
                      <ChevronDown
                        className={cn(
                          "mt-0.5 size-3.5 shrink-0 text-muted-foreground transition-transform",
                          isExpanded && "rotate-180",
                        )}
                      />
                    </div>

                    {/* Expanded detail */}
                    {isExpanded && (
                      <div className="mt-3 space-y-3 border-t border-border/50 pt-3">
                        {/* Current loop metadata */}
                        {current && (
                          <div className="flex items-center gap-3 text-[11px] text-muted-foreground">
                            <span>
                              Started:{" "}
                              <strong className="text-foreground">{current.started}</strong>
                            </span>
                            {current.completed && (
                              <span>
                                Completed:{" "}
                                <strong className="text-foreground">
                                  {current.completed}
                                </strong>
                              </span>
                            )}
                          </div>
                        )}

                        {/* Next steps — clone-style checklist with check/empty boxes */}
                        {ls.next_steps.length > 0 && (
                          <div>
                            <span className="text-[10px] font-semibold uppercase tracking-wider text-muted-foreground">
                              Next Steps
                            </span>
                            <ol className="mt-1.5 space-y-1.5">
                              {ls.next_steps.map((step, i) => {
                                return (
                                  <li key={i} className="flex items-center gap-2 text-xs">
                                    <span
                                      className={cn(
                                        "flex size-4 items-center justify-center rounded border",
                                        step.checked
                                          ? "border-success bg-success/10 text-success"
                                          : "border-border bg-background",
                                      )}
                                    >
                                      {step.checked && <Check className="size-2.5" />}
                                    </span>
                                    <span
                                      className={cn(
                                        "leading-relaxed",
                                        step.checked
                                          ? "text-muted-foreground line-through"
                                          : "text-foreground",
                                      )}
                                    >
                                      {step.text}
                                    </span>
                                  </li>
                                );
                              })}
                            </ol>
                          </div>
                        )}

                        {/* History summary */}
                        {ls.history.length > 0 && (
                          <div>
                            <span className="text-[10px] font-semibold uppercase tracking-wider text-muted-foreground">
                              History ({ls.history.length} completed)
                            </span>
                            <ul className="mt-1.5 space-y-1">
                              {ls.history.slice(0, 5).map((h, i) => (
                                <li
                                  key={i}
                                  className="flex items-start gap-2 text-xs text-muted-foreground"
                                >
                                  <CheckCircle2 className="mt-0.5 size-3 shrink-0 text-[var(--success)]/60" />
                                  <span className="line-clamp-1 leading-relaxed">
                                    {h.goal}
                                  </span>
                                </li>
                              ))}
                              {ls.history.length > 5 && (
                                <li className="pl-5 text-[10px] text-muted-foreground/60">
                                  +{ls.history.length - 5} more
                                </li>
                              )}
                            </ul>
                          </div>
                        )}
                      </div>
                    )}
                  </button>
                </div>
              );
            })}
          </div>
        )}
      </div>
    </div>
  );
}
