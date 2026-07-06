import { memo } from "react";
import { Download, Check, GitCommit, Layers } from "lucide-react";
import type { DiscoveredRepo } from "../../types";
import { relativeTime } from "../../lib/time";
import "./RepoCard.css";

interface RepoCardProps {
  repo: DiscoveredRepo;
  onImport: (path: string) => void;
}

export const RepoCard = memo(function RepoCard({ repo, onImport }: RepoCardProps) {
  return (
    <div className={`repo-card ${repo.has_loopdeck ? "repo-card--imported" : ""}`}>
      <div className="repo-card__info">
        <h3 className="repo-card__name">
          {repo.name}
          {repo.has_loopdeck && <span className="repo-card__badge">Imported</span>}
        </h3>
        <span className="repo-card__path">{repo.path}</span>
        {repo.description_preview && (
          <p className="repo-card__preview">{repo.description_preview}</p>
        )}
        <div className="repo-card__tags">
          <span className="repo-card__tag repo-card__tag--git">
            <GitCommit size={11} /> {relativeTime(repo.last_commit)}
          </span>
          <span className="repo-card__tag repo-card__tag--stack">
            <Layers size={11} /> {repo.detected_stack}
          </span>
          {repo.has_readme && (
            <span className="repo-card__tag">README</span>
          )}
        </div>
      </div>
      <div className="repo-card__actions">
        {repo.has_loopdeck ? (
          <button className="btn-secondary" disabled>
            <Check size={14} /> Imported
          </button>
        ) : (
          <button
            className="btn-primary"
            onClick={() => onImport(repo.path)}
          >
            <Download size={14} /> Import
          </button>
        )}
      </div>
    </div>
  );
});
