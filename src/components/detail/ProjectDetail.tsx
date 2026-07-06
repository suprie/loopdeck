import { useState, useEffect } from "react";
import { useParams, useNavigate } from "@tanstack/react-router";
import {
  ArrowLeft,
  Pencil,
  RefreshCw,
  Folder,
  Terminal,
  Trash2,
  GitCommit,
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
import { PageHeader } from "../layout/AppShell";
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
    <div className="flex-1 flex flex-col min-h-0">
      <PageHeader
        title={project.name}
        subtitle={project.path}
        actions={
          <div className="flex items-center gap-1.5">
            <button
              onClick={() => navigate({ to: "/" })}
              className="inline-flex items-center gap-1.5 h-7 px-2.5 rounded-md text-muted-foreground text-xs font-medium hover:bg-accent hover:text-foreground transition"
              title="Back to dashboard"
            >
              <ArrowLeft size={14} />
            </button>
          </div>
        }
      />

      {/* Body: sidebar + content */}
      <div className="flex flex-1 min-h-0">
        {/* Sidebar nav */}
        <nav className="w-[180px] shrink-0 border-r border-border p-3 space-y-0.5">
          <div className="px-2 py-1.5 mb-2">
            <div className="text-xs font-semibold text-foreground truncate">
              {project.name}
            </div>
            <div className="text-[10px] text-muted-foreground font-mono truncate mt-0.5">
              {project.path}
            </div>
          </div>
          {TABS.map((tab) => (
            <button
              key={tab.id}
              onClick={() => setActiveTab(tab.id)}
              className={`flex items-center gap-2 px-3 py-2 rounded-md text-sm transition-colors w-full text-left ${
                activeTab === tab.id
                  ? "bg-accent text-foreground"
                  : "text-muted-foreground hover:bg-accent/50 hover:text-foreground"
              }`}
            >
              {tab.icon}
              {tab.label}
            </button>
          ))}
        </nav>

        {/* Content panel */}
        <div className="flex-1 min-w-0 min-h-0 overflow-y-auto p-6">
          {activeTab === "overview" && (
            <div className="max-w-2xl space-y-4">
              {/* Overview card */}
              <div className="rounded-xl border border-border bg-card p-5 space-y-5">
                {/* Path */}
                <div>
                  <div className="text-[10px] uppercase tracking-wider text-muted-foreground font-semibold mb-1.5">
                    Path
                  </div>
                  <div className="text-sm font-mono text-muted-foreground break-all">
                    {project.path}
                  </div>
                </div>

                {/* Description */}
                <div>
                  <div className="text-[10px] uppercase tracking-wider text-muted-foreground font-semibold mb-1.5">
                    Description
                  </div>
                  {isEditing ? (
                    <EditDescription
                      path={project.path}
                      initialDescription={project.description}
                      onSaved={() => setIsEditing(false)}
                      onCancel={() => setIsEditing(false)}
                    />
                  ) : (
                    <div className="flex items-start justify-between gap-3">
                      <p
                        className={
                          project.description
                            ? "text-sm text-foreground leading-relaxed"
                            : "text-sm text-muted-foreground italic leading-relaxed"
                        }
                      >
                        {project.description || "No description set."}
                      </p>
                      <div className="flex gap-1 shrink-0">
                        <button
                          onClick={() => setIsEditing(true)}
                          className="inline-flex items-center gap-1 h-7 px-2 rounded-md bg-muted text-muted-foreground text-xs font-medium hover:bg-accent hover:text-foreground transition"
                          title="Edit description"
                        >
                          <Pencil size={12} />
                        </button>
                        <button
                          onClick={handleRegenerate}
                          className="inline-flex items-center gap-1 h-7 px-2 rounded-md bg-muted text-muted-foreground text-xs font-medium hover:bg-accent hover:text-foreground transition"
                          title="Regenerate from README"
                        >
                          <RefreshCw size={12} />
                        </button>
                      </div>
                    </div>
                  )}
                </div>

                {/* Details */}
                <div>
                  <div className="text-[10px] uppercase tracking-wider text-muted-foreground font-semibold mb-1.5">
                    Details
                  </div>
                  <div className="flex flex-wrap gap-3 text-sm">
                    <span className="text-muted-foreground">
                      Status: <strong className="text-foreground">{project.status}</strong>
                    </span>
                    <span className="text-muted-foreground">
                      Created:{" "}
                      <strong className="text-foreground">
                        {new Date(project.created_at).toLocaleDateString()}
                      </strong>
                    </span>
                    {project.last_opened && (
                      <span className="text-muted-foreground">
                        Last opened:{" "}
                        <strong className="text-foreground">
                          {new Date(project.last_opened).toLocaleDateString()}
                        </strong>
                      </span>
                    )}
                  </div>
                </div>

                {/* Repository Activity */}
                <div>
                  <div className="text-[10px] uppercase tracking-wider text-muted-foreground font-semibold mb-1.5">
                    Repository Activity
                  </div>
                  <div className="flex flex-wrap gap-3 text-sm">
                    <span className="text-muted-foreground inline-flex items-center gap-1.5">
                      <GitCommit size={13} />
                      Last commit:{" "}
                      <strong className="text-foreground">
                        {project.last_commit_date
                          ? relativeTime(project.last_commit_date)
                          : "Uncommitted"}
                      </strong>
                      {project.last_commit_message && (
                        <span className="text-muted-foreground font-mono text-xs truncate max-w-[200px]">
                          — {project.last_commit_message}
                        </span>
                      )}
                    </span>
                    {project.last_modified && (
                      <span className="text-muted-foreground inline-flex items-center gap-1.5">
                        <Clock size={13} />
                        Last modified:{" "}
                        <strong className="text-foreground">
                          {relativeTime(project.last_modified)}
                        </strong>
                      </span>
                    )}
                  </div>
                </div>
              </div>

              {/* Actions */}
              <div className="flex gap-2 flex-wrap">
                <button
                  onClick={handleRescan}
                  className="inline-flex items-center gap-1.5 h-8 px-3 rounded-md bg-muted text-muted-foreground text-xs font-medium hover:bg-accent hover:text-foreground transition"
                >
                  <RefreshCw size={13} /> Rescan
                </button>
                <button
                  onClick={() => openInFinder(project.path)}
                  className="inline-flex items-center gap-1.5 h-8 px-3 rounded-md bg-muted text-muted-foreground text-xs font-medium hover:bg-accent hover:text-foreground transition"
                >
                  <Folder size={13} /> Open in Finder
                </button>
                <button
                  onClick={() => openInTerminal(project.path)}
                  className="inline-flex items-center gap-1.5 h-8 px-3 rounded-md bg-muted text-muted-foreground text-xs font-medium hover:bg-accent hover:text-foreground transition"
                >
                  <Terminal size={13} /> Open in Terminal
                </button>
                <button
                  onClick={() => setShowRemoveConfirm(true)}
                  className="inline-flex items-center gap-1.5 h-8 px-3 rounded-md text-destructive text-xs font-medium hover:bg-[color-mix(in_oklab,var(--destructive)_12%,transparent)] transition"
                >
                  <Trash2 size={13} /> Remove from Registry
                </button>
              </div>
            </div>
          )}

          {activeTab === "decisions" && (
            <DecisionsPanel projectPath={project.path} />
          )}

          {activeTab === "loops" && (
            <LoopsPanel projectPath={project.path} />
          )}

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
