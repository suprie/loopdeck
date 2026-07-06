import { FolderOpen } from "lucide-react";
import "./EmptyState.css";

interface EmptyStateProps {
  onScan: () => void;
}

export function EmptyState({ onScan }: EmptyStateProps) {
  return (
    <div className="empty-state">
      <FolderOpen size={64} className="empty-state__icon" />
      <h2>No projects found</h2>
      <p>
        Scan a folder to discover repositories and create project memory.
        LoopDeck stores project context directly inside each repository.
      </p>
      <button className="btn-primary btn-lg" onClick={onScan}>
        Scan Folder
      </button>
    </div>
  );
}
