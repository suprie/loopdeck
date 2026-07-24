import { useState, useEffect } from "react";
import { Network, ExternalLink, FileText, FolderOpen, RefreshCw } from "lucide-react";
import type { GraphifyStats } from "../../types";
import * as api from "../../lib/tauri";
import { LoadingSpinner } from "../shared/LoadingSpinner";

interface KnowledgeGraphPanelProps {
  projectPath: string;
}

/** Open a path with the OS default handler (browser for .html, viewer for
 *  .md, etc.). Mirrors the pattern in `Markdown.tsx` — dynamic-import the
 *  shell plugin so it doesn't dominate first paint, and fall back to a
 *  no-op window.open if the plugin is unavailable. */
async function openWithOS(target: string) {
  try {
    const m = await import("@tauri-apps/plugin-shell");
    await m.open(target);
  } catch {
    window.open(target, "_blank", "noopener,noreferrer");
  }
}

export function KnowledgeGraphPanel({ projectPath }: KnowledgeGraphPanelProps) {
  const [stats, setStats] = useState<GraphifyStats | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let cancelled = false;
    async function load() {
      try {
        const data = await api.getGraphifyStats(projectPath);
        if (!cancelled) {
          setStats(data);
          setError(null);
        }
      } catch (err) {
        if (!cancelled) {
          setError(String(err));
        }
      } finally {
        if (!cancelled) setLoading(false);
      }
    }
    load();
    return () => {
      cancelled = true;
    };
  }, [projectPath]);

  // Re-fetch on demand. The Graph tab is the only place a user would look
  // right after running `graphify .` in a terminal, so a manual refresh
  // button matters more here than in the other panels.
  const refresh = async () => {
    setLoading(true);
    try {
      const data = await api.getGraphifyStats(projectPath);
      setStats(data);
      setError(null);
    } catch (err) {
      setError(String(err));
    } finally {
      setLoading(false);
    }
  };

  if (loading) {
    return <LoadingSpinner label="Checking for knowledge graph..." />;
  }

  if (error) {
    return (
      <div className="text-destructive text-sm p-3">
        Failed to read knowledge graph: {error}
      </div>
    );
  }

  // No graph yet — informational empty state, not an error. The user simply
  // hasn't run Graphify in this repo. Don't oversell; just point them at it.
  if (!stats || !stats.present) {
    return (
      <div className="max-w-2xl">
        <div className="flex flex-col items-center justify-center py-16 text-center">
          <Network size={32} className="text-muted-foreground/30 mb-3" />
          <h3 className="text-sm font-semibold text-foreground mb-1.5">
            No knowledge graph detected
          </h3>
          <p className="text-xs text-muted-foreground max-w-sm leading-relaxed mb-4">
            <a
              href="https://github.com/Graphify-Labs/graphify"
              target="_blank"
              rel="noopener noreferrer"
              className="text-primary underline decoration-primary/30 underline-offset-2 hover:decoration-primary/70"
            >
              Graphify
            </a>{" "}
            turns a codebase into a queryable knowledge graph. Run it in this
            repo and the stats will appear here — LoopDeck only reads the
            output, it doesn&apos;t install or run Graphify for you.
          </p>
          <code className="font-mono text-[11px] bg-muted px-2 py-1 rounded text-muted-foreground">
            pip install graphifyy &amp;&amp; graphify .
          </code>
        </div>
      </div>
    );
  }

  const confidenceTotal =
    stats.confidence.extracted + stats.confidence.inferred + stats.confidence.ambiguous;

  return (
    <div className="max-w-2xl">
      <div className="flex items-center justify-between mb-4">
        <div className="text-xs text-muted-foreground">
          {stats.built_at ? `Built ${stats.built_at}` : "Build date unknown"}
        </div>
        <button
          type="button"
          onClick={refresh}
          className="inline-flex items-center gap-1.5 rounded-md px-2 py-1 text-xs text-muted-foreground transition-colors hover:bg-accent hover:text-foreground"
        >
          <RefreshCw size={12} />
          Refresh
        </button>
      </div>

      {/* Stats grid */}
      <div className="grid grid-cols-3 gap-3 mb-5">
        <StatCard label="Nodes" value={stats.node_count} />
        <StatCard label="Edges" value={stats.edge_count} />
        <StatCard label="Communities" value={stats.community_count} />
      </div>

      {/* Confidence breakdown */}
      {confidenceTotal > 0 && (
        <div className="rounded-xl border border-border bg-card p-4 mb-5">
          <div className="text-[10px] font-semibold uppercase tracking-wider text-muted-foreground mb-3">
            Link confidence
          </div>
          <div className="flex h-2 overflow-hidden rounded-full bg-muted">
            <ConfidenceBar
              value={stats.confidence.extracted}
              total={confidenceTotal}
              color="var(--success)"
            />
            <ConfidenceBar
              value={stats.confidence.inferred}
              total={confidenceTotal}
              color="var(--warning)"
            />
            <ConfidenceBar
              value={stats.confidence.ambiguous}
              total={confidenceTotal}
              color="var(--destructive)"
            />
          </div>
          <div className="mt-3 flex flex-wrap gap-x-4 gap-y-1 text-[11px]">
            <LegendDot
              color="var(--success)"
              label="Extracted"
              value={stats.confidence.extracted}
            />
            <LegendDot
              color="var(--warning)"
              label="Inferred"
              value={stats.confidence.inferred}
            />
            <LegendDot
              color="var(--destructive)"
              label="Ambiguous"
              value={stats.confidence.ambiguous}
            />
          </div>
        </div>
      )}

      {/* God nodes */}
      {stats.god_nodes.length > 0 && (
        <div className="rounded-xl border border-border bg-card p-4 mb-5">
          <div className="text-[10px] font-semibold uppercase tracking-wider text-muted-foreground mb-3">
            God nodes (most connected)
          </div>
          <div className="flex flex-wrap gap-1.5">
            {stats.god_nodes.map((label, i) => (
              <span
                key={`${label}-${i}`}
                className="inline-flex items-center gap-1 rounded-md bg-muted px-2 py-1 text-[11px] font-mono text-foreground/80"
              >
                <span className="text-muted-foreground/60">{i + 1}.</span>
                {label}
              </span>
            ))}
          </div>
        </div>
      )}

      {/* Open affordances */}
      <div className="flex flex-wrap gap-2">
        {stats.html_path && (
          <PanelButton
            icon={ExternalLink}
            label="Open graph.html"
            onClick={() => void openWithOS(stats.html_path!)}
          />
        )}
        {stats.report_path && (
          <PanelButton
            icon={FileText}
            label="Open report"
            onClick={() => void openWithOS(stats.report_path!)}
          />
        )}
        <PanelButton
          icon={FolderOpen}
          label="Reveal output"
          onClick={() => void api.openInFinder(stats.graph_path)}
        />
      </div>
    </div>
  );
}

function StatCard({ label, value }: { label: string; value: number }) {
  return (
    <div className="rounded-xl border border-border bg-card p-4">
      <div className="text-2xl font-semibold text-foreground tabular-nums">
        {value.toLocaleString()}
      </div>
      <div className="text-[10px] font-semibold uppercase tracking-wider text-muted-foreground mt-1">
        {label}
      </div>
    </div>
  );
}

function ConfidenceBar({
  value,
  total,
  color,
}: {
  value: number;
  total: number;
  color: string;
}) {
  if (total === 0) return null;
  const pct = (value / total) * 100;
  if (pct === 0) return null;
  return (
    <div
      style={{ width: `${pct}%`, backgroundColor: color }}
      title={`${value} link${value === 1 ? "" : "s"}`}
    />
  );
}

function LegendDot({
  color,
  label,
  value,
}: {
  color: string;
  label: string;
  value: number;
}) {
  return (
    <span className="inline-flex items-center gap-1.5 text-muted-foreground">
      <span
        className="inline-block size-2 rounded-full"
        style={{ backgroundColor: color }}
      />
      {label} · {value}
    </span>
  );
}

function PanelButton({
  icon: Icon,
  label,
  onClick,
}: {
  icon: React.ComponentType<{ className?: string }>;
  label: string;
  onClick?: () => void;
}) {
  return (
    <button
      type="button"
      onClick={onClick}
      className="inline-flex h-8 items-center gap-1.5 rounded-md px-3 text-xs font-medium text-muted-foreground transition-colors hover:bg-accent hover:text-foreground"
    >
      <Icon className="size-3.5" />
      {label}
    </button>
  );
}
