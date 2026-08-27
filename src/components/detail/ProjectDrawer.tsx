import { useEffect, useRef, useState } from "react";
import {
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
  Moon,
} from "lucide-react";
import { toast } from "sonner";
import { relativeTime } from "../../lib/time";
import { useAppStore, selectSelectedProject } from "../../store/appStore";
import { useProjects } from "../../hooks/useProjects";
import { useRunStatus } from "../../hooks/useRunStatus";
import { hasActiveOrQueuedRun } from "../../lib/rail";
import { shouldAutoSelectNightVariant, hasQueueablePhases } from "../../lib/nightRun";
import { usePendingInteractions } from "../../store/pendingInteractions";
import * as api from "../../lib/tauri";
import { EditDescription } from "./EditDescription";
import { DecisionsPanel } from "./DecisionsPanel";
import { LoopsPanel } from "./LoopsPanel";
import { EpicsPanel } from "./EpicsPanel";
import { KnowledgeGraphPanel } from "./KnowledgeGraphPanel";
import { AgentPanel } from "./AgentPanel";
import { NightRunTab } from "./NightRunTab";
import { AskUserQuestionCard } from "./AskUserQuestionCard";
import { PermissionApprovalCard, PlanApprovalCard, buildAllowRule } from "./Chat";
import { ConfirmDialog } from "../shared/ConfirmDialog";
import { StatusBadge } from "../shared/StatusBadge";
import { PermissionModeBadge } from "../shared/PermissionModeBadge";
import { Section, IconButton, ActionButton } from "./Section";
import { Sheet, SheetContent, SheetHeader, SheetTitle, SheetDescription } from "../ui/sheet";
import { Tabs, TabsList, TabsTrigger, TabsContent } from "../ui/tabs";
import { cn } from "../../lib/utils";
import type {
  AskUserQuestionAnswers,
  ApprovalDecision,
  DetailTab,
  PlanApprovalDecision,
  ProjectEntry,
} from "../../types";

/** The drawer's 4 top-level tabs, per `prd-detail-drawer` Phase 2. Epics and
 *  Graph (both former top-level tabs) fold into Loops and Decisions
 *  respectively as nested sub-tabs — see the Phase 1 spike ADR in
 *  `docs/epics/selasar-revamp/README.md`. */
type TopTab = "overview" | "agent" | "loops" | "decisions";

const TABS: { id: TopTab; label: string; icon: React.ReactNode }[] = [
  { id: "overview", label: "Overview", icon: <LayoutDashboard size={14} /> },
  { id: "agent", label: "Agent", icon: <Bot size={14} /> },
  { id: "loops", label: "Loops", icon: <Repeat size={14} /> },
  { id: "decisions", label: "Decisions", icon: <Lightbulb size={14} /> },
];

/** Maps the persisted `DetailTab` (which still carries the legacy `"epics"`/
 *  `"graph"` values for deep-selecting a sub-tab, e.g. from
 *  `useProjects.createProject`) onto one of the drawer's 4 top-level tabs. */
function topLevelTab(tab: DetailTab): TopTab {
  if (tab === "epics") return "loops";
  if (tab === "graph") return "decisions";
  return tab;
}

/**
 * Right-side overlay drawer — the project detail surface. Replaces the old
 * routed, full-page `/project/$projectPath` view. Per the `prd-detail-drawer`
 * Phase 1 spike ADR, this is pure UI state (`appStore.drawerOpen` +
 * `selectedProjectPath`), not URL/route-backed: this app's router runs on
 * in-memory history with no visible address bar, so route-backing bought no
 * real bookmark/deep-link/back-button benefit, and `selectedProjectPath` was
 * already a store field decoupled from routing before this change.
 *
 * Built on the (previously unused) shadcn `Sheet` primitive
 * (`@radix-ui/react-dialog` under the hood) — modal by default, which gives
 * focus trapping, Escape-to-close, and body-scroll-lock (the corridor
 * "freeze" behavior) for free, no hand-rolled overlay code needed.
 */
export function ProjectDrawer() {
  const drawerOpen = useAppStore((s) => s.drawerOpen);
  const setDrawerOpen = useAppStore((s) => s.setDrawerOpen);
  const project = useAppStore(selectSelectedProject);
  const activeTab = useAppStore((s) => s.detailTab);
  const setActiveTab = useAppStore((s) => s.setDetailTab);
  const { openInFinder, openInTerminal, removeProject, rescanProject, regenerateDesc, setAutonomous } =
    useProjects();

  const [isEditing, setIsEditing] = useState(false);
  const [showRemoveConfirm, setShowRemoveConfirm] = useState(false);

  // Night-variant selection (prd-night-run-surfaces Phase 1): while the
  // project has a run in flight or queued, the Agent tab swaps for the night
  // variant (phase-chip rail + budget gauges). Derived from the same
  // getRunStatus poll the rail doors use — no new backend concept.
  const runStatus = useRunStatus(drawerOpen && project ? project.path : null);
  const nightPlan =
    runStatus && hasActiveOrQueuedRun(runStatus) && runStatus.plan ? runStatus.plan : null;

  // prd-night-run-surfaces Phase 1 item 3: while the project has an
  // active/queued run, the drawer auto-selects the Agent tab (swapped for the
  // night variant below) — so opening a moon-badged rail door lands on the
  // night variant, not whatever tab was last persisted. Once per continuous
  // drawer-open span per project (`shouldAutoSelectNightVariant`'s latch): a
  // user who navigates away mid-run isn't yanked back, and closing the drawer
  // (or switching project) re-arms it.
  const switchedForPath = useRef<string | null>(null);
  useEffect(() => {
    if (!drawerOpen) {
      switchedForPath.current = null;
      return;
    }
    const projectPath = project?.path ?? null;
    if (
      shouldAutoSelectNightVariant({
        drawerOpen,
        projectPath,
        nightPlan,
        activeTopLevelTab: topLevelTab(activeTab),
        switchedForPath: switchedForPath.current,
      })
    ) {
      setActiveTab("agent");
    }
    // Latch even when already on the Agent tab — "auto-switched" means "this
    // open already showed the night variant", which is also true if the user
    // landed there themselves.
    if (nightPlan && projectPath) switchedForPath.current = projectPath;
  }, [drawerOpen, project, nightPlan, activeTab, setActiveTab]);

  const handleRemove = () => {
    if (!project) return;
    removeProject(project.path);
    setShowRemoveConfirm(false);
  };

  const handleRegenerate = async () => {
    if (!project) return;
    await regenerateDesc(project.path);
  };

  const handleRescan = async () => {
    if (!project) return;
    await rescanProject(project.path);
  };

  const handleRefreshSkills = async () => {
    if (!project) return;
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
    <Sheet open={drawerOpen && !!project} onOpenChange={(open) => !open && setDrawerOpen(false)}>
      <SheetContent
        side="right"
        className="flex h-full w-[760px] max-w-[92vw] flex-col gap-0 overflow-hidden p-0 sm:max-w-[760px]"
      >
        {project && (
          <>
            <SheetHeader className="shrink-0 border-b border-border px-6 py-5 pr-12 text-left">
              <div className="flex items-start justify-between gap-3">
                <div className="min-w-0">
                  <SheetTitle className="font-display text-lg">{project.name}</SheetTitle>
                  <SheetDescription className="mt-0.5 font-mono text-[11px]">
                    {project.path}
                  </SheetDescription>
                  {project.description && (
                    <p className="mt-2 text-xs leading-relaxed text-muted-foreground">
                      {project.description}
                    </p>
                  )}
                </div>
                <div className="flex shrink-0 items-center gap-2">
                  <PlanTonightButton projectPath={project.path} />
                  <StatusBadge status={project.status} />
                </div>
              </div>
            </SheetHeader>

            {/* Stuck-question callout: tab-agnostic. Shown whenever a
                Selasar-spawned agent has an AskUserQuestion parked for this
                project, regardless of which tab is active. */}
            <StuckQuestionCallout projectPath={project.path} />
            <StuckPermissionCallout projectPath={project.path} />
            <StuckPlanCallout projectPath={project.path} />

            {/* Top-level tab rail */}
            <nav className="flex shrink-0 items-center gap-1 border-b border-border px-4">
              {TABS.map((tab) => {
                const active = topLevelTab(activeTab) === tab.id;
                return (
                  <button
                    key={tab.id}
                    onClick={() => setActiveTab(tab.id)}
                    className={cn(
                      "flex items-center gap-1.5 border-b-2 px-3 py-2.5 text-sm transition-colors",
                      active
                        ? "border-primary font-medium text-foreground"
                        : "border-transparent text-muted-foreground hover:text-foreground",
                    )}
                  >
                    {tab.icon}
                    {tab.label}
                  </button>
                );
              })}
            </nav>

            {/* Content panel.
                Overview / Loops / Decisions use a scroll container. The
                Agent tab needs an inner-scroll layout instead: its Chat
                surface has its own transcript scroller, so the wrapper must
                be a bounded flex child — NOT an overflow-y-auto container,
                which would give the Chat root an unbounded height and break
                the inner scroll. */}
            {topLevelTab(activeTab) === "agent" ? (
              <div className="flex min-h-0 min-w-0 flex-1 flex-col p-6">
                {nightPlan ? (
                  <NightRunTab projectPath={project.path} plan={nightPlan} />
                ) : (
                  <AgentPanel projectPath={project.path} />
                )}
              </div>
            ) : (
              <div className="flex-1 min-h-0 min-w-0 overflow-y-auto p-6">
                {topLevelTab(activeTab) === "overview" && (
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

                {topLevelTab(activeTab) === "loops" && (
                  <LoopsTabContent
                    projectPath={project.path}
                    focusEpics={activeTab === "epics"}
                  />
                )}

                {topLevelTab(activeTab) === "decisions" && (
                  <DecisionsTabContent
                    projectPath={project.path}
                    focusGraph={activeTab === "graph"}
                  />
                )}
              </div>
            )}
          </>
        )}
      </SheetContent>

      {project && showRemoveConfirm && (
        <ConfirmDialog
          title="Remove Project"
          message={`Remove "${project.name}" from the registry? The .loopdeck folder and project files will NOT be deleted.`}
          confirmLabel="Remove"
          onConfirm={handleRemove}
          onCancel={() => setShowRemoveConfirm(false)}
          danger
        />
      )}
    </Sheet>
  );
}

// ── Loops tab: nested Loops / Epics sub-tabs ─────────────────────────────────

/**
 * Epics tab content (`EpicsPanel.tsx`, incl. `RunQueuePanel`) relocates here
 * as a nested sub-tab per the Phase 1 spike ADR — reused unchanged, not
 * redesigned.
 */
function LoopsTabContent({
  projectPath,
  focusEpics,
}: {
  projectPath: string;
  focusEpics: boolean;
}) {
  return (
    <Tabs defaultValue={focusEpics ? "epics" : "loops"}>
      <TabsList>
        <TabsTrigger value="loops">
          <Repeat size={13} className="mr-1.5" />
          Loops
        </TabsTrigger>
        <TabsTrigger value="epics">
          <Layers size={13} className="mr-1.5" />
          Epics
        </TabsTrigger>
      </TabsList>
      <TabsContent value="loops">
        <LoopsPanel projectPath={projectPath} />
      </TabsContent>
      <TabsContent value="epics">
        <EpicsPanel projectPath={projectPath} />
      </TabsContent>
    </Tabs>
  );
}

// ── Decisions tab: nested Decisions / Graph sub-tabs ─────────────────────────

/**
 * Graph tab content (`KnowledgeGraphPanel.tsx`) relocates here as a nested
 * sub-tab per the Phase 1 spike ADR — reused unchanged, not redesigned.
 */
function DecisionsTabContent({
  projectPath,
  focusGraph,
}: {
  projectPath: string;
  focusGraph: boolean;
}) {
  return (
    <Tabs defaultValue={focusGraph ? "graph" : "decisions"}>
      <TabsList>
        <TabsTrigger value="decisions">
          <Lightbulb size={13} className="mr-1.5" />
          Decisions
        </TabsTrigger>
        <TabsTrigger value="graph">
          <Network size={13} className="mr-1.5" />
          Graph
        </TabsTrigger>
      </TabsList>
      <TabsContent value="decisions">
        <DecisionsPanel projectPath={projectPath} />
      </TabsContent>
      <TabsContent value="graph">
        <KnowledgeGraphPanel projectPath={projectPath} />
      </TabsContent>
    </Tabs>
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
    <div className="shrink-0 border-b border-amber-500/30 bg-amber-500/5 px-6 py-3">
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
    <div className="shrink-0 border-b border-rose-500/30 bg-rose-500/5 px-6 py-3">
      <PermissionApprovalCard
        toolName={pending.toolName}
        input={pending.input}
        onDecide={onDecide}
        onAlwaysAllow={onAlwaysAllow}
      />
    </div>
  );
}

function StuckPlanCallout({ projectPath }: { projectPath: string }) {
  const pending = usePendingInteractions((s) => s.plans[projectPath] ?? null);
  const clearPlan = usePendingInteractions((s) => s.clearPlan);
  if (!pending) return null;

  async function onDecide(decision: PlanApprovalDecision) {
    try {
      await api.agentAnswerPlan(projectPath, pending.requestId, decision);
    } catch (err) {
      console.warn("agentAnswerPlan failed", err);
      clearPlan(projectPath);
    }
  }

  return (
    <div className="shrink-0 border-b border-sky-500/30 bg-sky-500/5 px-6 py-3">
      <PlanApprovalCard plan={pending.plan} onDecide={onDecide} />
    </div>
  );
}

// ── Plan-tonight entry point ─────────────────────────────────────────────────

/**
 * "Plan tonight" button in the drawer header (prd-night-run-surfaces Phase 2
 * item 3). Gated on the project having queueable PRD phases — the same
 * `!done && !noId` gate EpicsPanel's overnight-run picker checkboxes use, via
 * the shared `hasQueueablePhases`.
 *
 * The 3-step wizard this opens is this phase's items 1-2 (separate loops);
 * until it exists the button only holds local open/close state, per the
 * pre-answered clarification in the run plan: no modal content yet. Renders
 * null when nothing is queueable.
 */
function PlanTonightButton({ projectPath }: { projectPath: string }) {
  const [open, setOpen] = useState(false);
  const [queueable, setQueueable] = useState(false);

  useEffect(() => {
    let cancelled = false;
    api.getEpics(projectPath)
      .then((epics) => {
        if (!cancelled) setQueueable(hasQueueablePhases(epics));
      })
      .catch(() => {
        // Gate closed on fetch failure — the entry point simply doesn't show.
        if (!cancelled) setQueueable(false);
      });
    return () => {
      cancelled = true;
    };
  }, [projectPath]);

  if (!queueable) return null;

  return (
    <button
      type="button"
      onClick={() => setOpen((v) => !v)}
      aria-expanded={open}
      title="Plan an overnight run"
      className={cn(
        "flex items-center gap-1.5 rounded-md border px-2.5 py-1 text-[11px] font-medium transition-colors",
        open
          ? "border-[var(--primary)] text-[var(--primary)]"
          : "border-border text-foreground hover:bg-accent",
      )}
    >
      <Moon size={12} />
      Plan tonight
    </button>
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
