import { useState } from "react";
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
} from "lucide-react";
import { relativeTime } from "../../lib/time";
import { useAppStore } from "../../store/appStore";
import { useProjects } from "../../hooks/useProjects";
import { EditDescription } from "./EditDescription";
import { DecisionsPanel } from "./DecisionsPanel";
import { LoopsPanel } from "./LoopsPanel";
import { ConfirmDialog } from "../shared/ConfirmDialog";
import type { DetailTab } from "../../types";
import "./ProjectDetail.css";

const TABS: { id: DetailTab; label: string; icon: React.ReactNode }[] = [
  { id: "overview", label: "Overview", icon: <LayoutDashboard size={14} /> },
  { id: "decisions", label: "Decisions", icon: <Lightbulb size={14} /> },
  { id: "loops", label: "Loops", icon: <Repeat size={14} /> },
];

export function ProjectDetail() {
  const project = useAppStore((s) => s.selectedProject);
  const setCurrentView = useAppStore((s) => s.setCurrentView);
  const {
    openInFinder,
    openInTerminal,
    removeProject,
    rescanProject,
    regenerateDesc,
  } = useProjects();

  const [activeTab, setActiveTab] = useState<DetailTab>("overview");
  const [isEditing, setIsEditing] = useState(false);
  const [showRemoveConfirm, setShowRemoveConfirm] = useState(false);

  if (!project) {
    return (
      <div className="project-detail">
        <p style={{ color: "var(--color-text-secondary)" }}>No project selected.</p>
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
    <div className="project-detail">
      {/* Header */}
      <div className="project-detail__header">
        <button
          className="project-detail__back"
          onClick={() => setCurrentView("dashboard")}
          title="Back to dashboard"
        >
          <ArrowLeft size={18} />
        </button>
        <h1 className="project-detail__title">{project.name}</h1>
      </div>

      {/* Body: sidebar + content */}
      <div className="project-detail__body">
        {/* Sidebar nav */}
        <nav className="project-detail__sidebar">
          <div className="project-detail__sidebar-project">
            <span className="project-detail__sidebar-name">{project.name}</span>
            <span className="project-detail__sidebar-path">{project.path}</span>
          </div>
          {TABS.map((tab) => (
            <button
              key={tab.id}
              className={`project-detail__tab ${
                activeTab === tab.id ? "project-detail__tab--active" : ""
              }`}
              onClick={() => setActiveTab(tab.id)}
            >
              {tab.icon}
              {tab.label}
            </button>
          ))}
        </nav>

        {/* Content panel */}
        <div className="project-detail__content">
          {activeTab === "overview" && (
            <>
              <div className="project-detail__card">
                <div className="project-detail__field">
                  <div className="project-detail__label">Path</div>
                  <div className="project-detail__value project-detail__value--path">
                    {project.path}
                  </div>
                </div>

                <div className="project-detail__field">
                  <div className="project-detail__label">Description</div>
                  {isEditing ? (
                    <EditDescription
                      path={project.path}
                      initialDescription={project.description}
                      onSaved={() => setIsEditing(false)}
                      onCancel={() => setIsEditing(false)}
                    />
                  ) : (
                    <div className="project-detail__description">
                      <p className={project.description ? "" : "is-empty"}>
                        {project.description || "No description set."}
                      </p>
                      <div style={{ display: "flex", gap: 4, flexShrink: 0 }}>
                        <button
                          className="btn-secondary"
                          onClick={() => setIsEditing(true)}
                          title="Edit description"
                        >
                          <Pencil size={14} />
                        </button>
                        <button
                          className="btn-secondary"
                          onClick={handleRegenerate}
                          title="Regenerate from README"
                        >
                          <RefreshCw size={14} />
                        </button>
                      </div>
                    </div>
                  )}
                </div>

                <div className="project-detail__field">
                  <div className="project-detail__label">Details</div>
                  <div className="project-detail__meta">
                    <div className="project-detail__value">
                      Status: <strong>{project.status}</strong>
                    </div>
                    <div className="project-detail__value">
                      Created:{" "}
                      <strong>
                        {new Date(project.created_at).toLocaleDateString()}
                      </strong>
                    </div>
                    {project.last_opened && (
                      <div className="project-detail__value">
                        Last opened:{" "}
                        <strong>
                          {new Date(project.last_opened).toLocaleDateString()}
                        </strong>
                      </div>
                    )}
                  </div>
                </div>

                <div className="project-detail__field">
                  <div className="project-detail__label">Repository Activity</div>
                  <div className="project-detail__meta">
                    <div className="project-detail__value">
                      <GitCommit size={13} /> Last commit:{" "}
                      <strong>
                        {project.last_commit
                          ? relativeTime(project.last_commit)
                          : "Uncommitted"}
                      </strong>
                    </div>
                    {project.last_modified && (
                      <div className="project-detail__value">
                        <Clock size={13} /> Last modified:{" "}
                        <strong>
                          {relativeTime(project.last_modified)}
                        </strong>
                      </div>
                    )}
                  </div>
                </div>
              </div>

              <div className="project-detail__actions">
                <button
                  className="btn-secondary"
                  onClick={handleRescan}
                >
                  <RefreshCw size={14} /> Rescan
                </button>
                <button
                  className="btn-secondary"
                  onClick={() => openInFinder(project.path)}
                >
                  <Folder size={14} /> Open in Finder
                </button>
                <button
                  className="btn-secondary"
                  onClick={() => openInTerminal(project.path)}
                >
                  <Terminal size={14} /> Open in Terminal
                </button>
                <button
                  className="btn-danger"
                  onClick={() => setShowRemoveConfirm(true)}
                >
                  <Trash2 size={14} /> Remove from Registry
                </button>
              </div>
            </>
          )}

          {activeTab === "decisions" && (
            <DecisionsPanel projectPath={project.path} />
          )}

          {activeTab === "loops" && (
            <LoopsPanel projectPath={project.path} />
          )}
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
