import { createFileRoute } from "@tanstack/react-router";
import { Activity, GitCommitHorizontal, RotateCw, Bot } from "lucide-react";

import { PageHeader } from "@/components/app-shell";

export const Route = createFileRoute("/activity")({
  component: ActivityPage,
});

const events = [
  { icon: GitCommitHorizontal, label: "loopdeck", detail: "feat: add streaming agent chat", ago: "3h ago" },
  { icon: Bot, label: "agentui", detail: "Agent run completed · 4.2s", ago: "5h ago" },
  { icon: RotateCw, label: "harbor", detail: "Loop started · Investigate rounding bug", ago: "6h ago" },
  { icon: GitCommitHorizontal, label: "agentui", detail: "chore: bump react to 19.1", ago: "6h ago" },
  { icon: Activity, label: "notebook", detail: "Folder modified", ago: "8h ago" },
];

function ActivityPage() {
  return (
    <>
      <PageHeader title="Activity" subtitle="Recent events across all projects" />
      <div className="mx-auto w-full max-w-3xl px-8 py-8">
        <ol className="relative space-y-3 border-l border-border pl-6">
          {events.map((e, i) => (
            <li key={i} className="relative">
              <span className="absolute -left-[27px] top-2 flex size-4 items-center justify-center rounded-full border border-border bg-background">
                <e.icon className="size-2.5 text-muted-foreground" />
              </span>
              <div className="rounded-lg border border-border bg-card p-3 shadow-[var(--shadow-sm)]">
                <div className="flex items-center justify-between text-xs">
                  <span className="font-semibold">{e.label}</span>
                  <span className="text-muted-foreground">{e.ago}</span>
                </div>
                <div className="mt-1 text-xs text-muted-foreground">{e.detail}</div>
              </div>
            </li>
          ))}
        </ol>
      </div>
    </>
  );
}
