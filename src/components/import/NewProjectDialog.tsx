import { useState } from "react";
import { FolderOpen, Loader2 } from "lucide-react";
import {
  Dialog,
  DialogContent,
  DialogHeader,
  DialogTitle,
  DialogDescription,
  DialogFooter,
} from "../ui/dialog";
import { Input } from "../ui/input";
import { Label } from "../ui/label";
import { useProjects } from "../../hooks/useProjects";

interface NewProjectDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
}

/**
 * "Start a new project" — the non-import path to a project. Picks a parent
 * folder on disk and a fresh project name, then creates the directory (with
 * git init + .loopdeck/ bootstrap) and lands on the new project's Epics tab.
 */
export function NewProjectDialog({ open, onOpenChange }: NewProjectDialogProps) {
  const { createProject } = useProjects();
  const [parent, setParent] = useState<string | null>(null);
  const [name, setName] = useState("");
  const [creating, setCreating] = useState(false);

  const handlePickParent = async () => {
    try {
      const { open: pick } = await import("@tauri-apps/plugin-dialog");
      const selected = await pick({
        directory: true,
        multiple: false,
        title: "Choose Parent Folder",
      });
      if (selected && typeof selected === "string") setParent(selected);
    } catch {
      // User cancelled.
    }
  };

  const handleCreate = async () => {
    if (!parent || !name.trim() || creating) return;
    setCreating(true);
    // `createProject` navigates away on success and surfaces failures via the
    // global error banner (same contract as importRepo).
    const entry = await createProject(parent, name.trim());
    if (entry) {
      setParent(null);
      setName("");
      onOpenChange(false);
    }
    setCreating(false);
  };

  const canCreate = !!parent && name.trim().length > 0 && !creating;

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="max-w-sm">
        <DialogHeader>
          <DialogTitle>Start a new project</DialogTitle>
          <DialogDescription>
            Create a fresh folder with LoopDeck project memory — no existing repo
            needed. You'll land on its Epics tab to author the first epic.
          </DialogDescription>
        </DialogHeader>

        <div className="space-y-4 py-1">
          <div className="space-y-1.5">
            <Label>Parent folder</Label>
            <div className="flex gap-2">
              <Input
                value={parent ?? ""}
                readOnly
                placeholder="Choose a parent folder…"
                className="font-mono text-[11px]"
              />
              <button
                type="button"
                onClick={handlePickParent}
                className="inline-flex h-9 shrink-0 items-center gap-1.5 rounded-md border border-border px-3 text-xs font-medium text-muted-foreground transition-colors hover:bg-accent hover:text-foreground"
              >
                <FolderOpen className="size-3.5" />
                Browse
              </button>
            </div>
          </div>

          <div className="space-y-1.5">
            <Label>Project name</Label>
            <Input
              value={name}
              onChange={(e) => setName(e.target.value)}
              disabled={!parent}
              placeholder="e.g. my-app"
              autoFocus
              onKeyDown={(e) => {
                if (e.key === "Enter") void handleCreate();
              }}
            />
            {parent && name.trim() && (
              <p className="font-mono text-[10px] text-muted-foreground">
                {parent}/{name.trim()}
              </p>
            )}
          </div>
        </div>

        <DialogFooter>
          <button
            type="button"
            onClick={() => onOpenChange(false)}
            className="inline-flex h-9 items-center rounded-md bg-muted px-3 text-xs font-medium text-muted-foreground transition-colors hover:bg-accent hover:text-foreground"
          >
            Cancel
          </button>
          <button
            type="button"
            onClick={() => void handleCreate()}
            disabled={!canCreate}
            className="inline-flex h-9 items-center gap-1.5 rounded-md bg-primary px-3 text-xs font-medium text-primary-foreground transition-opacity hover:opacity-90 disabled:opacity-50"
          >
            {creating && <Loader2 className="size-3.5 animate-spin" />}
            Create project
          </button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
