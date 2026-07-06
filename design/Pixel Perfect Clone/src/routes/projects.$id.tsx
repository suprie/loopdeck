import { createFileRoute, Link, notFound, useRouterState } from "@tanstack/react-router";
import {
  ArrowLeft,
  RefreshCw,
  FolderOpen,
  Terminal,
  Trash2,
  Pencil,
  RotateCw,
  GitCommitHorizontal,
  Clock,
} from "lucide-react";

import { PageHeader } from "@/components/app-shell";
import { StatusBadge } from "@/components/status-badge";
import { getProject } from "@/lib/mock-data";
import { cn } from "@/lib/utils";

export const Route = createFileRoute("/projects/$id")({
  loader: ({ params }) => {
    const project = getProject(params.id);
    if (!project) throw notFound();
    return { project };
  },
  component: ProjectDetailPage,
});

const tabs = [
  { key: "overview", label: "Overview" },
  { key: "decisions", label: "Decisions" },
  { key: "loops", label: "Loops" },
  { key: "agent", label: "Agent" },
] as const;

function ProjectDetailPage() {
  const { project } = Route.useLoaderData();
  const pathname = useRouterState({ select: (s) => s.location.pathname });
  const activeTab =
    tabs.find((t) => pathname.endsWith("/" + t.key))?.key ?? "overview";

  return (
    <>
      <PageHeader
        title={
          <span className="flex items-center gap-2">
            <Link
              to="/"
              className="flex size-7 items-center justify-center rounded-md text-muted-foreground transition-colors hover:bg-accent hover:text-foreground"
            >
              <ArrowLeft className="size-4" />
            </Link>
            {project.name}
          </span>
        }
        subtitle={<span className="font-mono text-[11px]">{project.path}</span>}
        actions={<StatusBadge status={project.status} />}
      />

      <div className="flex min-h-0 flex-1">
        <nav className="flex w-44 shrink-0 flex-col gap-0.5 border-r border-border p-3">
          <div className="mb-1 px-3 py-1 text-[10px] font-semibold uppercase tracking-wider text-muted-foreground">
            {project.name}
          </div>
          {tabs.map((t) => {
            const active = t.key === activeTab;
            return (
              <button
                key={t.key}
                type="button"
                className={cn(
                  "relative rounded-md px-3 py-1.5 text-left text-sm transition-colors",
                  active
                    ? "nav-active-bar bg-accent text-foreground font-medium"
                    : "text-muted-foreground hover:bg-accent/60 hover:text-foreground",
                )}
              >
                {t.label}
              </button>
            );
          })}
        </nav>

        <div className="min-w-0 flex-1 p-8">
          <div className="mx-auto max-w-2xl">
            <h2 className="mb-5 text-sm font-semibold tracking-tight">Overview</h2>

            <div className="rounded-xl border border-border bg-card p-6 shadow-[var(--shadow-sm)]">
              <Section label="Path">
                <p className="font-mono text-xs text-muted-foreground">{project.path}</p>
              </Section>

              <Section
                label="Description"
                actions={
                  <>
                    <IconBtn>
                      <Pencil className="size-3.5" />
                    </IconBtn>
                    <IconBtn>
                      <RotateCw className="size-3.5" />
                    </IconBtn>
                  </>
                }
              >
                <p className="text-sm leading-relaxed">{project.description}</p>
              </Section>

              <Section label="Details">
                <dl className="grid grid-cols-3 gap-4 text-xs">
                  <div>
                    <dt className="text-muted-foreground">Status</dt>
                    <dd className="mt-1">
                      <StatusBadge status={project.status} />
                    </dd>
                  </div>
                  <div>
                    <dt className="text-muted-foreground">Created</dt>
                    <dd className="mt-1 font-medium">{project.createdAt}</dd>
                  </div>
                  <div>
                    <dt className="text-muted-foreground">Opened</dt>
                    <dd className="mt-1 font-medium">{project.openedAgo}</dd>
                  </div>
                </dl>
              </Section>

              <Section label="Repository Activity">
                <div className="space-y-2 text-xs">
                  <div className="flex items-start gap-2">
                    <GitCommitHorizontal className="mt-0.5 size-3.5 text-muted-foreground" />
                    <div>
                      <span className="text-muted-foreground">Last commit · </span>
                      <span>{project.lastCommit.ago}</span>
                      <div className="font-mono text-[11px] text-muted-foreground">
                        {project.lastCommit.message}
                      </div>
                    </div>
                  </div>
                  <div className="flex items-center gap-2">
                    <Clock className="size-3.5 text-muted-foreground" />
                    <span className="text-muted-foreground">Last modified · </span>
                    <span>{project.lastModified}</span>
                  </div>
                </div>
              </Section>
            </div>

            <div className="mt-6 flex flex-wrap items-center gap-2">
              <ActionButton icon={RefreshCw} label="Rescan" />
              <ActionButton icon={FolderOpen} label="Finder" />
              <ActionButton icon={Terminal} label="Terminal" />
              <ActionButton icon={Trash2} label="Remove" destructive />
            </div>
          </div>
        </div>
      </div>
    </>
  );
}

function Section({
  label,
  actions,
  children,
}: {
  label: string;
  actions?: React.ReactNode;
  children: React.ReactNode;
}) {
  return (
    <section className="border-b border-border py-4 first:pt-0 last:border-b-0 last:pb-0">
      <div className="mb-2 flex items-center justify-between">
        <span className="text-[10px] font-semibold uppercase tracking-wider text-muted-foreground">
          {label}
        </span>
        {actions && <div className="flex gap-1">{actions}</div>}
      </div>
      {children}
    </section>
  );
}

function IconBtn({ children }: { children: React.ReactNode }) {
  return (
    <button
      type="button"
      className="flex size-6 items-center justify-center rounded-md text-muted-foreground transition-colors hover:bg-accent hover:text-foreground"
    >
      {children}
    </button>
  );
}

function ActionButton({
  icon: Icon,
  label,
  destructive,
}: {
  icon: React.ComponentType<{ className?: string }>;
  label: string;
  destructive?: boolean;
}) {
  return (
    <button
      type="button"
      className={cn(
        "inline-flex h-8 items-center gap-1.5 rounded-md px-3 text-xs font-medium transition-colors",
        destructive
          ? "text-destructive hover:bg-destructive/10"
          : "text-muted-foreground hover:bg-accent hover:text-foreground",
      )}
    >
      <Icon className="size-3.5" />
      {label}
    </button>
  );
}
