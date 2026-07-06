import { createFileRoute } from "@tanstack/react-router";
import { Plus, FolderOpen } from "lucide-react";
import { useState } from "react";

import { PageHeader } from "@/components/app-shell";
import { ProjectCard } from "@/components/project-card";
import { projects } from "@/lib/mock-data";

export const Route = createFileRoute("/")({
  component: DashboardPage,
});

function DashboardPage() {
  const [showEmpty, setShowEmpty] = useState(false);
  const active = projects.filter((p) => p.status === "active").length;

  return (
    <>
      <PageHeader
        title="Dashboard"
        subtitle={`${projects.length} projects · ${active} active`}
        actions={
          <>
            <button
              type="button"
              onClick={() => setShowEmpty((s) => !s)}
              className="inline-flex h-8 items-center gap-1.5 rounded-md border border-border bg-transparent px-3 text-xs font-medium text-muted-foreground transition-colors hover:bg-accent hover:text-foreground"
            >
              {showEmpty ? "Show projects" : "Preview empty state"}
            </button>
            <button
              type="button"
              className="inline-flex h-8 items-center gap-1.5 rounded-md bg-primary px-3 text-xs font-medium text-primary-foreground transition-opacity hover:opacity-90"
            >
              <Plus className="size-3.5" />
              Import Repo
            </button>
          </>
        }
      />

      {showEmpty ? (
        <EmptyState />
      ) : (
        <div className="grid flex-1 auto-rows-max grid-cols-[repeat(auto-fill,minmax(280px,1fr))] gap-4 p-8">
          {projects.map((p) => (
            <ProjectCard key={p.id} project={p} />
          ))}
        </div>
      )}
    </>
  );
}

function EmptyState() {
  return (
    <div className="flex flex-1 items-center justify-center px-4 py-24">
      <div className="flex max-w-sm flex-col items-center text-center">
        <div className="mb-6 flex size-20 items-center justify-center rounded-2xl border border-dashed border-border bg-muted/30 opacity-70">
          <FolderOpen className="size-10 text-muted-foreground opacity-60" />
        </div>
        <h2 className="text-base font-semibold tracking-tight">No projects found</h2>
        <p className="mt-2 text-sm leading-relaxed text-muted-foreground">
          Scan a folder to discover repositories and create project memory. LoopDeck stores context
          inside each repository.
        </p>
        <button
          type="button"
          className="mt-6 inline-flex h-9 items-center gap-2 rounded-md bg-primary px-4 text-sm font-medium text-primary-foreground transition-opacity hover:opacity-90"
        >
          <FolderOpen className="size-4" />
          Scan Folder
        </button>
      </div>
    </div>
  );
}
