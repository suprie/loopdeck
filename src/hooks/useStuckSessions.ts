import { useCallback } from "react";
import { toast } from "sonner";
import * as api from "../lib/tauri";
import { router } from "../router";
import { useAppStore } from "../store/appStore";
import { usePendingInteractions } from "../store/pendingInteractions";

/**
 * Sonner's default `duration` (~4s) auto-dismisses the toast — but a stuck
 * prompt is, by definition, something the user hasn't seen yet (they were on
 * another view, or the Mac was locked). Letting the only cross-view nudge
 * vanish after 4s defeats its purpose. Pin these to `Infinity` (Sonner's
 * documented "never auto-dismiss" value) so the toast stays on screen until
 * the user clicks Answer/Approve or dismisses it. The card pill and the
 * detail-view callout already persist until answered; this matches them.
 */
const STUCK_TOAST_DURATION = Infinity;

/**
 * Reconcile "stuck" parked prompts across the whole registry —
 * `AskUserQuestion` prompts, manual tool approvals, AND plan approvals
 * (`ExitPlanMode`).
 *
 * A LoopDeck-spawned agent parks one oneshot per pending prompt in
 * `AppState.pending_answers` / `AppState.pending_permissions` /
 * `AppState.pending_plans`; the per-project cards (`AgentPanel`) only
 * reconcile their own project's slot when its tab mounts. So if the user is
 * on another view — or the Mac was locked / focus moved away when the prompt
 * arrived — the card never renders and the agent freezes silently. For
 * approvals that meant a 10-minute auto-deny on `PARKED_SLOT_TIMEOUT`,
 * surfacing as a generic "Interrupted" bubble the user couldn't explain (the
 * recurring bug this hook was extended to close).
 *
 * This hook closes that gap. `reconcileStuckSessions` pulls every pending
 * prompt of all three kinds from the backend (`listPendingQuestions` +
 * `listPendingPermissions` + `listPendingPlans`), writes them into the
 * navigation-stable `usePendingInteractions` store (replacing the whole map
 * so resolved prompts are dropped), and fires **one** toast per prompt that
 * is newly-detected
 * since the last reconcile — not on every focus.
 *
 * Uses the exported `router` instance (not `useNavigate`) so it works from
 * `App.tsx`, which sits above `RouterProvider` and therefore outside router
 * context. Wired in `App.tsx` to run on mount and on `window` focus. No
 * background polling: the triggers are launch, focus, and manual refresh only.
 */
export function useStuckSessions() {
  /**
   * Fetch all pending prompts (questions + approvals), reconcile the store,
   * and toast the new ones. Safe to call repeatedly; idempotent apart from the
   * one-time-per-prompt toast. Errors are swallowed (console.warn) so a
   * transient backend hiccup never breaks the focus listener.
   */
  const reconcileStuckSessions = useCallback(async () => {
    // Fetch all three kinds in parallel — they're independent backend reads. A
    // failure of any is swallowed so the others still reconcile.
    const [questions, permissions, plans] = await Promise.all([
      api.listPendingQuestions().catch((err) => {
        console.warn("listPendingQuestions failed", err);
        return [];
      }),
      api.listPendingPermissions().catch((err) => {
        console.warn("listPendingPermissions failed", err);
        return [];
      }),
      api.listPendingPlans().catch((err) => {
        console.warn("listPendingPlans failed", err);
        return [];
      }),
    ]);

    // Resolve project names for the toast labels. The registry is already
    // loaded by useProjects on mount; fall back to the raw path.
    const projects = useAppStore.getState().projects;
    const nameFor = (path: string) =>
      projects.find((p) => p.path === path)?.name ?? path;

    // Helper to navigate the user to the stuck project's detail view, where
    // the tab-agnostic callouts (question + approval) render regardless of
    // which tab is active.
    const goTo = (path: string) =>
      router.navigate({
        to: "/project/$projectPath",
        params: { projectPath: encodeURIComponent(path) },
      });

    // ── Questions ──
    const newlyStuckQuestions = usePendingInteractions
      .getState()
      .reconcileQuestions(questions);
    for (const path of newlyStuckQuestions) {
      const entry = questions.find((e) => e.path === path);
      const firstQ = entry?.questions[0]?.question;
      toast.warning(`Agent in ${nameFor(path)} is waiting for your answer`, {
        description: firstQ,
        duration: STUCK_TOAST_DURATION,
        action: { label: "Answer", onClick: () => goTo(path) },
      });
    }

    // ── Permissions ──
    const newlyStuckPermissions = usePendingInteractions
      .getState()
      .reconcilePermissions(permissions);
    for (const path of newlyStuckPermissions) {
      const entry = permissions.find((e) => e.path === path);
      // Tool name + a short snippet of the input so the user can tell at a
      // glance what's being asked (e.g. "Bash: npm install …"). Capped to keep
      // the toast body to one line.
      const tool = entry?.toolName ?? "tool";
      const inputSnippet = (entry?.input ?? "").trim().slice(0, 120);
      toast.warning(`Agent in ${nameFor(path)} needs your approval`, {
        description: inputSnippet ? `${tool}: ${inputSnippet}` : tool,
        duration: STUCK_TOAST_DURATION,
        action: { label: "Approve", onClick: () => goTo(path) },
      });
    }

    // ── Plan approvals ──
    const newlyStuckPlans = usePendingInteractions.getState().reconcilePlans(plans);
    for (const path of newlyStuckPlans) {
      const entry = plans.find((e) => e.path === path);
      const planSnippet = (entry?.plan ?? "").trim().slice(0, 120);
      toast.warning(`Agent in ${nameFor(path)} has a plan ready for review`, {
        description: planSnippet || undefined,
        duration: STUCK_TOAST_DURATION,
        action: { label: "Review", onClick: () => goTo(path) },
      });
    }
  }, []);

  return { reconcileStuckSessions };
}
