import type { AdjudicationReportEntry } from "../../types";

export function AdjudicationRecords({
  records,
  idToTitle,
}: {
  records: AdjudicationReportEntry[];
  idToTitle: Record<string, string>;
}) {
  if (!records?.length) return null;

  return (
    <section className="mt-4 border-t border-border pt-3 text-xs">
      <h3 className="mb-2 font-semibold">Adjudications</h3>
      <ul className="divide-y divide-border">
        {records.map(({ execution_id, decider, decision, reviews, escalation }) => (
          <li key={execution_id} className="space-y-2 py-2 [overflow-wrap:anywhere]">
            <h4 className="font-medium">{idToTitle[execution_id] ?? execution_id}</h4>
            <p>Decider: {decider}</p>
            {decision && (
              <div className="space-y-1">
                <p className="font-medium">Options considered</p>
                <ol className="list-decimal space-y-1 pl-5">
                  {decision.options.map((option, i) => (
                    <li key={i}>
                      {option}
                      {decision.verdict.winner === i + 1 && <strong> (Decider’s choice)</strong>}
                      {decision.human_winner === i + 1 && <strong> (Human choice)</strong>}
                    </li>
                  ))}
                </ol>
                {decision.verdict.winner == null && <p>The decider selected no option.</p>}
                <p className="whitespace-pre-wrap">Rationale: {decision.verdict.rationale}</p>
              </div>
            )}
            {reviews.map(({ artifact_topic, verdict }, i) => (
              <div key={i} className="space-y-1">
                <p><span className="font-medium">Review of {artifact_topic}</span>: {verdict.outcome}</p>
                <p className="whitespace-pre-wrap">Rationale: {verdict.rationale}</p>
              </div>
            ))}
            {escalation ? (
              <div className="space-y-1 border-l-2 border-[var(--warning)] pl-2">
                <p className="font-medium">Escalation: {escalation.trigger.replaceAll("_", " ")}</p>
                <p className="whitespace-pre-wrap">{escalation.reason}</p>
                {escalation.resolution && <p className="whitespace-pre-wrap">Resolution: {escalation.resolution}</p>}
                {escalation.retry_required && !escalation.resolution && <p>Retry required.</p>}
              </div>
            ) : <p className="text-muted-foreground">No escalation recorded.</p>}
          </li>
        ))}
      </ul>
    </section>
  );
}
