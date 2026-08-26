import type { AskUserQuestionSpec, RunPhase, RunPhaseStatus, RunPlan } from "../types";

// ── Shared run-plan presentation (relocated from RunQueuePanel.tsx) ─────────
// Single source for both run surfaces — RunQueuePanel's phase rows and the
// drawer's night variant — per prd-night-run-surfaces Phase 1's
// "reusing RunQueuePanel.tsx's existing phase-row and parked-question-parsing
// logic rather than re-deriving shapes."

export const STATUS_LABEL: Record<RunPhaseStatus, string> = {
  queued: "queued",
  running: "running",
  parked: "parked",
  completed: "completed",
  failed: "failed",
  interrupted: "interrupted",
  killed: "killed",
};

export const STATUS_COLOR: Record<RunPhaseStatus, string> = {
  queued: "var(--muted-foreground)",
  running: "var(--primary)",
  parked: "var(--warning)",
  completed: "var(--success)",
  failed: "var(--destructive)",
  interrupted: "var(--warning)",
  killed: "var(--destructive)",
};

/** Extract structured AskUserQuestionSpec from a park_payload that contains
 *  a `__QUESTIONS__<json>__END__` marker written by the executor. */
export function parseParkedQuestions(reason?: string): { questions: AskUserQuestionSpec[] | null } {
  if (!reason) return { questions: null };
  const start = reason.indexOf("__QUESTIONS__");
  const end = reason.indexOf("__END__");
  if (start === -1 || end === -1 || end <= start) return { questions: null };
  try {
    // "__QUESTIONS__" is 13 chars. (The original RunQueuePanel copy sliced
    // from start + 14, dropping the JSON's leading "[" — this fix is why the
    // parser now lives in one tested place.)
    const json = reason.slice(start + 13, end);
    const parsed = JSON.parse(json);
    if (Array.isArray(parsed) && parsed.length > 0) {
      return { questions: parsed as AskUserQuestionSpec[] };
    }
  } catch {
    // JSON parse failed — fall through to plain text rendering.
  }
  return { questions: null };
}

// Display-only mirrors of `limits::DEFAULT_RUN_*`
// (src-tauri/src/limits.rs:102-108). `RunBudgets` fields are `Option<u64>`
// where None means "the backend applies these defaults at execute time," not
// "unbounded" — and no IPC exposes the constants (adding one would violate
// prd-night-run-surfaces' Non-Goals), so the gauges hardcode the same numbers
// for display math. MUST stay in sync with limits.rs.
export const DEFAULT_RUN_PHASE_TOKEN_CAP = 500_000;
export const DEFAULT_RUN_PHASE_WALL_CLOCK_SECS = 90 * 60;
export const DEFAULT_RUN_TOTAL_WALL_CLOCK_SECS = 8 * 60 * 60;

/** One of the night variant's three budget gauges, with fill math already
 *  resolved against the plan's caps (falling back to the defaults above). */
export interface BudgetGauge {
  id: "phase-tokens" | "phase-wall-clock" | "run-wall-clock";
  label: string;
  used: number;
  cap: number;
  unit: "tokens" | "secs";
}

/**
 * The 3 budget gauges (prd-night-run-surfaces Phase 1): tokens/phase,
 * wall-clock/phase, total-run wall-clock. Phases execute sequentially, so the
 * per-phase gauges track the worst phase so far — the phase that matters for
 * "am I about to blow a cap" — not the sum. The total gauge reads the plan's
 * own `wall_clock_secs` (run-level, includes parked wait time), not the sum
 * of per-phase clocks.
 */
export function budgetGauges(plan: RunPlan): BudgetGauge[] {
  const worstPhaseTokens = Math.max(0, ...plan.phases.map((p) => p.token_usage));
  const worstPhaseSecs = Math.max(0, ...plan.phases.map((p) => p.wall_clock_secs));
  return [
    {
      id: "phase-tokens",
      label: "Tokens / phase",
      used: worstPhaseTokens,
      cap: plan.budgets.per_phase_token_cap ?? DEFAULT_RUN_PHASE_TOKEN_CAP,
      unit: "tokens",
    },
    {
      id: "phase-wall-clock",
      label: "Wall clock / phase",
      used: worstPhaseSecs,
      cap: plan.budgets.per_phase_wall_clock_secs ?? DEFAULT_RUN_PHASE_WALL_CLOCK_SECS,
      unit: "secs",
    },
    {
      id: "run-wall-clock",
      label: "Total run",
      used: plan.wall_clock_secs,
      cap: plan.budgets.total_run_wall_clock_secs ?? DEFAULT_RUN_TOTAL_WALL_CLOCK_SECS,
      unit: "secs",
    },
  ];
}

/** One currently-parked phase's card model for the night variant's inline
 *  parked-question inbox (prd-night-run-surfaces Phase 1, item 2).
 *  `questions` is non-null when the payload carries structured
 *  `__QUESTIONS__` JSON (answered via `answerParkedQuestion`), null for a
 *  raw-text payload (requeued via `requeueRunPhase`). */
export interface ParkedCard {
  phase: RunPhase;
  questions: AskUserQuestionSpec[] | null;
}

/** The night variant's parked-question inbox: one card per *currently-parked*
 *  phase. Status-gated, not just payload-gated — `park_payload` can persist
 *  on a phase that later completed or was requeued, and those must not render
 *  as open questions. Mirrors RunQueuePanel's "Parked questions" inbox. */
export function parkedInbox(plan: RunPlan): ParkedCard[] {
  return plan.phases
    .filter((p) => p.status === "parked" && p.park_payload)
    .map((p) => ({ phase: p, questions: parseParkedQuestions(p.park_payload).questions }));
}

/** Whether the drawer should auto-select its Agent tab — which the render
 *  below swaps for the night variant — per prd-night-run-surfaces Phase 1
 *  item 3. Fires at most once per continuous drawer-open span per project
 *  (`switchedForPath` latch): a user who navigates to another tab mid-run is
 *  never yanked back, while a run that *starts* (or is first detected) while
 *  the drawer is already open still triggers the one-time switch. The signal
 *  is the same `hasActiveOrQueuedRun`-gated plan the rail doors' moon badge
 *  uses — per the detail-drawer spike (ADR-3), "night run" is a derived
 *  run-plan flag, not a new `RunState` variant. */
export function shouldAutoSelectNightVariant(args: {
  drawerOpen: boolean;
  projectPath: string | null;
  /** Non-null only while the project has an active/queued run plan. */
  nightPlan: RunPlan | null;
  activeTopLevelTab: string;
  switchedForPath: string | null;
}): boolean {
  if (!args.drawerOpen || !args.projectPath || !args.nightPlan) return false;
  if (args.switchedForPath === args.projectPath) return false;
  return args.activeTopLevelTab !== "agent";
}

/** Fill percentage for a gauge bar, clamped to [0, 100] — a blown cap pins
 *  the bar full rather than overflowing the track. */
export function gaugePercent(used: number, cap: number): number {
  if (cap <= 0) return 0;
  return Math.min(100, (used / cap) * 100);
}

/** `1_234_567` → `"1,234,567"`. */
export function formatTokens(n: number): string {
  return n.toLocaleString();
}

/** Seconds → `"Xh Ym"` past an hour, else `"Xm Ys"`, else `"Ys"`. */
export function formatDuration(secs: number): string {
  if (secs >= 3600) return `${Math.floor(secs / 3600)}h ${Math.floor((secs % 3600) / 60)}m`;
  if (secs >= 60) return `${Math.floor(secs / 60)}m ${secs % 60}s`;
  return `${secs}s`;
}
