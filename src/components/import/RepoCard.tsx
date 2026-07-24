import { memo } from "react";
import { Download, Check, GitCommit, Layers, Network } from "lucide-react";
import type { DiscoveredRepo } from "../../types";
import { relativeTime } from "../../lib/time";

interface RepoCardProps {
  repo: DiscoveredRepo;
  onImport: (path: string) => void;
}

export const RepoCard = memo(function RepoCard({ repo, onImport }: RepoCardProps) {
  return (
    <div
      className={`flex items-start justify-between p-5 rounded-xl border border-border transition-all gap-4 ${
        repo.has_loopdeck
          ? "opacity-50 border-border bg-card"
          : "bg-card hover:border-primary/40 hover:bg-surface-elevated"
      }`}
    >
      <div className="flex-1 min-w-0">
        <h3 className="text-sm font-semibold text-foreground flex items-center gap-2 mb-1">
          {repo.name}
          {repo.has_loopdeck && (
            <span className="text-[10px] px-1.5 py-0.5 rounded bg-[color-mix(in_oklab,var(--success)_15%,transparent)] text-[var(--success)] font-medium">
              Imported
            </span>
          )}
        </h3>
        <span className="text-xs text-muted-foreground font-mono block mb-2 truncate">
          {repo.path}
        </span>
        {repo.description_preview && (
          <p className="text-xs text-foreground/80 mb-3 line-clamp-2 leading-relaxed">
            {repo.description_preview}
          </p>
        )}
        <div className="flex gap-1.5 flex-wrap">
          <span className="text-[10px] px-1.5 py-0.5 rounded bg-muted text-muted-foreground font-mono inline-flex items-center gap-1">
            <GitCommit className="size-2.5" />
            {relativeTime(repo.last_commit)}
          </span>
          <span className="text-[10px] px-1.5 py-0.5 rounded bg-[color-mix(in_oklab,var(--primary)_12%,transparent)] text-[var(--primary)] font-mono inline-flex items-center gap-1">
            <Layers className="size-2.5" />
            {repo.detected_stack}
          </span>
          {repo.has_readme && (
            <span className="text-[10px] px-1.5 py-0.5 rounded bg-muted text-muted-foreground font-mono">
              README
            </span>
          )}
          {repo.has_graphify && (
            <span
              title="graphify-out/graph.json detected"
              className="text-[10px] px-1.5 py-0.5 rounded bg-[color-mix(in_oklab,var(--primary)_12%,transparent)] text-[var(--primary)] font-mono inline-flex items-center gap-1"
            >
              <Network className="size-2.5" />
              Graph
            </span>
          )}
        </div>
      </div>
      <div className="shrink-0">
        {repo.has_loopdeck ? (
          <button
            disabled
            className="inline-flex items-center gap-1.5 h-8 px-3 rounded-md bg-muted/50 text-muted-foreground text-xs font-medium cursor-not-allowed"
          >
            <Check className="size-3.5" />
            Imported
          </button>
        ) : (
          <button
            onClick={() => onImport(repo.path)}
            className="inline-flex items-center gap-1.5 h-8 px-3 rounded-md bg-primary text-primary-foreground text-xs font-medium hover:opacity-90 transition"
          >
            <Download className="size-3.5" />
            Import
          </button>
        )}
      </div>
    </div>
  );
});
