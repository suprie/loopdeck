import { useState, useEffect } from "react";
import { useParams, useNavigate, Link } from "@tanstack/react-router";
import {
  ArrowLeft,
  Pencil,
  Sparkles,
  RefreshCw,
  FolderOpen,
  Terminal,
  Trash2,
  GitCommitHorizontal,
  Clock,
  LayoutDashboard,
  Lightbulb,
  Repeat,
  Layers,
  Bot,
  Network,
} from "lucide-react";
import { toast } from "sonner";
import { relativeTime } from "../../lib/time";
import { useAppStore, selectSelectedProject } from "../../store/appStore";
import { useProjects } from "../../hooks/useProjects";
import { usePendingInteractions } from "../../store/pendingInteractions";
import * as api from "../../lib/tauri";
import { EditDescription } from "./EditDescription";
import { DecisionsPanel } from "./DecisionsPanel";
import { LoopsPanel } from "./LoopsPanel";
import { EpicsPanel } from "./EpicsPanel";
import { KnowledgeGraphPanel } from "./KnowledgeGraphPanel";
import { AgentPanel } from "./AgentPanel";
import { AskUserQuestionCard } from "./AskUserQuestionCard";
import { PermissionApprovalCard, buildAllowRule } from "./Chat";
import { ConfirmDialog } from "../shared/ConfirmDialog";
import { StatusBadge } from "../shared/StatusBadge";
import { PermissionModeBadge } from "../shared/PermissionModeBadge";
import { PageHeader } from "../layout/AppShell";
import { Section, IconButton, ActionButton } from "./Section";
import { cn } from "../../lib/utils";
import type { AskUserQuestionAnswers, ApprovalDecision, DetailTab, ProjectEntry } from "../../types";

const TABS: { id: DetailTab; label: string; icon: React.ReactNode }[] = [
  { id: "overview", label: "Overview", icon: <LayoutDashboard size={14} /> },
  { id: "decisions", label: "Decisions", icon: <Lightbulb size={14} /> },
  { id: "loops", label: "Loops", icon: <Repeat size={14} /> },
  { id: "epics", label: "Epics", icon: <Layers size={14} /> },
  { id: "graph", label: "Graph", icon: <Network size={14} /> },
  { id: "agent", label: "Agent", icon: <Bot size={14} /> },
];

export function ProjectDetail() {
  const navigate = useNavigate();
  const { projectPath: encodedPath } = useParams({ strict: false }) as { projectPath: string };
  const projectPath = decodeURIComponent(encodedPath);

  const setSelectedProjectPath = useAppStore((s) => s.setSelectedProjectPath);
  const project = useAppStore(selectSelectedProject);
  const activeTab = useAppStore((s) => s.detailTab);
  const setActiveTab = useAppStore((s) => s.setDetailTab);
  const { openInFinder, openInTerminal, removeProject, rescanProject, regenerateDesc, setAutonomous } =
    useProjects();

  const [isEditing, setIsEditing] = useState(false);
  const [showRemoveConfirm, setShowRemoveConfirm] = useState(false);

  // Keep the persisted navigation identifier in sync with the route param. The
  // route is the source of truth for *which* project is open; `project` itself
  // is derived from `projects` (Rust) via `selectSelectedProject`, so it always
  // carries fresh git/run state and resolves to null if the path is no longer
  // registered.
  useEffect(() => {
    setSelectedProjectPath(projectPath);
  }, [projectPath, setSelectedProjectPath]);

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

  const handleRefreshSkills = async () => {
    try {
      const skills = await api.refreshSkills(project.path);
      toast.success(`Refreshed ${skills.length} skill${skills.length === 1 ? "" : "s"}.`);
    } catch (e) {
      toast.error(`Failed to refresh skills: ${String(e)}`);
    }
  };

  const handleToggleAutonomous = (path: string, autonomous: boolean) => {
    setAutonomous(path, autonomous);
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

      {/* Stuck-question callout: tab-agnostic. Shown whenever a LoopDeck-spawned
          agent has an AskUserQuestion parked for this project, regardless of
          which tab is active — so the user can answer it from anywhere in the
          detail view (not just the Agent tab). Mirrors the card the Agent tab
          renders, reading from the same navigation-stable store. */}
      <StuckQuestionCallout projectPath={project.path} />
      <StuckPermissionCallout projectPath={project.path} />

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

        {/* Content panel.
            Overview / Decisions / Loops use a scroll container (their content
            flows like a document). The Agent tab needs an inner-scroll layout
            instead: its Chat surface has its own transcript scroller, so the
            wrapper must be a bounded flex child — NOT an overflow-y-auto
            container, which would give the Chat root an unbounded height and
            break the inner scroll. */}
        {activeTab === "agent" ? (
          <div className="flex min-h-0 min-w-0 flex-1 flex-col p-6">
            <AgentPanel projectPath={project.path} />
          </div>
        ) : (
          <div className="flex-1 min-h-0 min-w-0 overflow-y-auto p-8">
            {activeTab === "overview" && (
              <OverviewTab
                project={project}
                isEditing={isEditing}
                onEdit={() => setIsEditing(true)}
                onCancelEdit={() => setIsEditing(false)}
                onRegenerate={handleRegenerate}
                onRescan={handleRescan}
                onRefreshSkills={handleRefreshSkills}
                onToggleAutonomous={handleToggleAutonomous}
                onFinder={() => openInFinder(project.path)}
                onTerminal={() => openInTerminal(project.path)}
                onRemove={() => setShowRemoveConfirm(true)}
              />
            )}

            {activeTab === "decisions" && (
              <DecisionsPanel projectPath={project.path} />
            )}

            {activeTab === "loops" && <LoopsPanel projectPath={project.path} />}

            {activeTab === "epics" && <EpicsPanel projectPath={project.path} />}

            {activeTab === "graph" && (
              <KnowledgeGraphPanel projectPath={project.path} />
            )}
          </div>
        )}
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

// ── Stuck-question callout ───────────────────────────────────────────────────

/**
 * Tab-agnostic banner that surfaces a pending `AskUserQuestion` for this
 * project, with the same interactive card the Agent tab's Chat renders.
 *
 * The per-project question payload is reconciled into `usePendingInteractions`
 * globally (launch + focus) by `useStuckSessions`; the Agent tab ALSO renders
 * the card when mounted. Both read the same store entry, so answering here
 * clears it everywhere. On submit, the answer is delivered to the parked
 * backend slot via `agentAnswerQuestion` (the same path the Agent tab uses),
 * the local entry is cleared, and a re-reconcile drops it store-wide.
 *
 * Returns null when nothing is pending for this project.
 */
function StuckQuestionCallout({ projectPath }: { projectPath: string }) {
  const pending = usePendingInteractions((s) => s.questions[projectPath] ?? null);
  const clearQuestion = usePendingInteractions((s) => s.clearQuestion);
  if (!pending) return null;

  async function onSubmit(answers: AskUserQuestionAnswers) {
    try {
      await api.agentAnswerQuestion(projectPath, pending.requestId, answers);
    } catch (err) {
      // The turn may have ended while the user deliberated; the backend
      // returns a "no pending question" error in that case. Either way the
      // entry is stale — clear it locally so the banner doesn't linger.
      console.warn("agentAnswerQuestion failed", err);
    }
    clearQuestion(projectPath);
  }

  return (
    <div className="border-b border-amber-500/30 bg-amber-500/5 px-6 py-3">
      <div className="max-h-[45vh] overflow-y-auto">
        <AskUserQuestionCard questions={pending.questions} onSubmit={onSubmit} />
      </div>
    </div>
  );
}

/**
 * Tab-agnostic banner that surfaces a pending manual tool-approval for this
 * project — the permission-side mirror of `StuckQuestionCallout`. Renders the
 * same `PermissionApprovalCard` the Agent tab's Chat uses, so the look is
 * identical and answering here clears it everywhere (both read the same store
 * entry).
 *
 * This closes the recurrence where an approval parked while the user was on
 * another view (or with the Mac locked) was invisible until it silently
 * auto-denied on the 10-min `PARKED_SLOT_TIMEOUT` and surfaced as a generic
 * "Interrupted" bubble. `useStuckSessions` now reconciles permissions
 * cross-project (launch + focus); this callout makes them actionable from any
 * tab, not just Agent.
 *
 * Returns null when nothing is pending for this project.
 */
function StuckPermissionCallout({ projectPath }: { projectPath: string }) {
  const pending = usePendingInteractions((s) => s.permissions[projectPath] ?? null);
  const clearPermission = usePendingInteractions((s) => s.clearPermission);
  if (!pending) return null;

  async function onDecide(decision: ApprovalDecision) {
    // On success, DON'T clear here — wait for the resolved permission_request
    // event to arrive (the same flow the Agent tab uses, so the ⏳ marker
    // becomes ✓/✗ consistently). On IPC error (the turn already ended, no
    // pending approval), the entry is stale — clear it so the banner goes away.
    try {
      await api.agentAnswerPermission(projectPath, pending.requestId, decision);
    } catch (err) {
      console.warn("agentAnswerPermission failed", err);
      clearPermission(projectPath);
    }
  }

  async function onAlwaysAllow() {
    try {
      await api.agentAnswerPermission(projectPath, pending.requestId, {
        allow: true,
      });
      const rule = buildAllowRule(pending.toolName, pending.input);
      await api.agentAddAllowRule(projectPath, rule);
    } catch (err) {
      console.warn("always-allow failed", err);
    }
  }

  return (
    <div className="border-b border-rose-500/30 bg-rose-500/5 px-6 py-3">
      <PermissionApprovalCard
        toolName={pending.toolName}
        input={pending.input}
        onDecide={onDecide}
        onAlwaysAllow={onAlwaysAllow}
      />
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
  onRefreshSkills,
  onToggleAutonomous,
  onFinder,
  onTerminal,
  onRemove,
}: {
  project: ProjectEntry | null;
  isEditing: boolean;
  onEdit: () => void;
  onCancelEdit: () => void;
  onRegenerate: () => void;
  onRescan: () => void;
  /** Re-install managed skills to the current app version. */
  onRefreshSkills: () => void;
  /** Enable autonomous mode. The confirm dialog lives inside OverviewTab —
   *  this fires only after the user confirms. Disabling (back to confirm) is
   *  unconditional and doesn't need confirmation. */
  onToggleAutonomous: (path: string, autonomous: boolean) => void;
  onFinder: () => void;
  onTerminal: () => void;
  onRemove: () => void;
}) {
  const [showAutonomousConfirm, setShowAutonomousConfirm] = useState(false);
  if (!project) return null;
  const hasChanges = project.uncommitted.files > 0;
  const autonomous = project.autonomous ?? false;

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

        <Section label="Agent mode">
          <div className="flex items-center justify-between gap-3">
            <div className="min-w-0">
              <div className="flex items-center gap-2">
                {autonomous ? (
                  <PermissionModeBadge mode="autonomous" />
                ) : (
                  <PermissionModeBadge mode="confirm" />
                )}
              </div>
              <p className="mt-1.5 text-xs leading-relaxed text-muted-foreground">
                {autonomous
                  ? "The agent self-approves tool calls so loops run unattended. The destructive floor still applies. Review the resulting pull requests before merging."
                  : "File edits, commands, and network calls require your approval. Read-only tools run automatically."}
              </p>
            </div>
            {/* Disabling is unconditional; enabling shows the confirm dialog. */}
            <button
              type="button"
              onClick={() => {
                if (autonomous) {
                  onToggleAutonomous(project.path, false);
                } else {
                  setShowAutonomousConfirm(true);
                }
              }}
              role="switch"
              aria-checked={autonomous}
              aria-label="Toggle autonomous mode"
              className={`relative inline-flex h-5 w-9 shrink-0 items-center rounded-full transition-colors ${
                autonomous ? "bg-amber-500" : "bg-muted"
              }`}
            >
              <span
                className={`inline-block size-4 transform rounded-full bg-white shadow transition-transform ${
                  autonomous ? "translate-x-4" : "translate-x-0.5"
                }`}
              />
            </button>
          </div>
        </Section>
      </div>

      <div className="mt-6 flex flex-wrap items-center gap-2">
        <ActionButton icon={RefreshCw} label="Rescan" onClick={onRescan} />
        <ActionButton icon={Sparkles} label="Skills" onClick={onRefreshSkills} />
        <ActionButton icon={FolderOpen} label="Finder" onClick={onFinder} />
        <ActionButton icon={Terminal} label="Terminal" onClick={onTerminal} />
        <ActionButton icon={Trash2} label="Remove" onClick={onRemove} destructive />
      </div>

      {showAutonomousConfirm && (
        <ConfirmDialog
          title="Enable autonomous mode?"
          message={`The agent for "${project.name}" will self-approve tool calls — including file edits, commands, and MCP calls — so loops can run unattended. The destructive floor still applies (rm -rf, force-push, curl|sh, sudo, etc. are still denied). Review the resulting pull requests before merging.`}
          confirmLabel="Enable"
          onConfirm={() => {
            onToggleAutonomous(project.path, true);
            setShowAutonomousConfirm(false);
          }}
          onCancel={() => setShowAutonomousConfirm(false)}
        />
      )}
    </div>
  );
}
