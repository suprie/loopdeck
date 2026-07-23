import { useState, useEffect, useCallback } from "react";
import { Layers, Zap, CheckCircle2, Circle, Loader2, Pencil, ChevronRight } from "lucide-react";
import { toast } from "sonner";
import type { Epic, PrdLoop, LoopStatus, AppError } from "../../types";
import * as api from "../../lib/tauri";
import { LoadingSpinner } from "../shared/LoadingSpinner";
import { Tabs, TabsList, TabsTrigger, TabsContent } from "../ui/tabs";
import { SpecEditor } from "./SpecEditor";
import { PrdContent } from "./PrdContent";

interface EpicsPanelProps {
  projectPath: string;
}

export function EpicsPanel({ projectPath }: EpicsPanelProps) {
  const [epics, setEpics] = useState<Epic[]>([]);
  const [loopStatus, setLoopStatus] = useState<LoopStatus | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [promoting, setPromoting] = useState<string | null>(null);
  const [toggling, setToggling] = useState<string | null>(null);
  /** Relative path of the spec file being edited, or null. */
  const [editingSpec, setEditingSpec] = useState<string | null>(null);
  /** Relative path of the PRD expanded into Content/Checklist tabs, or null. */
  const [expandedSpec, setExpandedSpec] = useState<string | null>(null);

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
  }, [projectPath]);

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
    return () => {
      cancelled = true;
    };
  }, [projectPath]);

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
    } catch (err) {
      toast.error("Failed to toggle", { description: String(err) });
    } finally {
      setToggling(null);
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

      <div className="text-xs text-muted-foreground mb-4">
        {epics.length} epic{epics.length !== 1 ? "s" : ""}
      </div>

      <div className="space-y-4">
        {epics.map((epic) => (
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
                onClick={() => setEditingSpec(`${epic.slug}/README.md`)}
                title="Edit epic README"
                className="flex size-6 shrink-0 items-center justify-center rounded-md text-muted-foreground transition-colors hover:bg-accent hover:text-foreground"
              >
                <Pencil className="size-3.5" />
              </button>
            </div>

            {/* Epic README editor */}
            {editingSpec === `${epic.slug}/README.md` && (
              <div className="mb-4">
                <SpecEditor
                  projectPath={projectPath}
                  relPath={`${epic.slug}/README.md`}
                  filename="README.md"
                  onSaved={() => {
                    setEditingSpec(null);
                    load();
                  }}
                  onCancel={() => setEditingSpec(null)}
                />
              </div>
            )}

            {/* PRDs */}
            <div className="space-y-3">
              {epic.prds.map((prd) => {
                const prdKey = `${epic.slug}/${prd.file}`;
                const isEditing = editingSpec === prdKey;
                const isExpanded = !isEditing && expandedSpec === prdKey;
                const loopCount = prd.phases.reduce((n, p) => n + p.loops.length, 0);
                const prdIsActive =
                  hasActiveLoop &&
                  prd.phases.some((p) => p.loops.some((l) => l.title === currentGoal));
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
                        {loopCount} loop{loopCount !== 1 ? "s" : ""}
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
                        onClick={() => setEditingSpec(prdKey)}
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

                  {isEditing ? (
                    <div className="mt-2">
                      <SpecEditor
                        projectPath={projectPath}
                        relPath={prdKey}
                        filename={prd.file}
                        onSaved={() => {
                          setEditingSpec(null);
                          load();
                        }}
                        onCancel={() => setEditingSpec(null)}
                      />
                    </div>
                  ) : isExpanded ? (
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

                                      {/* Promote action — only on not-done loops */}
                                      {!done && (
                                        <button
                                          onClick={() => handlePromote(epic.slug, prd.file, loop)}
                                          disabled={disabled || isPromoting}
                                          title={
                                            isCurrent
                                              ? "Currently in progress"
                                              : disabled
                                                ? "Another loop is in progress"
                                                : "Promote to current loop"
                                          }
                                          className={`shrink-0 rounded px-1.5 py-0.5 text-[10px] font-medium transition-colors ${
                                            isCurrent
                                              ? "text-[var(--primary)]"
                                              : disabled
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
        ))}
      </div>
    </div>
  );
}
