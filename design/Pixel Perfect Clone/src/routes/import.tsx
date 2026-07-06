import { createFileRoute } from "@tanstack/react-router";
import { FolderPlus, FolderOpen, GitBranch } from "lucide-react";

import { PageHeader } from "@/components/app-shell";

export const Route = createFileRoute("/import")({
  component: ImportPage,
});

function ImportPage() {
  return (
    <>
      <PageHeader
        title="Import Repo"
        subtitle="Scan a folder or clone a remote repository into LoopDeck"
      />
      <div className="mx-auto grid w-full max-w-3xl grid-cols-1 gap-4 px-8 py-8 md:grid-cols-2">
        <ImportCard
          icon={FolderOpen}
          title="Scan local folder"
          body="Pick a folder on disk. LoopDeck discovers repositories and writes project memory in .loopdeck/."
          cta="Choose folder"
        />
        <ImportCard
          icon={GitBranch}
          title="Clone remote repo"
          body="Paste a git URL. We clone it, initialize memory, and open the project detail."
          cta="Paste git URL"
        />
      </div>

      <div className="mx-auto w-full max-w-3xl px-8 pb-12">
        <div className="rounded-xl border border-dashed border-border bg-muted/20 p-10 text-center">
          <FolderPlus className="mx-auto size-8 text-muted-foreground opacity-60" />
          <p className="mt-3 text-xs text-muted-foreground">
            Or drag a folder here to import
          </p>
        </div>
      </div>
    </>
  );
}

function ImportCard({
  icon: Icon,
  title,
  body,
  cta,
}: {
  icon: React.ComponentType<{ className?: string }>;
  title: string;
  body: string;
  cta: string;
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
        className="mt-4 inline-flex h-8 items-center rounded-md bg-primary px-3 text-xs font-medium text-primary-foreground transition-opacity hover:opacity-90"
      >
        {cta}
      </button>
    </div>
  );
}
