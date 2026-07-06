import { useAppStore } from "../../store/appStore";
import { useProjects } from "../../hooks/useProjects";
import { ProjectRow } from "./ProjectRow";
import { EmptyState } from "./EmptyState";
import { LoadingSpinner } from "../shared/LoadingSpinner";
import "./Dashboard.css";

export function Dashboard() {
  const projects = useAppStore((s) => s.projects);
  const isLoading = useAppStore((s) => s.isLoading);
  const setSelectedProject = useAppStore((s) => s.setSelectedProject);
  const { openInFinder, openInTerminal, removeProject, rescanProject, scanFolder } = useProjects();

  const handleScan = async () => {
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
      // User cancelled or dialog failed
    }
  };

  if (isLoading && projects.length === 0) {
    return <LoadingSpinner label="Loading projects..." />;
  }

  if (projects.length === 0) {
    return <EmptyState onScan={handleScan} />;
  }

  return (
    <div className="dashboard">
      <div className="dashboard__header">
        <h2>Projects</h2>
        <span className="dashboard__count">
          {projects.length} project{projects.length !== 1 ? "s" : ""}
        </span>
      </div>
      <div className="dashboard__list">
        {projects.map((project) => (
          <ProjectRow
            key={project.path}
            project={project}
            onSelect={setSelectedProject}
            onOpenInFinder={openInFinder}
            onOpenInTerminal={openInTerminal}
            onRemove={removeProject}
            onRescan={rescanProject}
          />
        ))}
      </div>
    </div>
  );
}
