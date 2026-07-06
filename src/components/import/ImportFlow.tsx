import { useNavigate } from "@tanstack/react-router";
import { useAppStore } from "../../store/appStore";
import { useProjects } from "../../hooks/useProjects";
import { RepoCard } from "./RepoCard";
import { LoadingSpinner } from "../shared/LoadingSpinner";
import { PageHeader } from "../layout/AppShell";
import { ArrowLeft, Search } from "lucide-react";

export function ImportFlow() {
  const navigate = useNavigate();
  const discoveredRepos = useAppStore((s) => s.discoveredRepos);
  const isScanning = useAppStore((s) => s.isScanning);
  const isLoading = useAppStore((s) => s.isLoading);
  const { importRepo, scanFolder } = useProjects();

  const handleImport = async (path: string) => {
    const result = await importRepo(path);
    if (result) {
      // Stay on import view to allow importing more repos
    }
  };

  const handleScanAgain = async () => {
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
      <div className="flex-1 flex items-center justify-center">
        <LoadingSpinner label="Scanning for repositories..." />
      </div>
    );
  }

  return (
    <div className="flex-1 flex flex-col min-h-0">
      <PageHeader
        title="Import Repository"
        subtitle={
          discoveredRepos.length > 0
            ? `Found ${discoveredRepos.length} repo${discoveredRepos.length !== 1 ? "s" : ""}`
            : "Select a folder to scan for repositories"
        }
        actions={
          <div className="flex items-center gap-2">
            <button
              onClick={handleScanAgain}
              className="inline-flex items-center gap-1.5 h-8 px-3 rounded-md bg-muted text-muted-foreground text-xs font-medium hover:bg-accent hover:text-foreground transition"
            >
              <Search className="size-3.5" />
              Scan Again
            </button>
            <button
              onClick={() => navigate({ to: "/" })}
              className="inline-flex items-center gap-1.5 h-8 px-3 rounded-md bg-muted text-muted-foreground text-xs font-medium hover:bg-accent hover:text-foreground transition"
            >
              <ArrowLeft className="size-3.5" />
              Back
            </button>
          </div>
        }
      />

      <div className="flex-1 min-h-0 overflow-y-auto p-6">
        {discoveredRepos.length === 0 ? (
          <div className="flex flex-col items-center justify-center py-20 text-center">
            <p className="text-sm text-muted-foreground">
              No repositories discovered. Try scanning a different folder.
            </p>
          </div>
        ) : (
          <div className="max-w-3xl mx-auto space-y-3">
            {discoveredRepos.map((repo) => (
              <RepoCard key={repo.path} repo={repo} onImport={handleImport} />
            ))}
          </div>
        )}

        {isLoading && (
          <div className="mt-6">
            <LoadingSpinner label="Importing project..." />
          </div>
        )}
      </div>
    </div>
  );
}
