import { FolderOpen, FolderPlus } from "lucide-react";

interface EmptyStateProps {
  onScan: () => void;
  onNewProject?: () => void;
}

export function EmptyState({ onScan, onNewProject }: EmptyStateProps) {
  return (
    <div className="flex flex-1 items-center justify-center px-4 py-24">
      <div className="flex max-w-sm flex-col items-center text-center">
        <div className="mb-6 flex size-20 items-center justify-center rounded-2xl border border-dashed border-border bg-muted/30 opacity-70">
          <FolderOpen className="size-10 text-muted-foreground opacity-60" />
        </div>
        <h2 className="text-base font-semibold tracking-tight">No projects found</h2>
        <p className="mt-2 text-sm leading-relaxed text-muted-foreground">
          Scan a folder to discover repositories and create project memory, or start a
          brand-new project from scratch. LoopDeck stores context inside each repository.
        </p>
        <div className="mt-6 flex items-center gap-2">
          <button
            type="button"
            onClick={onScan}
            className="inline-flex h-9 items-center gap-2 rounded-md bg-primary px-4 text-sm font-medium text-primary-foreground transition-opacity hover:opacity-90"
          >
            <FolderOpen className="size-4" />
            Scan Folder
          </button>
          {onNewProject && (
            <button
              type="button"
              onClick={onNewProject}
              className="inline-flex h-9 items-center gap-2 rounded-md border border-border bg-background px-4 text-sm font-medium text-foreground transition-colors hover:bg-accent"
            >
              <FolderPlus className="size-4" />
              New Project
            </button>
          )}
        </div>
      </div>
    </div>
  );
}
