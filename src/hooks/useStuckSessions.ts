import { useCallback } from "react";
import * as api from "../lib/tauri";
import { usePendingInteractions } from "../store/pendingInteractions";

/**
 * Reconcile "stuck" parked prompts across the whole registry —
 * `AskUserQuestion` prompts, manual tool approvals, AND plan approvals
 * (`ExitPlanMode`).
 *
 * A Selasar-spawned agent parks one oneshot per pending prompt in
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
 * Opens the stuck project's drawer via the Zustand store directly (not
 * `useNavigate`) so it works from `App.tsx`, which sits above
 * `RouterProvider` and therefore outside router context — and because the
 * drawer is pure UI state, not a route, per the `prd-detail-drawer` Phase 1
 * spike ADR. Wired in `App.tsx` to run on mount and on `window` focus. No
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

    // The drawer callout and dashboard attention list are persistent and
    // sufficient. Reconcile them without raising an interruption-style popup.
    const interactions = usePendingInteractions.getState();
    interactions.reconcileQuestions(questions);
    interactions.reconcilePermissions(permissions);
    interactions.reconcilePlans(plans);
  }, []);

  return { reconcileStuckSessions };
}
