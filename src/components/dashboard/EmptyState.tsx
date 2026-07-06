import { FolderOpen } from "lucide-react";

interface EmptyStateProps {
  onScan: () => void;
}

export function EmptyState({ onScan }: EmptyStateProps) {
  return (
    <div className="flex flex-col items-center justify-center py-20 px-6 text-center gap-4">
      <FolderOpen size={64} className="text-muted-foreground/40" />
      <div>
        <h2 className="text-lg font-semibold text-foreground">No projects found</h2>
        <p className="text-sm text-muted-foreground max-w-sm mt-1.5 leading-relaxed">
          Scan a folder to discover repositories and create project memory.
          LoopDeck stores project context directly inside each repository.
        </p>
      </div>
      <button
        onClick={onScan}
        className="inline-flex items-center gap-2 h-9 px-5 rounded-md bg-primary text-primary-foreground text-sm font-medium hover:opacity-90 transition"
      >
        Scan Folder
      </button>
    </div>
  );
}
