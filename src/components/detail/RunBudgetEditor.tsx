import { useState } from "react";
import { toast } from "sonner";
import type { RunBudgets, RunPlan } from "../../types";
import { updateRunBudgets } from "../../lib/tauri";

const fields = [
  ["per_phase_token_cap", "Tokens/phase"],
  ["per_phase_wall_clock_secs", "Seconds/phase"],
  ["total_run_wall_clock_secs", "Seconds/run"],
] as const;

export function RunBudgetEditor({ projectPath, budgets, onSaved }: {
  projectPath: string;
  budgets: RunBudgets;
  onSaved: (plan: RunPlan) => void;
}) {
  const [editing, setEditing] = useState(false);
  const [saving, setSaving] = useState(false);
  const [values, setValues] = useState<RunBudgets>(budgets);
  const save = async () => {
    if (Object.values(values).some((value) => value != null && (!Number.isSafeInteger(value) || value <= 0))) {
      toast.error("Budget values must be positive whole numbers");
      return;
    }
    setSaving(true);
    try {
      onSaved(await updateRunBudgets(projectPath, values));
      setEditing(false);
      toast.success("Budgets saved", { description: "Select Continue unfinished to resume the run." });
    } catch (error) {
      toast.error("Failed to update budgets", { description: (error as Error).message ?? String(error) });
    } finally {
      setSaving(false);
    }
  };
  return (
    <div className="mb-3 text-xs text-muted-foreground">
      {editing ? (
        <form onSubmit={(event) => { event.preventDefault(); void save(); }} className="flex flex-wrap items-end gap-2">
          {fields.map(([key, label]) => (
            <label key={key} className="flex flex-col gap-1">
              {label}
              <input type="number" min="1" step="1" placeholder="Default" disabled={saving}
                value={values[key] ?? ""}
                onChange={(event) => setValues((previous) => ({ ...previous, [key]: event.target.value === "" ? null : Number(event.target.value) }))}
                className="h-7 w-32 rounded border border-border bg-background px-2 text-foreground" />
            </label>
          ))}
          <button type="submit" disabled={saving} className="rounded border border-border px-2 py-1 hover:bg-accent disabled:opacity-50">{saving ? "Saving…" : "Save budgets"}</button>
          <button type="button" disabled={saving} onClick={() => setEditing(false)} className="px-2 py-1">Cancel</button>
          <p className="basis-full">Leave blank to use defaults: 500,000 tokens/phase, 5,400 seconds/phase, 28,800 seconds/run. Save before continuing unfinished phases.</p>
        </form>
      ) : (
        <div className="flex flex-wrap items-center gap-2">
          <span>Budgets: {budgets.per_phase_token_cap?.toLocaleString() ?? "500,000"} tokens/phase · {budgets.per_phase_wall_clock_secs ?? 5400}s/phase · {budgets.total_run_wall_clock_secs ?? 28800}s/run</span>
          <button onClick={() => { setValues(budgets); setEditing(true); }} className="rounded border border-border px-2 py-1 hover:bg-accent">Edit budgets</button>
        </div>
      )}
    </div>
  );
}
