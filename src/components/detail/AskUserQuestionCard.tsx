import { useState } from "react";
import type { AskUserQuestionAnswers, AskUserQuestionSpec } from "../../types";

/**
 * Render an `AskUserQuestion` prompt as an interactive card.
 *
 * The agent has paused the turn to ask the user one or more questions; this
 * card collects the answers and calls `onSubmit` with a map keyed by question
 * text. While visible, it's the focal affordance — the composer is disabled.
 *
 * Selection model:
 * - Single-select (`multiSelect: false`): radio-style — clicking an option
 *   selects it and deselects the rest. Plus an "Other…" affordance with a
 *   text input, which clears any option selection.
 * - Multi-select (`multiSelect: true`): checkbox-style — options toggle
 *   independently. "Other…" appends free text alongside the labels.
 *
 * Submit is disabled until every question has at least one answer (a selected
 * label or non-empty "Other…" text). This mirrors the tool's contract that
 * every question must be answered.
 *
 * Extracted from `Chat.tsx` so it can be reused by the project detail view's
 * tab-agnostic "stuck prompt" callout (`ProjectDetail.tsx`) — same component,
 * same answer flow, mounted from two places.
 */
export function AskUserQuestionCard({
  questions,
  disabled,
  onSubmit,
  submitLabel,
}: {
  questions: AskUserQuestionSpec[];
  disabled?: boolean;
  onSubmit: (answers: AskUserQuestionAnswers) => void;
  /** Overrides the submit button's label. The night variant's parked card
   *  passes "Answer & requeue" — submitting there also requeues the phase. */
  submitLabel?: string;
}) {
  // Per-question selection state. For single-select we track a single label
  // string (or null); for multi-select a Set of labels. Plus an "Other…"
  // free-text value per question (its presence implies "Other…" was chosen).
  const [single, setSingle] = useState<Record<string, string | null>>({});
  const [multi, setMulti] = useState<Record<string, Set<string>>>({});
  const [otherText, setOtherText] = useState<Record<string, string>>({});
  const [otherActive, setOtherActive] = useState<Record<string, boolean>>({});

  /** Is this question answered? (a label selected, or non-empty Other text) */
  function isAnswered(q: AskUserQuestionSpec): boolean {
    if (otherActive[q.question] && otherText[q.question]?.trim()) return true;
    if (q.multiSelect) return (multi[q.question]?.size ?? 0) > 0;
    return single[q.question] != null;
  }

  /** Toggle a multi-select option on/off. */
  function toggleMulti(question: string, label: string) {
    setMulti((prev) => {
      const next = new Set(prev[question] ?? []);
      if (next.has(label)) next.delete(label);
      else next.add(label);
      return { ...prev, [question]: next };
    });
  }

  /** Build the answers map from current selection state. */
  function buildAnswers(): AskUserQuestionAnswers {
    const out: AskUserQuestionAnswers = {};
    for (const q of questions) {
      const labels = q.multiSelect
        ? Array.from(multi[q.question] ?? [])
        : single[q.question] != null
          ? [single[q.question] as string]
          : [];
      const ot = otherActive[q.question] ? otherText[q.question] : undefined;
      out[q.question] = { labels, otherText: ot };
    }
    return out;
  }

  const allAnswered = questions.every(isAnswered);

  return (
    <div className="my-2 rounded-lg border border-primary/30 bg-[color-mix(in_oklab,var(--primary)_5%,transparent)] p-3 space-y-4">
      <div className="flex items-center gap-2 text-xs font-medium text-primary">
        <span className="inline-block size-1.5 rounded-full bg-primary animate-pulse" />
        <span>The agent has a question for you</span>
      </div>

      {questions.map((q, qi) => {
        const answered = isAnswered(q);
        return (
          <div key={qi} className="space-y-2">
            <div className="flex items-center gap-2">
              <span className="text-[10px] font-semibold uppercase tracking-wide text-primary/80 bg-primary/10 rounded px-1.5 py-0.5">
                {q.header}
              </span>
            </div>
            <p className="text-sm text-foreground leading-relaxed">
              {q.question}
            </p>

            <div className="space-y-1.5">
              {q.options.map((opt, oi) => {
                const selected = q.multiSelect
                  ? (multi[q.question]?.has(opt.label) ?? false)
                  : single[q.question] === opt.label;
                return (
                  <button
                    key={oi}
                    type="button"
                    disabled={disabled}
                    onClick={() => {
                      if (q.multiSelect) {
                        toggleMulti(q.question, opt.label);
                      } else {
                        setSingle((p) => ({ ...p, [q.question]: opt.label }));
                        setOtherActive((p) => ({ ...p, [q.question]: false }));
                      }
                    }}
                    className={`w-full text-left rounded-md border px-3 py-2 transition disabled:opacity-50 disabled:cursor-not-allowed ${
                      selected
                        ? "border-primary bg-primary/10 text-foreground"
                        : "border-border bg-input hover:border-primary/40 text-foreground/90"
                    }`}
                  >
                    <div className="flex items-start gap-2">
                      <span className="text-primary text-xs mt-0.5 shrink-0">
                        {q.multiSelect
                          ? selected
                            ? "☑"
                            : "☐"
                          : selected
                            ? "●"
                            : "○"}
                      </span>
                      <div className="min-w-0">
                        <div className="text-sm font-medium">{opt.label}</div>
                        {opt.description && (
                          <div className="text-xs text-muted-foreground mt-0.5 leading-relaxed">
                            {opt.description}
                          </div>
                        )}
                      </div>
                    </div>
                  </button>
                );
              })}

              {/* "Other…" affordance: free-text override. For single-select it
                  replaces the option selection; for multi-select it appends. */}
              <div className="flex items-center gap-2">
                <button
                  type="button"
                  disabled={disabled}
                  onClick={() =>
                    setOtherActive((p) => ({
                      ...p,
                      [q.question]: !(p[q.question] ?? false),
                    }))
                  }
                  className={`text-xs px-2 py-1 rounded border transition disabled:opacity-50 disabled:cursor-not-allowed ${
                    otherActive[q.question]
                      ? "border-primary text-primary bg-primary/10"
                      : "border-border text-muted-foreground hover:text-foreground"
                  }`}
                >
                  {otherActive[q.question] ? "☑" : "☐"} Other…
                </button>
                {otherActive[q.question] && (
                  <input
                    type="text"
                    disabled={disabled}
                    value={otherText[q.question] ?? ""}
                    onChange={(e) =>
                      setOtherText((p) => ({
                        ...p,
                        [q.question]: e.target.value,
                      }))
                    }
                    // Single-select: typing Other clears the canned selection.
                    onFocus={() => {
                      if (!q.multiSelect) {
                        setSingle((p) => ({ ...p, [q.question]: null }));
                      }
                    }}
                    placeholder="Type your answer…"
                    className="flex-1 rounded-md border border-border bg-input px-2 py-1 text-sm text-foreground placeholder:text-muted-foreground focus:outline-none focus:ring-1 focus:ring-primary disabled:opacity-50"
                  />
                )}
              </div>
            </div>

            {!answered && (
              <p className="text-[10px] text-muted-foreground/60">
                Please select an option or type an answer.
              </p>
            )}
          </div>
        );
      })}

      <div className="flex justify-end pt-1">
        <button
          type="button"
          disabled={disabled || !allAnswered}
          onClick={() => onSubmit(buildAnswers())}
          className="inline-flex items-center gap-1.5 h-8 px-4 rounded-md bg-primary text-primary-foreground text-xs font-medium hover:opacity-90 transition disabled:opacity-50 disabled:cursor-not-allowed"
        >
          {submitLabel ?? `Submit answer${questions.length > 1 ? "s" : ""}`}
        </button>
      </div>
    </div>
  );
}
