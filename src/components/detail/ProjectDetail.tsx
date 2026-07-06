import { useState, useEffect } from "react";
import { useParams, useNavigate, Link } from "@tanstack/react-router";
import {
  ArrowLeft,
  Pencil,
  RefreshCw,
  FolderOpen,
  Terminal,
  Trash2,
  GitCommitHorizontal,
  Clock,
  LayoutDashboard,
  Lightbulb,
  Repeat,
  Bot,
} from "lucide-react";
import { relativeTime } from "../../lib/time";
import { useAppStore } from "../../store/appStore";
import { useProjects } from "../../hooks/useProjects";
import { EditDescription } from "./EditDescription";
import { DecisionsPanel } from "./DecisionsPanel";
import { LoopsPanel } from "./LoopsPanel";
import { AgentPanel } from "./AgentPanel";
import { ConfirmDialog } from "../shared/ConfirmDialog";
import { StatusBadge } from "../shared/StatusBadge";
import { PageHeader } from "../layout/AppShell";
import { Section, IconButton, ActionButton } from "./Section";
import { cn } from "../../lib/utils";
import type { DetailTab } from "../../types";

const TABS: { id: DetailTab; label: string; icon: React.ReactNode }[] = [
  { id: "overview", label: "Overview", icon: <LayoutDashboard size={14} /> },
  { id: "decisions", label: "Decisions", icon: <Lightbulb size={14} /> },
  { id: "loops", label: "Loops", icon: <Repeat size={14} /> },
  { id: "agent", label: "Agent", icon: <Bot size={14} /> },
];

export function ProjectDetail() {
  const navigate = useNavigate();
  const { projectPath: encodedPath } = useParams({ strict: false }) as { projectPath: string };
  const projectPath = decodeURIComponent(encodedPath);

  const projects = useAppStore((s) => s.projects);
  const setSelectedProject = useAppStore((s) => s.setSelectedProject);
  const project = useAppStore((s) => s.selectedProject);
  const activeTab = useAppStore((s) => s.detailTab);
  const setActiveTab = useAppStore((s) => s.setDetailTab);
  const { openInFinder, openInTerminal, removeProject, rescanProject, regenerateDesc } =
    useProjects();

  const [isEditing, setIsEditing] = useState(false);
  const [showRemoveConfirm, setShowRemoveConfirm] = useState(false);

  // Sync the store's selectedProject with the route param on mount / change.
  useEffect(() => {
    const match = projects.find((p) => p.path === projectPath);
    if (match && match.path !== project?.path) {
      setSelectedProject(match);
    }
  }, [projectPath, projects, project, setSelectedProject]);

  if (!project) {
    return (
      <div className="flex-1 flex items-center justify-center">
        <p className="text-sm text-muted-foreground">No project selected.</p>
      </div>
    );
  }

  const handleRemove = () => {
    removeProject(project.path);
    setShowRemoveConfirm(false);
  };

  const handleRegenerate = async () => {
    await regenerateDesc(project.path);
  };

  const handleRescan = async () => {
    await rescanProject(project.path);
  };

  return (
    <div className="flex flex-1 flex-col min-h-0">
      <PageHeader
        title={
          <span className="flex items-center gap-2">
            <Link
              to="/"
              className="flex size-7 items-center justify-center rounded-md text-muted-foreground transition-colors hover:bg-accent hover:text-foreground"
              onClick={(e) => {
                // Use the navigate() path so memory-history back works the same.
                e.preventDefault();
                navigate({ to: "/" });
              }}
              aria-label="Back to dashboard"
            >
              <ArrowLeft className="size-4" />
            </Link>
            {project.name}
          </span>
        }
        subtitle={<span className="font-mono text-[11px]">{project.path}</span>}
        actions={<StatusBadge status={project.status} />}
      />

      {/* Body: tab rail + content */}
      <div className="flex flex-1 min-h-0">
        {/* Sidebar nav */}
        <nav className="flex w-44 shrink-0 flex-col gap-0.5 border-r border-border p-3">
          <div className="mb-1 px-3 py-1 text-[10px] font-semibold uppercase tracking-wider text-muted-foreground">
            {project.name}
          </div>
          {TABS.map((tab) => {
            const active = activeTab === tab.id;
            return (
              <button
                key={tab.id}
                onClick={() => setActiveTab(tab.id)}
                className={cn(
                  "relative rounded-md px-3 py-1.5 text-left text-sm transition-colors",
                  active
                    ? "nav-active-bar bg-accent font-medium text-foreground"
                    : "text-muted-foreground hover:bg-accent/60 hover:text-foreground",
                )}
              >
                {tab.label}
              </button>
            );
          })}
        </nav>

        {/* Content panel */}
        <div className="flex-1 min-h-0 min-w-0 overflow-y-auto p-8">
          {activeTab === "overview" && (
            <OverviewTab
              project={project}
              isEditing={isEditing}
              onEdit={() => setIsEditing(true)}
              onCancelEdit={() => setIsEditing(false)}
              onRegenerate={handleRegenerate}
              onRescan={handleRescan}
              onFinder={() => openInFinder(project.path)}
              onTerminal={() => openInTerminal(project.path)}
              onRemove={() => setShowRemoveConfirm(true)}
            />
          )}

          {activeTab === "decisions" && (
            <DecisionsPanel projectPath={project.path} />
          )}

          {activeTab === "loops" && <LoopsPanel projectPath={project.path} />}

          {activeTab === "agent" && <AgentPanel projectPath={project.path} />}
        </div>
      </div>

      {showRemoveConfirm && (
        <ConfirmDialog
          title="Remove Project"
          message={`Remove "${project.name}" from the registry? The .loopdeck folder and project files will NOT be deleted.`}
          confirmLabel="Remove"
          onConfirm={handleRemove}
          onCancel={() => setShowRemoveConfirm(false)}
          danger
        />
      )}
    </div>
  );
}

// ── Overview tab ─────────────────────────────────────────────────────────────

function OverviewTab({
  project,
  isEditing,
  onEdit,
  onCancelEdit,
  onRegenerate,
  onRescan,
  onFinder,
  onTerminal,
  onRemove,
}: {
  project: ReturnType<typeof useAppStore.getState>["selectedProject"];
  isEditing: boolean;
  onEdit: () => void;
  onCancelEdit: () => void;
  onRegenerate: () => void;
  onRescan: () => void;
  onFinder: () => void;
  onTerminal: () => void;
  onRemove: () => void;
}) {
  if (!project) return null;
  const hasChanges = project.uncommitted.files > 0;

  return (
    <div className="mx-auto max-w-2xl">
      <h2 className="mb-5 text-sm font-semibold tracking-tight">Overview</h2>

      <div className="rounded-xl border border-border bg-card p-6 shadow-[var(--shadow-sm)]">
        <Section label="Path">
          <p className="font-mono text-xs text-muted-foreground break-all">{project.path}</p>
        </Section>

        <Section
          label="Description"
          actions={
            <>
              <IconButton label="Edit description" onClick={onEdit}>
                <Pencil className="size-3.5" />
              </IconButton>
              <IconButton label="Regenerate from README" onClick={onRegenerate}>
                <RefreshCw className="size-3.5" />
              </IconButton>
            </>
          }
        >
          {isEditing ? (
            <EditDescription
              path={project.path}
              initialDescription={project.description}
              onSaved={onCancelEdit}
              onCancel={onCancelEdit}
            />
          ) : (
            <p className="text-sm leading-relaxed">
              {project.description || (
                <span className="italic text-muted-foreground">No description set.</span>
              )}
            </p>
          )}
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
              <dd className="mt-1 font-medium">
                {new Date(project.created_at).toLocaleDateString()}
              </dd>
            </div>
            <div>
              <dt className="text-muted-foreground">Opened</dt>
              <dd className="mt-1 font-medium">
                {project.last_opened ? relativeTime(project.last_opened) : "—"}
              </dd>
            </div>
          </dl>
        </Section>

        <Section label="Repository Activity">
          <div className="space-y-2 text-xs">
            <div className="flex items-start gap-2">
              <GitCommitHorizontal className="mt-0.5 size-3.5 text-muted-foreground" />
              <div>
                <span className="text-muted-foreground">Last commit · </span>
                <span>
                  {project.last_commit_date ? relativeTime(project.last_commit_date) : "none"}
                </span>
                {project.last_commit_message && (
                  <div className="font-mono text-[11px] text-muted-foreground">
                    {project.last_commit_message}
                  </div>
                )}
              </div>
            </div>
            {project.last_modified && (
              <div className="flex items-center gap-2">
                <Clock className="size-3.5 text-muted-foreground" />
                <span className="text-muted-foreground">Last modified · </span>
                <span>{relativeTime(project.last_modified)}</span>
              </div>
            )}
            <div className="flex items-center gap-2">
              <Clock className="size-3.5 text-muted-foreground" />
              <span className="text-muted-foreground">Uncommitted · </span>
              <span>
                {hasChanges
                  ? `${project.uncommitted.files} ${project.uncommitted.files === 1 ? "file" : "files"} · +${project.uncommitted.added} −${project.uncommitted.deleted}`
                  : "clean"}
              </span>
            </div>
          </div>
        </Section>
      </div>

      <div className="mt-6 flex flex-wrap items-center gap-2">
        <ActionButton icon={RefreshCw} label="Rescan" onClick={onRescan} />
        <ActionButton icon={FolderOpen} label="Finder" onClick={onFinder} />
        <ActionButton icon={Terminal} label="Terminal" onClick={onTerminal} />
        <ActionButton icon={Trash2} label="Remove" onClick={onRemove} destructive />
      </div>
    </div>
  );
}
