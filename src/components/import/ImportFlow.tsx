import { useEffect, useState } from "react";
import { useNavigate } from "@tanstack/react-router";
import { FolderOpen, FolderPlus, Loader2 } from "lucide-react";
import { useAppStore } from "../../store/appStore";
import { useProjects } from "../../hooks/useProjects";
import { RepoCard } from "./RepoCard";
import { LoadingSpinner } from "../shared/LoadingSpinner";
import { PageHeader } from "../layout/AppShell";
import { cn } from "../../lib/utils";

export function ImportFlow() {
  const navigate = useNavigate();
  const discoveredRepos = useAppStore((s) => s.discoveredRepos);
  const isScanning = useAppStore((s) => s.isScanning);
  const isLoading = useAppStore((s) => s.isLoading);
  const { importRepo, scanFolder } = useProjects();

  /** Drag-and-drop hover state, driven by Tauri's window drag-drop events.
   *  Falls back to no-op outside Tauri (e.g. plain browser dev). */
  const [dragOver, setDragOver] = useState(false);

  // Subscribe to Tauri window drag-drop events. On a `drop`, treat the first
  // dropped path as a folder to scan. `over`/`leave` toggle the highlight.
  useEffect(() => {
    let unlisten: (() => void) | undefined;
    (async () => {
      try {
        const { getCurrentWindow } = await import("@tauri-apps/api/window");
        unlisten = await getCurrentWindow().onDragDropEvent((event) => {
          const payload = event.payload as
            | { type: "enter" | "over"; paths: string[] }
            | { type: "drop"; paths: string[] }
            | { type: "leave" };
          if (payload.type === "drop") {
            setDragOver(false);
            const path = payload.paths[0];
            if (path) void scanFolder(path);
          } else if (payload.type === "enter" || payload.type === "over") {
            setDragOver(true);
          } else if (payload.type === "leave") {
            setDragOver(false);
          }
        });
      } catch {
        // Not in Tauri (browser dev) — drag-drop unsupported, no-op.
      }
    })();
    return () => {
      unlisten?.();
    };
  }, [scanFolder]);

  const handleImport = async (path: string) => {
    await importRepo(path);
  };

  const handleScanClick = async () => {
    try {
      const { open } = await import("@tauri-apps/plugin-dialog");
      const selected = await open({
        directory: true,
        multiple: false,
        title: "Select Folder to Scan",
      });
      if (selected && typeof selected === "string") {
        await scanFolder(selected);
      }
    } catch {
      // User cancelled
    }
  };

  if (isScanning) {
    return (
      <div className="flex flex-1 items-center justify-center">
        <LoadingSpinner label="Scanning for repositories..." />
      </div>
    );
  }

  return (
    <div className="flex flex-1 flex-col min-h-0">
      <PageHeader
        title="Import Repo"
        subtitle={
          discoveredRepos.length > 0
            ? `Found ${discoveredRepos.length} repo${discoveredRepos.length !== 1 ? "s" : ""}`
            : "Scan a folder to discover repositories"
        }
        actions={
          <button
            type="button"
            onClick={() => navigate({ to: "/" })}
            className="inline-flex h-8 items-center gap-1.5 rounded-md border border-border bg-transparent px-3 text-xs font-medium text-muted-foreground transition-colors hover:bg-accent hover:text-foreground"
          >
            Back
          </button>
        }
      />

      <div className="flex-1 min-h-0 overflow-y-auto px-8 py-8">
        {/* ── Hero: single scan-folder card ── */}
        <div className="mx-auto grid w-full max-w-3xl grid-cols-1 gap-4 md:grid-cols-2">
          <ImportCard
            icon={FolderOpen}
            title="Scan local folder"
            body="Pick a folder on disk. Selasar discovers repositories and writes project memory in .loopdeck/."
            cta="Choose folder"
            onClick={handleScanClick}
          />
          {/* Second tile intentionally informational — "Clone remote repo"
              was descoped for this redesign. Leaves the grid balanced. */}
          <div className="rounded-xl border border-dashed border-border bg-muted/20 p-5 opacity-60">
            <div className="flex size-9 items-center justify-center rounded-md bg-accent text-foreground">
              <FolderPlus className="size-4" />
            </div>
            <h3 className="mt-3 text-sm font-semibold tracking-tight">
              More import options soon
            </h3>
            <p className="mt-1 text-xs leading-relaxed text-muted-foreground">
              Drag a folder onto this window, or use the scan card to pick one.
            </p>
          </div>
        </div>

        {/* ── Drag-and-drop dropzone ── */}
        <div className="mx-auto w-full max-w-3xl px-0 pb-6 pt-4">
          <div
            className={cn(
              "rounded-xl border border-dashed p-10 text-center transition-colors",
              dragOver
                ? "border-primary bg-primary/5"
                : "border-border bg-muted/20",
            )}
          >
            <FolderPlus
              className={cn(
                "mx-auto size-8 transition-colors",
                dragOver ? "text-primary" : "text-muted-foreground opacity-60",
              )}
            />
            <p className="mt-3 text-xs text-muted-foreground">
              {dragOver ? "Release to scan this folder" : "Or drag a folder here to import"}
            </p>
          </div>
        </div>

        {/* ── Discovered repos ── */}
        {discoveredRepos.length > 0 && (
          <div className="mx-auto w-full max-w-3xl space-y-3">
            <h2 className="text-[10px] font-semibold uppercase tracking-wider text-muted-foreground">
              Discovered repositories
            </h2>
            {discoveredRepos.map((repo) => (
              <RepoCard key={repo.path} repo={repo} onImport={handleImport} />
            ))}
          </div>
        )}

        {discoveredRepos.length === 0 && !isLoading && (
          <div className="mx-auto w-full max-w-3xl pt-6 text-center">
            <p className="text-xs text-muted-foreground">
              No repositories discovered yet. Pick a folder above to start.
            </p>
          </div>
        )}

        {isLoading && (
          <div className="mt-6 flex items-center justify-center gap-2 text-xs text-muted-foreground">
            <Loader2 className="size-3.5 animate-spin" />
            Importing project…
          </div>
        )}
      </div>
    </div>
  );
}

// ── ImportCard ───────────────────────────────────────────────────────────────

function ImportCard({
  icon: Icon,
  title,
  body,
  cta,
  onClick,
}: {
  icon: React.ComponentType<{ className?: string }>;
  title: string;
  body: string;
  cta: string;
  onClick: () => void;
}) {
  return (
    <div className="rounded-xl border border-border bg-card p-5 shadow-[var(--shadow-sm)]">
      <div className="flex size-9 items-center justify-center rounded-md bg-accent text-foreground">
        <Icon className="size-4" />
      </div>
      <h3 className="mt-3 text-sm font-semibold tracking-tight">{title}</h3>
      <p className="mt-1 text-xs leading-relaxed text-muted-foreground">{body}</p>
      <button
        type="button"
        onClick={onClick}
        className="mt-4 inline-flex h-8 items-center rounded-md bg-primary px-3 text-xs font-medium text-primary-foreground transition-opacity hover:opacity-90"
      >
        {cta}
      </button>
    </div>
  );
}
