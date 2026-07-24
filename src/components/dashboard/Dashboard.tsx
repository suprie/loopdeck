import { useNavigate } from "@tanstack/react-router";
import { Plus } from "lucide-react";
import { useAppStore } from "../../store/appStore";
import { useProjects } from "../../hooks/useProjects";
import { ProjectCard } from "./ProjectCard";
import { EmptyState } from "./EmptyState";
import { LoadingSpinner } from "../shared/LoadingSpinner";
import { PageHeader } from "../layout/AppShell";

export function Dashboard() {
  const navigate = useNavigate();
  const projects = useAppStore((s) => s.projects);
  const isLoading = useAppStore((s) => s.isLoading);
  const setSelectedProjectPath = useAppStore((s) => s.setSelectedProjectPath);
  const setDetailTab = useAppStore((s) => s.setDetailTab);
  const setPendingAgentStart = useAppStore((s) => s.setPendingAgentStart);
  const { openInFinder, openInTerminal, removeProject, rescanProject, scanFolder } =
    useProjects();

  /** Navigate to a project's detail view. The full entry is derived from
   *  `projects` in ProjectDetail, so only the path identifier is stored. */
  const handleSelect = (path: string) => {
    setSelectedProjectPath(path);
    navigate({ to: "/project/$projectPath", params: { projectPath: encodeURIComponent(path) } });
  };

  /** Start the agent from the dashboard: navigate to the project on the Agent
   *  tab and signal AgentPanel to auto-fire `agent_start_loop` on mount. */
  const handleStart = (path: string) => {
    if (!projects.some((p) => p.path === path)) return;
    setSelectedProjectPath(path);
    setDetailTab("agent");
    setPendingAgentStart(path);
    navigate({ to: "/project/$projectPath", params: { projectPath: encodeURIComponent(path) } });
  };

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
    return (
      <div className="flex-1 flex items-center justify-center">
        <LoadingSpinner label="Loading projects..." />
      </div>
    );
  }

  const showEmpty = projects.length === 0;
  const activeCount = projects.filter((p) => p.status === "active").length;
  const subtitle = `${projects.length} project${projects.length !== 1 ? "s" : ""} · ${activeCount} active`;

  return (
    <div className="flex flex-1 flex-col min-h-0">
      <PageHeader
        title="Dashboard"
        subtitle={subtitle}
        actions={
          <>
            <button
              type="button"
              onClick={handleScan}
              className="inline-flex h-8 items-center gap-1.5 rounded-md bg-primary px-3 text-xs font-medium text-primary-foreground transition-opacity hover:opacity-90"
            >
              <Plus className="size-3.5" />
              Import Repo
            </button>
          </>
        }
      />

      {showEmpty ? (
        <EmptyState onScan={handleScan} />
      ) : (
        <div className="grid flex-1 auto-rows-max grid-cols-[repeat(auto-fill,minmax(280px,1fr))] gap-4 overflow-y-auto p-8">
          {projects.map((project) => (
            <ProjectCard
              key={project.path}
              project={project}
              onSelect={() => handleSelect(project.path)}
              onOpenInFinder={openInFinder}
              onOpenInTerminal={openInTerminal}
              onRemove={removeProject}
              onRescan={rescanProject}
              onStart={handleStart}
            />
          ))}
        </div>
      )}
    </div>
  );
}
