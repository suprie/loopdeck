import { createFileRoute } from "@tanstack/react-router";
import { RotateCw, Check } from "lucide-react";

import { PageHeader } from "@/components/app-shell";
import { cn } from "@/lib/utils";

export const Route = createFileRoute("/loops")({
  component: LoopsPage,
});

const loops = [
  {
    project: "loopdeck",
    title: "Refactor chat UI",
    steps: [
      { label: "Extract BlockList component", done: true },
      { label: "Move approval card to primitive", done: true },
      { label: "Wire streaming cursor to store", done: false },
      { label: "Remove legacy tool-call sidebar", done: false },
    ],
  },
  {
    project: "harbor",
    title: "Investigate rounding bug",
    steps: [
      { label: "Reproduce with dispatcher fixture", done: true },
      { label: "Isolate to ledger normalizer", done: false },
      { label: "Patch + regression test", done: false },
    ],
  },
];

function LoopsPage() {
  return (
    <>
      <PageHeader title="Loops" subtitle="Active work threads with structured steps" />
      <div className="mx-auto w-full max-w-3xl space-y-4 px-8 py-8">
        {loops.map((l, i) => (
          <div
            key={i}
            className="rounded-xl border border-border bg-card p-5 shadow-[var(--shadow-sm)]"
          >
            <div className="flex items-center justify-between">
              <div className="flex items-center gap-2">
                <RotateCw className="size-3.5 text-primary" />
                <span className="text-[10px] font-semibold uppercase tracking-wider text-muted-foreground">
                  {l.project}
                </span>
              </div>
              <span className="text-[11px] text-muted-foreground">
                {l.steps.filter((s) => s.done).length}/{l.steps.length} done
              </span>
            </div>
            <h3 className="mt-2 text-sm font-semibold tracking-tight">{l.title}</h3>
            <ol className="mt-3 space-y-1.5">
              {l.steps.map((s, j) => (
                <li key={j} className="flex items-center gap-2 text-xs">
                  <span
                    className={cn(
                      "flex size-4 items-center justify-center rounded border",
                      s.done
                        ? "border-success bg-success/10 text-success"
                        : "border-border bg-background",
                    )}
                  >
                    {s.done && <Check className="size-2.5" />}
                  </span>
                  <span
                    className={cn(
                      s.done ? "text-muted-foreground line-through" : "text-foreground",
                    )}
                  >
                    {s.label}
                  </span>
                </li>
              ))}
            </ol>
          </div>
        ))}
      </div>
    </>
  );
}
