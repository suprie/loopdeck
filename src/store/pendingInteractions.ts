import { create } from "zustand";
import type {
  AskUserQuestionSpec,
  PendingPermissionEntry,
  PendingPlanEntry,
  PendingQuestionEntry,
} from "../types";

/**
 * The payload of a pending manual tool approval, surfaced as an Allow/Deny card.
 *
 * Set when a `permission_request` event with `decision: "pending"` arrives;
 * cleared when the user answers, when the resolved allow/deny event arrives,
 * or when the turn ends. Mirrors the Rust `ClaudeEvent::PermissionRequest`
 * pending-variant fields.
 */
export interface PendingPermission {
  requestId: string;
  toolName: string;
  input: string;
}

/**
 * The payload of a pending `AskUserQuestion`, surfaced as a question card.
 *
 * Set when an `ask_user_question` event arrives; cleared on submit or when the
 * turn ends. Mirrors the Rust `ClaudeEvent::AskUserQuestion` fields.
 */
export interface PendingQuestion {
  requestId: string;
  questions: AskUserQuestionSpec[];
}

/**
 * The payload of a pending `ExitPlanMode` request, surfaced as a plan-approval
 * card.
 *
 * Set when a `plan_approval` event with `decision: "pending"` arrives;
 * cleared when the user answers, when the resolved allow/deny event arrives,
 * or when the turn ends. Mirrors the Rust `ClaudeEvent::PlanApproval`
 * pending-variant fields.
 */
export interface PendingPlan {
  requestId: string;
  plan: string;
}

/**
 * Per-project pending interactions (manual approvals + AskUserQuestion prompts).
 *
 * WHY THIS STORE EXISTS:
 * `AgentPanel` previously held `pendingPermission`/`pendingQuestion` in local
 * `useState`. Navigating away from the Agent tab unmounted `AgentPanel`,
 * destroying that state — so coming back, the approval/question card was gone
 * even though the backend was still parked waiting for an answer (see the
 * "moved to other page and back, pop up not shown" bug). The backend's per-
 * project oneshot slots (`pending_permissions`/`pending_answers`) survive
 * independently of the frontend, so lifting the *display payload* out of the
 * component and into this navigation-stable store restores the card on remount.
 *
 * Keyed by project path so different projects' prompts can't collide. Not
 * persisted to localStorage (unlike `appStore`): pending prompts are
 * session-scoped, and serializing large question option arrays would bloat
 * storage for no benefit.
 *
 * STALENESS: if a turn ends while the user is away (timeout, or the answer was
 * delivered and the turn completed), the store entry lingers until the next
 * remount or answer attempt. The backend's `agent_answer_*` commands return a
 * "no pending question/approval" error in that case, which the caller surfaces
 * by clearing the entry + showing a banner — so staleness degrades gracefully
 * rather than wedging the UI.
 */
interface PendingInteractionState {
  /** Per-project pending approval, keyed by absolute project path. */
  permissions: Record<string, PendingPermission>;
  /** Per-project pending question, keyed by absolute project path. */
  questions: Record<string, PendingQuestion>;
  /** Per-project pending plan approval, keyed by absolute project path. */
  plans: Record<string, PendingPlan>;

  // ── Actions ──
  setPermission: (path: string, pending: PendingPermission) => void;
  clearPermission: (path: string) => void;
  setQuestion: (path: string, pending: PendingQuestion) => void;
  clearQuestion: (path: string) => void;
  setPlan: (path: string, pending: PendingPlan) => void;
  clearPlan: (path: string) => void;
  /**
   * Replace the entire `questions` map from a fresh backend snapshot
   * (`listPendingQuestions`). Entries absent from the snapshot are stale (the
   * turn ended or was answered elsewhere) and are dropped, so a prompt that
   * resolved while the user was away doesn't linger. Returns the set of paths
   * that are newly-pending (were absent before, now present) so the caller can
   * fire a one-time toast per prompt rather than on every reconcile.
   */
  reconcileQuestions: (
    entries: PendingQuestionEntry[],
  ) => string[];
  /**
   * Replace the entire `permissions` map from a fresh backend snapshot
   * (`listPendingPermissions`). The permission-side mirror of
   * `reconcileQuestions`: entries absent from the snapshot are stale and
   * dropped, and the set of newly-pending paths is returned so the caller can
   * fire a one-time toast per approval rather than on every reconcile.
   */
  reconcilePermissions: (
    entries: PendingPermissionEntry[],
  ) => string[];
  /**
   * Replace the entire `plans` map from a fresh backend snapshot
   * (`listPendingPlans`). The plan-side mirror of `reconcilePermissions`.
   */
  reconcilePlans: (entries: PendingPlanEntry[]) => string[];
}

export const usePendingInteractions = create<PendingInteractionState>((set) => ({
  permissions: {},
  questions: {},
  plans: {},

  setPermission: (path, pending) =>
    set((s) => ({ permissions: { ...s.permissions, [path]: pending } })),

  clearPermission: (path) =>
    set((s) => {
      if (!s.permissions[path]) return s;
      const next = { ...s.permissions };
      delete next[path];
      return { permissions: next };
    }),

  setQuestion: (path, pending) =>
    set((s) => ({ questions: { ...s.questions, [path]: pending } })),

  clearQuestion: (path) =>
    set((s) => {
      if (!s.questions[path]) return s;
      const next = { ...s.questions };
      delete next[path];
      return { questions: next };
    }),

  setPlan: (path, pending) =>
    set((s) => ({ plans: { ...s.plans, [path]: pending } })),

  clearPlan: (path) =>
    set((s) => {
      if (!s.plans[path]) return s;
      const next = { ...s.plans };
      delete next[path];
      return { plans: next };
    }),

  reconcileQuestions: (entries) => {
    // Compute the fresh map and the newly-stuck paths BEFORE mutating, since
    // Zustand's `set` returns void. Reading current state via `get()` (set up
    // below) lets us diff without a second subscription.
    const prev = usePendingInteractions.getState().questions;
    const next: Record<string, PendingQuestion> = {};
    const newlyStuck: string[] = [];
    for (const e of entries) {
      next[e.path] = { requestId: e.requestId, questions: e.questions };
      if (!prev[e.path]) newlyStuck.push(e.path);
    }
    set({ questions: next });
    return newlyStuck;
  },
  reconcilePermissions: (entries) => {
    // Permission-side mirror of `reconcileQuestions`. Same diff-before-mutate
    // shape: build the fresh map, record newly-pending paths, then swap.
    const prev = usePendingInteractions.getState().permissions;
    const next: Record<string, PendingPermission> = {};
    const newlyStuck: string[] = [];
    for (const e of entries) {
      next[e.path] = {
        requestId: e.requestId,
        toolName: e.toolName,
        input: e.input,
      };
      if (!prev[e.path]) newlyStuck.push(e.path);
    }
    set({ permissions: next });
    return newlyStuck;
  },
  reconcilePlans: (entries) => {
    // Plan-side mirror of `reconcilePermissions`. Same diff-before-mutate
    // shape: build the fresh map, record newly-pending paths, then swap.
    const prev = usePendingInteractions.getState().plans;
    const next: Record<string, PendingPlan> = {};
    const newlyStuck: string[] = [];
    for (const e of entries) {
      next[e.path] = { requestId: e.requestId, plan: e.plan };
      if (!prev[e.path]) newlyStuck.push(e.path);
    }
    set({ plans: next });
    return newlyStuck;
  },
}));
