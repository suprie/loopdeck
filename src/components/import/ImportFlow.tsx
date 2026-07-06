import { useAppStore } from "../../store/appStore";
import { useProjects } from "../../hooks/useProjects";
import { RepoCard } from "./RepoCard";
import { LoadingSpinner } from "../shared/LoadingSpinner";
import "./ImportFlow.css";

export function ImportFlow() {
  const discoveredRepos = useAppStore((s) => s.discoveredRepos);
  const isScanning = useAppStore((s) => s.isScanning);
  const isLoading = useAppStore((s) => s.isLoading);
  const setCurrentView = useAppStore((s) => s.setCurrentView);
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
    return <LoadingSpinner label="Scanning for repositories..." />;
  }

  return (
    <div className="import-flow">
      <div className="import-flow__header">
        <h2>Discovered Repositories</h2>
        <div style={{ display: "flex", gap: 8 }}>
          <button className="btn-secondary" onClick={handleScanAgain}>
            Scan Again
          </button>
          <button className="btn-secondary" onClick={() => setCurrentView("dashboard")}>
            Back to Dashboard
          </button>
        </div>
      </div>
      <p className="import-flow__subtitle">
        Found {discoveredRepos.length} repo{discoveredRepos.length !== 1 ? "s" : ""}.
        Select repositories to import and create project memory.
      </p>

      {discoveredRepos.length === 0 ? (
        <div className="import-flow__empty">
          No repositories discovered. Try scanning a different folder.
        </div>
      ) : (
        <div className="import-flow__list">
          {discoveredRepos.map((repo) => (
            <RepoCard
              key={repo.path}
              repo={repo}
              onImport={handleImport}
            />
          ))}
        </div>
      )}

      {isLoading && <LoadingSpinner label="Importing project..." />}
    </div>
  );
}
