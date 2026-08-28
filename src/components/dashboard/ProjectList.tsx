import { ChevronRight, Sunrise } from "lucide-react";
import type { ProjectEntry } from "../../types";
import { relativeTime } from "../../lib/time";
import { cn } from "../../lib/utils";
import type { RunIndicators } from "../../hooks/useRunIndicators";

interface ProjectListProps {
  projects: ProjectEntry[];
  onSelect: (project: ProjectEntry) => void;
  /** Per-path run badges (night/morning), from the Dashboard's shared poll.
   *  Only the morning flag is rendered here — the room card's state pill
  //  already covers in-flight work; the sunrise chip is the "morning report
  //  ready" attention signal (prd-night-run-surfaces Phase 3, item 2). */
  runIndicators?: Record<string, RunIndicators>;
}

// ── Per-project gradient accents ────────────────────────────────────────────
// Deterministic hash of the project name → one of six gradient pairs, so each
// project gets a stable monogram colour identity.
const ACCENT_PAIRS: ReadonlyArray<[string, string]> = [
  ["oklch(0.7 0.19 295)", "oklch(0.72 0.18 340)"], // violet → magenta
  ["oklch(0.72 0.18 200)", "oklch(0.75 0.17 160)"], // cyan → green
  ["oklch(0.72 0.16 60)", "oklch(0.75 0.18 30)"], // amber → red
  ["oklch(0.65 0.2 255)", "oklch(0.7 0.18 295)"], // blue → violet
  ["oklch(0.75 0.15 75)", "oklch(0.72 0.19 25)"], // yellow → red
  ["oklch(0.72 0.16 155)", "oklch(0.74 0.14 200)"], // green → cyan
];

function accentsFor(name: string): [string, string] {
  let hash = 0;
  for (let i = 0; i < name.length; i++) {
    hash = name.charCodeAt(i) + ((hash << 5) - hash);
  }
  return ACCENT_PAIRS[Math.abs(hash) % ACCENT_PAIRS.length];
}

// ── State pill + focus-line derivation ──────────────────────────────────────

type PillTone = "waiting" | "working" | "ready" | "idle";

/** Next-steps items not yet checked off. A fully-completed checklist isn't "Ready". */
function remainingSteps(p: ProjectEntry): number {
  return p.next_steps_total - p.next_steps_done;
}

function statePill(p: ProjectEntry): { label: string; tone: PillTone } {
  if (p.run_state === "waiting") return { label: "Waiting on you", tone: "waiting" };
  if (p.run_state === "working") return { label: "Working", tone: "working" };
  if (remainingSteps(p) > 0) return { label: "Ready", tone: "ready" };
  return { label: "Idle", tone: "idle" };
}

const PILL_CLASS: Record<PillTone, string> = {
  waiting: "text-destructive bg-[color-mix(in_oklab,var(--destructive)_13%,transparent)]",
  working: "text-primary bg-[color-mix(in_oklab,var(--primary)_13%,transparent)]",
  ready: "text-success bg-[color-mix(in_oklab,var(--success)_13%,transparent)]",
  idle: "text-muted-foreground bg-muted",
};

function focusMeta(p: ProjectEntry): string {
  if (p.run_state === "waiting") {
    return p.next_steps_total > 0
      ? `${p.next_steps_done} of ${p.next_steps_total} steps complete`
      : "Waiting for your response";
  }
  if (p.run_state === "working") {
    return p.uncommitted.files > 0
      ? `Agent changed ${p.uncommitted.files} file${p.uncommitted.files === 1 ? "" : "s"}`
      : "Agent is working";
  }
  const remaining = remainingSteps(p);
  if (remaining > 0) {
    return `${remaining} next step${remaining === 1 ? "" : "s"}`;
  }
  return p.last_opened ? `Opened ${relativeTime(p.last_opened)}` : "Never opened";
}

// ── Component ────────────────────────────────────────────────────────────────

export function ProjectList({ projects, onSelect, runIndicators }: ProjectListProps) {
  return (
    <div className="overflow-hidden rounded-2xl border border-border bg-card">
      <div className="hidden grid-cols-[minmax(178px,0.85fr)_minmax(220px,1.18fr)_128px_18px] items-center gap-4 border-b border-border px-6 py-3 text-[10px] font-semibold uppercase tracking-wider text-muted-foreground/70 md:grid">
        <span>Project</span>
        <span>Current focus</span>
        <span>State</span>
        <span />
      </div>

      {projects.map((project) => {
        const [accentFrom, accentTo] = accentsFor(project.name);
        const monogram = project.name.charAt(0).toUpperCase() || "?";
        const pill = statePill(project);
        const meta = focusMeta(project);
        const title = project.current_loop || "No current loop";
        const morningReady = runIndicators?.[project.path]?.morning ?? false;

        return (
          <article
            key={project.path}
            role="button"
            tabIndex={0}
            onClick={() => onSelect(project)}
            onKeyDown={(e) => {
              if (e.key === "Enter" || e.key === " ") {
                e.preventDefault();
                onSelect(project);
              }
            }}
            className="grid cursor-pointer grid-cols-[1fr_auto] items-center gap-3 border-t border-border/70 px-5 py-4 transition-colors first:border-t-0 hover:bg-accent/40 md:grid-cols-[minmax(178px,0.85fr)_minmax(220px,1.18fr)_128px_18px] md:gap-4 md:px-6"
          >
            <div className="flex min-w-0 items-center gap-3">
              <div
                className="grid size-9 shrink-0 place-items-center rounded-[10px] text-sm font-bold text-white"
                style={{ background: `linear-gradient(135deg, ${accentFrom}, ${accentTo})` }}
              >
                {monogram}
              </div>
              <div className="min-w-0">
                <div className="truncate font-display text-sm font-semibold leading-tight">{project.name}</div>
                <div className="mt-1 truncate font-mono text-[11px] text-muted-foreground">
                  {project.path}
                </div>
              </div>
            </div>

            <div className="col-span-2 min-w-0 pl-12 md:col-span-1 md:pl-0">
              <div className="truncate text-sm leading-snug">{title}</div>
              <div className="mt-1 truncate text-xs text-muted-foreground">{meta}</div>
            </div>

            <div className="hidden md:block">
              {/* "Morning report ready" chip — the room-card indicator
                  (prd-night-run-surfaces Phase 3, item 2). Clicking anywhere
                  on the row opens the drawer auto-selected onto the
                  morning-report variant; the chip stays until every parked
                  question in the report is resolved. */}
              {morningReady && (
                <span className="mb-1 inline-flex w-fit items-center gap-1.5 rounded-full bg-[color-mix(in_oklab,var(--primary)_13%,transparent)] px-2.5 py-1 text-[10.5px] font-semibold text-primary">
                  <Sunrise className="size-3" />
                  Morning report
                </span>
              )}
              <span
                className={cn(
                  "inline-flex w-fit items-center gap-1.5 rounded-full px-2.5 py-1 text-[10.5px] font-semibold before:inline-block before:size-1.5 before:rounded-full before:bg-current before:content-['']",
                  PILL_CLASS[pill.tone],
                )}
              >
                {pill.label}
              </span>
            </div>

            <ChevronRight className="hidden size-4 text-muted-foreground/60 md:block" />
          </article>
        );
      })}
    </div>
  );
}
