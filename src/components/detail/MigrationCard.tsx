import { useState } from "react";
import {
  ArrowRightLeft,
  AlertTriangle,
  ChevronDown,
   Loader2,
} from "lucide-react";
import { toast } from "sonner";
import type { MigrationPreview, AppError } from "../../types";
import * as api from "../../lib/tauri";

interface MigrationCardProps {
  projectPath: string;
  preview: MigrationPreview;
  /** Called after a successful migration so the panel can reload into structured mode. */
  onMigrated: () => void;
}

/**
 * Phase 4 migration surface for a project still on legacy `.loopdeck/loops.md`.
 *
 * Shows the exact-match preview: which records map to PRD loop IDs, and every
 * unmatched/ambiguous record (the reconciliation panel the PRD calls for) plus
 * any unconverted Next Steps. Records are matched by exact title only — never
 * guessed — so unmatched ones are listed for the human to see, not silently
 * dropped. The "Migrate" action is gated by an inline confirmation that spells
 * out the rename (`loops.md` → `loops.legacy.md`, original preserved) before any
 * write. After confirmation it calls `applyMigration` and reloads.
 */
export function MigrationCard({
  projectPath,
  preview,
  onMigrated,
}: MigrationCardProps) {
  const [expanded, setExpanded] = useState(false);
  const [confirming, setConfirming] = useState(false);
  const [migrating, setMigrating] = useState(false);

  const unmatchedCount = preview.unmatched.length;
  const unconvertedCount = preview.unconverted_next_steps.length;

  async function runMigration() {
    setMigrating(true);
    try {
      await api.applyMigration(projectPath);
      toast.success("Migration complete", {
        description:
          "execution.yaml written; original saved as loops.legacy.md",
      });
      onMigrated();
    } catch (err) {
      const appErr = err as AppError;
      toast.error("Migration failed", { description: appErr.message ?? String(err) });
    } finally {
      setMigrating(false);
      setConfirming(false);
    }
  }

  return (
    <div className="rounded-lg border border-[color-mix(in_oklab,var(--primary)_35%,transparent)] bg-[color-mix(in_oklab,var(--primary)_5%,transparent)] p-4 mb-6 max-w-2xl">
      <div className="flex items-start gap-2.5">
        <ArrowRightLeft size={16} className="text-[var(--primary)] mt-0.5 shrink-0" />
        <div className="flex-1 min-w-0">
          <h3 className="text-sm font-semibold text-foreground">
            Migrate to structured execution state
          </h3>
          <p className="text-xs text-muted-foreground mt-1 leading-relaxed">
            Convert this project's{" "}
            <code className="font-mono text-[11px] bg-muted px-1 py-0.5 rounded">
              .loopdeck/loops.md
            </code>{" "}
            to the structured{" "}
            <code className="font-mono text-[11px] bg-muted px-1 py-0.5 rounded">
              execution.yaml
            </code>{" "}
            store. Records match PRD loops by exact title — never guessed.
          </p>

          <div className="flex flex-wrap gap-x-4 gap-y-1 mt-2 text-[11px] text-muted-foreground">
            <span>
              Current:{" "}
              <strong className="text-foreground">
                {preview.current_matched ? "matched" : "unmatched or none"}
              </strong>
            </span>
            <span>
              History:{" "}
              <strong className="text-foreground">
                {preview.matched_history} matched
              </strong>
            </span>
            {unmatchedCount > 0 && (
              <span style={{ color: "var(--warning)" }}>
                {unmatchedCount} unmatched
              </span>
            )}
            {unconvertedCount > 0 && (
              <span>{unconvertedCount} next-step(s) unconverted</span>
            )}
          </div>

          {(unmatchedCount > 0 || unconvertedCount > 0) && (
            <>
              <button
                onClick={() => setExpanded((v) => !v)}
                className="flex items-center gap-1 mt-2 text-[11px] text-muted-foreground hover:text-foreground transition-colors"
              >
                <ChevronDown
                  size={12}
                  className={`transition-transform ${expanded ? "rotate-180" : ""}`}
                />
                {expanded ? "Hide" : "Show"} reconciliation details
              </button>
              {expanded && (
                <ul className="mt-2 space-y-1 text-[11px] text-muted-foreground border-l-2 border-border pl-3">
                  {preview.unmatched.map((u) => (
                    <li key={u.label} className="flex items-start gap-2">
                      <AlertTriangle
                        size={11}
                        className="mt-0.5 shrink-0"
                        style={{ color: "var(--warning)" }}
                      />
                      <span>
                        <code className="font-mono text-[10px]">{u.label}</code>{" "}
                        · [{u.section}] {u.title}
                        <span className="text-muted-foreground/70">
                          {" "}
                          — {u.reason}
                        </span>
                      </span>
                    </li>
                  ))}
                  {preview.unconverted_next_steps.map((step, index) => (
                    <li
                      key={`next-step-${index}`}
                      className="flex items-start gap-2"
                    >
                      <AlertTriangle
                        size={11}
                        className="mt-0.5 shrink-0"
                        style={{ color: "var(--warning)" }}
                      />
                      <span>
                        <code className="font-mono text-[10px]">
                          next-step/{index + 1}
                        </code>{" "}
                        · [{step.checked ? "x" : " "}] {step.text}
                        <span className="text-muted-foreground/70">
                          {" "}
                          — no stable loop ID
                        </span>
                      </span>
                    </li>
                  ))}
                  <li className="text-[10px] text-muted-foreground/70 pt-1">
                    Unmatched records and next steps are preserved in{" "}
                    <code className="font-mono">loops.legacy.md</code> and are not
                    written to execution.yaml.
                  </li>
                </ul>
              )}
            </>
          )}

          {!confirming ? (
            <button
              onClick={() => setConfirming(true)}
              className="mt-3 text-xs font-medium px-3 py-1.5 rounded-md text-[var(--primary-foreground)] hover:opacity-90 transition-opacity"
              style={{ background: "var(--primary)" }}
            >
              Migrate
            </button>
          ) : (
            <div className="mt-3 p-3 rounded-md bg-card border border-border">
              <p className="text-xs text-foreground mb-2 leading-relaxed">
                This writes{" "}
                <code className="font-mono text-[11px]">execution.yaml</code> and
                renames{" "}
                <code className="font-mono text-[11px]">loops.md</code> →{" "}
                <code className="font-mono text-[11px]">loops.legacy.md</code>{" "}
                (the original is preserved verbatim). Continue?
              </p>
              <div className="flex gap-2">
                <button
                  onClick={runMigration}
                  disabled={migrating}
                  className="text-xs font-medium px-3 py-1.5 rounded-md text-white hover:opacity-90 disabled:opacity-50 flex items-center gap-1.5 transition-opacity"
                  style={{ background: "var(--success)" }}
                >
                  {migrating && <Loader2 size={12} className="animate-spin" />}
                  Confirm migration
                </button>
                <button
                  onClick={() => setConfirming(false)}
                  disabled={migrating}
                  className="text-xs font-medium px-3 py-1.5 rounded-md border border-border text-foreground hover:bg-muted disabled:opacity-50 transition-colors"
                >
                  Cancel
                </button>
              </div>
            </div>
          )}
        </div>
      </div>
    </div>
  );
}
