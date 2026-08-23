import { useEffect, useState } from "react";
import { Plus, FolderOpen, FolderPlus } from "lucide-react";
import { useAppStore } from "../../store/appStore";
import { useProjects } from "../../hooks/useProjects";
import { ProjectList } from "./ProjectList";
import { AttentionPanel, useAttentionItems } from "./AttentionPanel";
import { TodayPanel } from "./TodayPanel";
import { EmptyState } from "./EmptyState";
import { LoadingSpinner } from "../shared/LoadingSpinner";
import { PageHeader } from "../layout/AppShell";
import { NewProjectDialog } from "../import/NewProjectDialog";
import {
  DropdownMenu,
  DropdownMenuTrigger,
  DropdownMenuContent,
  DropdownMenuItem,
} from "../ui/dropdown-menu";

function greeting(): string {
  const hour = new Date().getHours();
  if (hour < 12) return "Good morning";
  if (hour < 18) return "Good afternoon";
  return "Good evening";
}

export function Dashboard() {
  const projects = useAppStore((s) => s.projects);
  const isLoading = useAppStore((s) => s.isLoading);
  const openDrawer = useAppStore((s) => s.openDrawer);
  const { scanFolder, loadProjects } = useProjects();
  const attentionCount = useAttentionItems().length;
  const [showNewProject, setShowNewProject] = useState(false);

  // Loop/agent activity elsewhere (the Agent tab, another session editing
  // .loopdeck/loops.md) can change current_loop / next_steps_total /
  // next_steps_done / run_state on disk without this tab knowing — the
  // project list is otherwise only loaded once, at App mount. Re-fetch on
  // every Dashboard visit so returning here after work shows current
  // progress instead of a stale startup snapshot.
  useEffect(() => {
    loadProjects();
  }, [loadProjects]);

  /** Open a project's detail drawer. The full entry is derived from
   *  `projects` in ProjectDrawer, so only the path identifier is stored. */
  const handleSelect = (path: string) => {
    openDrawer(path);
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
  const runningCount = projects.filter((p) => p.run_state === "working").length;
  const subtitle = showEmpty
    ? undefined
    : attentionCount > 0
      ? `${attentionCount} project${attentionCount === 1 ? "" : "s"} need${attentionCount === 1 ? "s" : ""} attention. Everything else is moving.`
      : "All caught up. Nothing needs your attention right now.";

  return (
    <div className="flex flex-1 flex-col min-h-0 overflow-y-auto">
      {showEmpty ? (
        <>
          <PageHeader title="Dashboard" />
          <EmptyState onScan={handleScan} onNewProject={() => setShowNewProject(true)} />
        </>
      ) : (
        <div className="mx-auto w-full max-w-[1240px] px-8 py-7">
          <div className="mb-7 flex items-start gap-6">
            <div className="min-w-0">
              <h1 className="text-2xl font-bold tracking-tight">{greeting()}</h1>
              <p className="mt-1.5 text-sm leading-relaxed text-muted-foreground">{subtitle}</p>
            </div>
            <DropdownMenu>
              <DropdownMenuTrigger asChild>
                <button
                  type="button"
                  className="ml-auto inline-flex h-9 shrink-0 items-center gap-1.5 rounded-md bg-primary px-4 text-sm font-semibold text-primary-foreground transition-opacity hover:opacity-90"
                >
                  <Plus className="size-4" />
                  Add project
                </button>
              </DropdownMenuTrigger>
              <DropdownMenuContent align="end" className="w-44">
                <DropdownMenuItem onClick={handleScan}>
                  <FolderOpen className="size-3.5" />
                  Import repo
                </DropdownMenuItem>
                <DropdownMenuItem onClick={() => setShowNewProject(true)}>
                  <FolderPlus className="size-3.5" />
                  New project
                </DropdownMenuItem>
              </DropdownMenuContent>
            </DropdownMenu>
          </div>

          <AttentionPanel />

          <div className="grid grid-cols-1 gap-7 lg:grid-cols-[minmax(0,1.65fr)_minmax(300px,1fr)]">
            <section aria-labelledby="projects-title">
              <div className="mb-2.5 flex min-h-[28px] items-start">
                <div>
                  <h2 id="projects-title" className="text-base font-semibold tracking-tight">
                    Projects
                  </h2>
                  <p className="mt-0.5 text-xs text-muted-foreground">
                    {projects.length} project{projects.length !== 1 ? "s" : ""} · sorted by recent
                    activity
                  </p>
                </div>
              </div>
              <ProjectList projects={projects} onSelect={(p) => handleSelect(p.path)} />
            </section>

            <TodayPanel />
          </div>

          <footer className="mt-7 flex items-center text-[11px] text-muted-foreground/70">
            <span>All project data stays on this Mac</span>
            <span className="ml-auto flex items-center gap-1.5">
              <span className="size-1.5 rounded-full bg-success" />
              {runningCount} agent{runningCount === 1 ? "" : "s"} running
            </span>
          </footer>
        </div>
      )}

      <NewProjectDialog open={showNewProject} onOpenChange={setShowNewProject} />
    </div>
  );
}
