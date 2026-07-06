import { useState, useEffect, useMemo } from "react";
import { Lightbulb, Loader2, AlertTriangle, Search, Filter, ChevronDown, GitBranch } from "lucide-react";
import type { Decision } from "../../types";
import * as api from "../../lib/tauri";
import { useAppStore } from "../../store/appStore";
import { PageHeader } from "../layout/AppShell";

// ── Types ────────────────────────────────────────────────────────────────────

interface EnrichedDecision extends Decision {
  projectName: string;
  projectPath: string;
}

// ── Helpers ──────────────────────────────────────────────────────────────────

const STATUS_COLOR: Record<Decision["status"], string> = {
  proposed: "text-[var(--warning)]",
  accepted: "text-[var(--success)]",
  superseded: "text-muted-foreground",
};

const STATUS_BG: Record<Decision["status"], string> = {
  proposed: "bg-[color-mix(in_oklab,var(--warning)_15%,transparent)] border-[color-mix(in_oklab,var(--warning)_30%,transparent)]",
  accepted: "bg-[color-mix(in_oklab,var(--success)_15%,transparent)] border-[color-mix(in_oklab,var(--success)_30%,transparent)]",
  superseded: "bg-muted/50 border-border",
};

// ── Component ────────────────────────────────────────────────────────────────

export function DecisionsView() {
  const projects = useAppStore((s) => s.projects);

  // ── Data ──
  const [decisions, setDecisions] = useState<EnrichedDecision[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);

  // ── Filters ──
  const [search, setSearch] = useState("");
  const [statusFilter, setStatusFilter] = useState<Decision["status"] | "all">("all");
  const [projectFilter, setProjectFilter] = useState<string | "all">("all");

  // ── Expanded detail ──
  const [expandedIdx, setExpandedIdx] = useState<number | null>(null);

  // ── Load all decisions from all projects ────────────────────────────────

  useEffect(() => {
    let cancelled = false;

    async function load() {
      setLoading(true);
      setError(null);

      const all: EnrichedDecision[] = [];

      for (const p of projects) {
        if (cancelled) break;
        try {
          const ds = await api.getDecisions(p.path);
          all.push(
            ...ds.map((d) => ({
              ...d,
              projectName: p.name,
              projectPath: p.path,
            })),
          );
        } catch {
          // No decisions file — skip.
        }
      }

      if (!cancelled) {
        // Sort by date descending.
        all.sort((a, b) => b.date.localeCompare(a.date));
        setDecisions(all);
        setLoading(false);
      }
    }

    if (projects.length > 0) {
      load();
    } else {
      setLoading(false);
      setDecisions([]);
    }

    return () => { cancelled = true; };
  }, [projects]);

  // ── Derived: unique projects that have decisions ────────────────────────

  const decisionProjects = useMemo(() => {
    const seen = new Set<string>();
    const list: { name: string; path: string }[] = [];
    for (const d of decisions) {
      if (!seen.has(d.projectPath)) {
        seen.add(d.projectPath);
        list.push({ name: d.projectName, path: d.projectPath });
      }
    }
    return list;
  }, [decisions]);

  // ── Filtered decisions ──────────────────────────────────────────────────

  const filtered = useMemo(() => {
    let result = decisions;

    if (statusFilter !== "all") {
      result = result.filter((d) => d.status === statusFilter);
    }
    if (projectFilter !== "all") {
      result = result.filter((d) => d.projectPath === projectFilter);
    }
    if (search.trim()) {
      const q = search.toLowerCase();
      result = result.filter(
        (d) =>
          d.title.toLowerCase().includes(q) ||
          d.context.toLowerCase().includes(q) ||
          (d.consequences ?? "").toLowerCase().includes(q) ||
          d.projectName.toLowerCase().includes(q),
      );
    }

    return result;
  }, [decisions, statusFilter, projectFilter, search]);

  // ── Group by month ──────────────────────────────────────────────────────

  const grouped = useMemo(() => {
    const map = new Map<string, EnrichedDecision[]>();
    for (const d of filtered) {
      const month = new Date(d.date + "T00:00:00").toLocaleDateString("en-US", {
        year: "numeric",
        month: "long",
      });
      const bucket = map.get(month);
      if (bucket) bucket.push(d);
      else map.set(month, [d]);
    }
    return [...map.entries()];
  }, [filtered]);

  const hasData = decisions.length > 0;
  const hasFiltered = filtered.length > 0;

  // ── Render ─────────────────────────────────────────────────────────────

  return (
    <div className="flex-1 flex flex-col min-h-0">
      {/* Header */}
      <PageHeader
        title="Decisions"
        subtitle="Architectural notes captured across projects"
        actions={
          !loading && hasData ? (
            <span className="text-[11px] text-muted-foreground">
              {decisions.length} decision{decisions.length !== 1 ? "s" : ""}
              {filtered.length !== decisions.length && ` · ${filtered.length} shown`}
            </span>
          ) : undefined
        }
      />

      {/* ── Filters ── */}
      {hasData && !loading && (
        <div className="shrink-0 px-6 py-2.5 border-b border-border/50 bg-surface/30 flex items-center gap-3 flex-wrap">
          {/* Search */}
          <div className="flex items-center gap-1.5 flex-1 min-w-[200px] max-w-sm">
            <Search className="size-3.5 text-muted-foreground shrink-0" />
            <input
              type="text"
              value={search}
              onChange={(e) => setSearch(e.target.value)}
              placeholder="Search decisions…"
              className="flex-1 bg-transparent border-none outline-none text-xs text-foreground placeholder:text-muted-foreground/50"
            />
          </div>

          {/* Status filter */}
          <div className="flex items-center gap-1">
            <Filter className="size-3 text-muted-foreground" />
            {(["all", "accepted", "proposed", "superseded"] as const).map((s) => (
              <button
                key={s}
                onClick={() => setStatusFilter(s)}
                className={`px-2 py-0.5 rounded text-[10px] font-medium transition ${
                  statusFilter === s
                    ? "bg-accent text-foreground"
                    : "text-muted-foreground hover:text-foreground"
                }`}
              >
                {s === "all" ? "All" : s}
              </button>
            ))}
          </div>

          {/* Project filter */}
          <select
            value={projectFilter}
            onChange={(e) => setProjectFilter(e.target.value)}
            className="bg-transparent border border-border rounded px-2 py-0.5 text-[10px] text-muted-foreground focus:outline-none focus:ring-1 focus:ring-primary"
          >
            <option value="all">All projects</option>
            {decisionProjects.map((p) => (
              <option key={p.path} value={p.path}>
                {p.name}
              </option>
            ))}
          </select>
        </div>
      )}

      {/* ── Body ── */}
      <div className="flex-1 min-h-0 overflow-y-auto">
        {/* Loading */}
        {loading && (
          <div className="flex flex-col items-center justify-center py-20 gap-4 text-muted-foreground">
            <Loader2 className="size-8 animate-spin" />
            <span className="text-sm">Loading decisions…</span>
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

        {/* Empty: no decisions at all */}
        {!loading && !error && !hasData && (
          <div className="flex flex-col items-center justify-center py-20 text-center">
            <Lightbulb size={40} className="text-muted-foreground/20 mb-4" />
            <h3 className="text-sm font-semibold text-foreground mb-1.5">
              No decisions yet
            </h3>
            <p className="text-xs text-muted-foreground max-w-xs leading-relaxed">
              Architectural decisions from{" "}
              <code className="font-mono text-[11px] bg-muted px-1 py-0.5 rounded">
                .loopdeck/decisions.md
              </code>{" "}
              files will appear here. Start an agent conversation to log your
              first decision.
            </p>
          </div>
        )}

        {/* Empty: filtered to nothing */}
        {!loading && !error && hasData && !hasFiltered && (
          <div className="flex flex-col items-center justify-center py-20 text-center text-muted-foreground">
            <Search size={32} className="opacity-20 mb-3" />
            <p className="text-sm">No decisions match your filters.</p>
            <button
              onClick={() => {
                setSearch("");
                setStatusFilter("all");
                setProjectFilter("all");
              }}
              className="text-xs text-[var(--primary)] hover:underline mt-1"
            >
              Clear filters
            </button>
          </div>
        )}

        {/* Decision list grouped by month */}
        {!loading && !error && hasFiltered && (
          <div className="mx-auto w-full max-w-3xl space-y-6 px-8 py-8">
            {grouped.map(([month, ds]) => (
              <section key={month}>
                <h2 className="mb-3 text-[10px] font-semibold uppercase tracking-wider text-muted-foreground">
                  {month}
                </h2>

                <div className="space-y-3">
                  {ds.map((d, i) => {
                    const isExpanded = expandedIdx === i;
                    return (
                      <div
                        key={`${d.date}-${d.title}-${i}`}
                        className={`rounded-xl border bg-card p-5 shadow-[var(--shadow-sm)] transition-colors ${
                          STATUS_BG[d.status]
                        } ${isExpanded ? "ring-1 ring-border" : ""}`}
                      >
                        <button
                          onClick={() =>
                            setExpandedIdx(isExpanded ? null : i)
                          }
                          className="w-full text-left"
                        >
                          <div className="flex items-start justify-between gap-3">
                            <div className="min-w-0 flex-1">
                              {/* Project chip + date */}
                              <div className="flex items-center gap-2 text-[10px] font-semibold uppercase tracking-wider text-muted-foreground">
                                <GitBranch className="size-3" />
                                {d.projectName}
                                <span className="font-normal text-muted-foreground/60">
                                  · {d.date}
                                </span>
                              </div>

                              {/* Title */}
                              <h3 className="mt-2 text-sm font-semibold tracking-tight text-foreground leading-snug">
                                {d.title}
                              </h3>

                              {/* Context preview (collapsed) */}
                              {!isExpanded && (
                                <p className="mt-1.5 text-xs leading-relaxed text-muted-foreground line-clamp-2">
                                  {d.context}
                                </p>
                              )}
                            </div>

                            {/* Status badge + chevron */}
                            <div className="flex shrink-0 items-center gap-2">
                              <span
                                className={`inline-flex items-center rounded px-1.5 py-0.5 text-[10px] font-medium uppercase tracking-wider ${
                                  STATUS_COLOR[d.status]
                                }`}
                              >
                                {d.status}
                              </span>
                              <ChevronDown
                                className={`size-3.5 text-muted-foreground transition-transform ${
                                  isExpanded ? "rotate-180" : ""
                                }`}
                              />
                            </div>
                          </div>

                          {/* Expanded body */}
                          {isExpanded && (
                            <div className="mt-3 space-y-2.5 border-t border-border/50 pt-3 text-xs leading-relaxed">
                              <div>
                                <span className="font-semibold text-foreground">
                                  Context
                                </span>
                                <p className="mt-0.5 text-muted-foreground">
                                  {d.context}
                                </p>
                              </div>
                              {d.consequences && (
                                <div>
                                  <span className="font-semibold text-foreground">
                                    Consequences
                                  </span>
                                  <p className="mt-0.5 whitespace-pre-wrap text-muted-foreground">
                                    {d.consequences}
                                  </p>
                                </div>
                              )}
                            </div>
                          )}
                        </button>
                      </div>
                    );
                  })}
                </div>
              </section>
            ))}
          </div>
        )}
      </div>
    </div>
  );
}
