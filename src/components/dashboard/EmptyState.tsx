import { FolderOpen } from "lucide-react";

interface EmptyStateProps {
  onScan: () => void;
}

export function EmptyState({ onScan }: EmptyStateProps) {
  return (
    <div className="flex flex-1 items-center justify-center px-4 py-24">
      <div className="flex max-w-sm flex-col items-center text-center">
        <div className="mb-6 flex size-20 items-center justify-center rounded-2xl border border-dashed border-border bg-muted/30 opacity-70">
          <FolderOpen className="size-10 text-muted-foreground opacity-60" />
        </div>
        <h2 className="text-base font-semibold tracking-tight">No projects found</h2>
        <p className="mt-2 text-sm leading-relaxed text-muted-foreground">
          Scan a folder to discover repositories and create project memory. LoopDeck stores
          context inside each repository.
        </p>
        <button
          type="button"
          onClick={onScan}
          className="mt-6 inline-flex h-9 items-center gap-2 rounded-md bg-primary px-4 text-sm font-medium text-primary-foreground transition-opacity hover:opacity-90"
        >
          <FolderOpen className="size-4" />
          Scan Folder
        </button>
      </div>
    </div>
  );
}
