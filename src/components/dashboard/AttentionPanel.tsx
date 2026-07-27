import { useMemo } from "react";
import { useNavigate } from "@tanstack/react-router";
import { ShieldAlert, MessageCircleQuestion, FileCheck2, Loader2 } from "lucide-react";
import { useAppStore } from "../../store/appStore";
import { usePendingInteractions } from "../../store/pendingInteractions";

interface AttentionItem {
  key: string;
  path: string;
  projectName: string;
  icon: React.ComponentType<{ className?: string }>;
  tone: "pink" | "cyan";
  title: string;
  meta: string;
  actionLabel: string;
  /** Solid pill for items that block the agent; text link for informational ones. */
  actionStyle: "solid" | "text";
  spin?: boolean;
}

/**
 * The full (uncapped) set of things needing attention, sourced from live
 * state: pending manual approvals / AskUserQuestion / plan-approval prompts
 * (which park an agent turn until the user answers), and projects currently
 * mid-turn. Shared by `AttentionPanel` (which caps the display to 3 cards)
 * and the Dashboard page heading (which reports the true count).
 */
export function useAttentionItems(): AttentionItem[] {
  const projects = useAppStore((s) => s.projects);
  const permissions = usePendingInteractions((s) => s.permissions);
  const questions = usePendingInteractions((s) => s.questions);
  const plans = usePendingInteractions((s) => s.plans);

  return useMemo<AttentionItem[]>(() => {
    const nameFor = (path: string) => projects.find((p) => p.path === path)?.name ?? path;
    const out: AttentionItem[] = [];

    for (const [path, p] of Object.entries(permissions)) {
      out.push({
        key: `perm-${path}`,
        path,
        projectName: nameFor(path),
        icon: ShieldAlert,
        tone: "pink",
        title: "Permission requested",
        meta: `wants to run ${p.toolName}`,
        actionLabel: "Review request",
        actionStyle: "solid",
      });
    }
    for (const [path, q] of Object.entries(questions)) {
      out.push({
        key: `q-${path}`,
        path,
        projectName: nameFor(path),
        icon: MessageCircleQuestion,
        tone: "pink",
        title: "Answer needed",
        meta: q.questions[0]?.header || "Agent is waiting on you",
        actionLabel: "Review request",
        actionStyle: "solid",
      });
    }
    for (const [path] of Object.entries(plans)) {
      out.push({
        key: `plan-${path}`,
        path,
        projectName: nameFor(path),
        icon: FileCheck2,
        tone: "pink",
        title: "Plan awaiting approval",
        meta: "Review the proposed plan",
        actionLabel: "Review request",
        actionStyle: "solid",
      });
    }
    for (const p of projects) {
      if (p.run_state !== "working") continue;
      out.push({
        key: `working-${p.path}`,
        path: p.path,
        projectName: p.name,
        icon: Loader2,
        tone: "cyan",
        title: "Agent is implementing",
        meta: "In progress",
        actionLabel: "View live run",
        actionStyle: "text",
        spin: true,
      });
    }

    return out;
  }, [permissions, questions, plans, projects]);
}

/**
 * "Needs attention" strip — up to 3 cards. Renders nothing when there's
 * genuinely nothing to flag — no empty-state filler card.
 */
export function AttentionPanel() {
  const navigate = useNavigate();
  const items = useAttentionItems().slice(0, 3);

  if (items.length === 0) return null;

  const openAgentTab = (path: string) => {
    useAppStore.getState().setSelectedProjectPath(path);
    useAppStore.getState().setDetailTab("agent");
    navigate({ to: "/project/$projectPath", params: { projectPath: encodeURIComponent(path) } });
  };

  return (
    <section aria-labelledby="attention-title" className="mb-8">
      <div
        id="attention-title"
        className="mb-2.5 text-[10px] font-semibold uppercase tracking-wider text-muted-foreground/70"
      >
        Needs attention
      </div>
      <div
        className="grid gap-3 rounded-2xl border p-4 shadow-[var(--shadow-md)]"
        style={{
          borderColor: "color-mix(in oklab, var(--primary) 22%, var(--border))",
          background:
            "linear-gradient(135deg, color-mix(in oklab, var(--primary) 10%, transparent), transparent 65%), var(--card)",
          gridTemplateColumns: `repeat(${items.length}, minmax(0, 1fr))`,
        }}
      >
        {items.map((item) => {
          const Icon = item.icon;
          return (
            <article
              key={item.key}
              className="grid grid-cols-[34px_minmax(0,1fr)] gap-3 rounded-[11px] border border-border bg-surface p-4"
            >
              <div
                className={
                  "grid size-[34px] place-items-center rounded-md " +
                  (item.tone === "pink"
                    ? "bg-[color-mix(in_oklab,var(--destructive)_14%,transparent)] text-destructive"
                    : "bg-[color-mix(in_oklab,var(--primary)_14%,transparent)] text-primary")
                }
              >
                <Icon className={item.spin ? "size-4 animate-spin" : "size-4"} />
              </div>
              <div className="min-w-0">
                <div className="text-sm font-semibold leading-tight">{item.title}</div>
                <div className="mt-1 truncate text-xs text-muted-foreground">
                  {item.projectName} · {item.meta}
                </div>
                {item.actionStyle === "solid" ? (
                  <button
                    type="button"
                    onClick={() => openAgentTab(item.path)}
                    className="mt-3 rounded-md bg-primary px-2.5 py-1.5 text-[10.5px] font-semibold text-primary-foreground transition-opacity hover:opacity-90"
                  >
                    {item.actionLabel}
                  </button>
                ) : (
                  <button
                    type="button"
                    onClick={() => openAgentTab(item.path)}
                    className="mt-3 text-xs font-medium text-foreground hover:underline"
                  >
                    {item.actionLabel} →
                  </button>
                )}
              </div>
            </article>
          );
        })}
      </div>
    </section>
  );
}
