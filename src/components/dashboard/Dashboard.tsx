import { useAppStore } from "../../store/appStore";
import { useProjects } from "../../hooks/useProjects";
import { ProjectCard } from "./ProjectCard";
import { EmptyState } from "./EmptyState";
import { LoadingSpinner } from "../shared/LoadingSpinner";
import { PageHeader } from "../layout/AppShell";
import { Plus } from "lucide-react";

export function Dashboard() {
  const projects = useAppStore((s) => s.projects);
  const isLoading = useAppStore((s) => s.isLoading);
  const setSelectedProject = useAppStore((s) => s.setSelectedProject);
  const { openInFinder, openInTerminal, removeProject, rescanProject, scanFolder } =
    useProjects();

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

  if (projects.length === 0) {
    return (
      <div className="flex-1 flex flex-col">
        <PageHeader title="Dashboard" subtitle="No projects yet" />
        <div className="flex-1 flex items-center justify-center">
          <EmptyState onScan={handleScan} />
        </div>
      </div>
    );
  }

  const activeCount = projects.filter((p) => p.status === "active").length;

  return (
    <>
      <PageHeader
        title="Dashboard"
        subtitle={`${projects.length} project${projects.length !== 1 ? "s" : ""} · ${activeCount} active`}
        actions={
          <button
            onClick={handleScan}
            className="inline-flex items-center gap-1.5 h-8 px-3 rounded-md bg-primary text-primary-foreground text-xs font-medium hover:opacity-90 transition"
          >
            <Plus className="size-3.5" />
            Import Repository
          </button>
        }
      />
      <div className="flex-1 overflow-y-auto p-6">
        <div className="grid grid-cols-1 md:grid-cols-2 xl:grid-cols-3 gap-4">
          {projects.map((project) => (
            <ProjectCard
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
    </>
  );
}
