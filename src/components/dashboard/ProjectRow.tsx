import { memo } from "react";
import { Folder, Terminal, Trash2, Info, GitCommit, RefreshCw, Clock } from "lucide-react";
import type { ProjectEntry } from "../../types";
import { relativeTime } from "../../lib/time";
import "./ProjectRow.css";

interface ProjectRowProps {
  project: ProjectEntry;
  onSelect: (project: ProjectEntry) => void;
  onOpenInFinder: (path: string) => void;
  onOpenInTerminal: (path: string) => void;
  onRemove: (path: string) => void;
  onRescan: (path: string) => void;
}

export const ProjectRow = memo(function ProjectRow({
  project,
  onSelect,
  onOpenInFinder,
  onOpenInTerminal,
  onRemove,
  onRescan,
}: ProjectRowProps) {
  return (
    <div className="project-row">
      <div className="project-row__info">
        <h3 className="project-row__name">{project.name}</h3>
        {project.description && (
          <p className="project-row__description">{project.description}</p>
        )}
        <div className="project-row__meta">
          <span className="project-row__git">
            <GitCommit size={12} />
            {relativeTime(project.last_commit)}
          </span>
          {project.last_opened && (
            <span className="project-row__opened">
              <Clock size={12} />
              Opened {relativeTime(project.last_opened)}
            </span>
          )}
          <span>{project.status}</span>
        </div>
      </div>
      <div className="project-row__actions">
        <button
          className="btn-secondary"
          onClick={() => onSelect(project)}
          title="View details"
        >
          <Info size={14} />
        </button>
        <button
          className="btn-secondary"
          onClick={() => onRescan(project.path)}
          title="Rescan project"
        >
          <RefreshCw size={14} />
        </button>
        <button
          className="btn-secondary"
          onClick={() => onOpenInFinder(project.path)}
          title="Open in Finder"
        >
          <Folder size={14} />
        </button>
        <button
          className="btn-secondary"
          onClick={() => onOpenInTerminal(project.path)}
          title="Open in Terminal"
        >
          <Terminal size={14} />
        </button>
        <button
          className="btn-danger"
          onClick={() => onRemove(project.path)}
          title="Remove from registry"
        >
          <Trash2 size={14} />
        </button>
      </div>
    </div>
  );
});
