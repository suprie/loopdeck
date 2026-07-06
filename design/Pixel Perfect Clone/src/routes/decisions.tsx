import { createFileRoute } from "@tanstack/react-router";
import { GitBranch } from "lucide-react";

import { PageHeader } from "@/components/app-shell";

export const Route = createFileRoute("/decisions")({
  component: DecisionsPage,
});

const decisions = [
  {
    project: "loopdeck",
    title: "Adopt TanStack Start for the web mockup",
    rationale: "Type-safe file routing and shared query client fit the local-first read model.",
    date: "Jul 1",
  },
  {
    project: "agentui",
    title: "Render tool calls inline, not in a sidebar",
    rationale: "Sidebar hides context; inline keeps the causal chain visible.",
    date: "Jun 24",
  },
  {
    project: "harbor",
    title: "Freeze dispatcher rounding at 2dp until reconciliation",
    rationale: "Downstream ledger cannot tolerate drift; 2dp matches invoice format.",
    date: "Jun 18",
  },
];

function DecisionsPage() {
  return (
    <>
      <PageHeader title="Decisions" subtitle="Architectural notes captured across projects" />
      <div className="mx-auto w-full max-w-3xl space-y-3 px-8 py-8">
        {decisions.map((d, i) => (
          <div
            key={i}
            className="rounded-xl border border-border bg-card p-5 shadow-[var(--shadow-sm)]"
          >
            <div className="flex items-center justify-between">
              <div className="flex items-center gap-2 text-[10px] font-semibold uppercase tracking-wider text-muted-foreground">
                <GitBranch className="size-3" />
                {d.project}
              </div>
              <span className="text-[11px] text-muted-foreground">{d.date}</span>
            </div>
            <h3 className="mt-2 text-sm font-semibold tracking-tight">{d.title}</h3>
            <p className="mt-1.5 text-xs leading-relaxed text-muted-foreground">{d.rationale}</p>
          </div>
        ))}
      </div>
    </>
  );
}
