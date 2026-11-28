//! Read-only projection of the decision and review records saved by the executor.
use super::RunPhase;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Decision {
    pub options: Vec<String>,
    pub verdict: DecisionVerdict,
    pub human_winner: Option<usize>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct DecisionVerdict {
    /// One-based index into the recorded options. An undecided verdict has no winner.
    pub winner: Option<usize>,
    pub rationale: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Review {
    pub artifact_topic: String,
    pub verdict: ReviewVerdict,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ReviewVerdict {
    pub outcome: String,
    pub rationale: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Escalation {
    pub trigger: String,
    pub reason: String,
    #[serde(default)]
    pub retry_required: bool,
    pub resolution: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct PhaseRecords {
    pub decision: Option<Decision>,
    #[serde(default)]
    pub reviews: Vec<Review>,
    pub escalation: Option<Escalation>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Adjudication {
    pub execution_id: String,
    pub decider: String,
    #[serde(flatten)]
    pub records: PhaseRecords,
}

#[derive(Deserialize)]
struct RecordedPlan {
    #[serde(default)]
    phases: Vec<PhaseRecords>,
}

/// Both projections read the same snapshot. Escalations belong to the phase:
/// a review gate can keep earlier reviews after a human resolves an escalation.
pub(super) fn from_snapshot(
    contents: &str,
    phases: &[RunPhase],
) -> Result<Vec<Adjudication>, serde_yaml::Error> {
    let records: RecordedPlan = serde_yaml::from_str(contents)?;
    Ok(phases
        .iter()
        .zip(records.phases)
        .filter(|(_, r)| r.decision.is_some() || !r.reviews.is_empty() || r.escalation.is_some())
        .map(|(phase, records)| Adjudication {
            execution_id: phase.execution_id.clone(),
            decider: phase
                .assigned_agent_name
                .as_ref()
                .or(phase.assigned_agent_id.as_ref())
                .or(phase.assigned_agent.as_ref())
                .cloned()
                .unwrap_or_else(|| "Default agent".into()),
            records,
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use crate::runplan::RunReport;
    use serde_json::json;

    #[test]
    fn adjudication_reads_run_scoped_decisions_reviews_and_escalations() {
        let root = std::env::temp_dir().join(format!("report-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&root).unwrap();
        let snapshot = r#"
id: test-report
project: /repo
created: 2026-08-01T00:00:00Z
phases:
  - execution_id: ordinary
    status: parked
    park_payload: "verify verdict: WARN"
  - execution_id: choose
    assigned_agent_id: deleted-id
    assigned_agent_name: Architect
    decision:
      options: ["branch-a (abc)", ".loopdeck/handoffs/design-b.md"]
      verdict: {winner: 1, rationale: "Lower cost", risks: [budget_breach]}
      human_winner: 2
    escalation:
      trigger: budget_breach
      reason: "Approval required"
      resolution: "Design B"
  - execution_id: gate
    assigned_agent: reviewer-id
    reviews:
      - artifact_topic: first
        verdict: {outcome: revise, rationale: "Missing estimate"}
      - artifact_topic: revised
        verdict: {outcome: approve, rationale: "Estimate supplied"}
  - execution_id: retry
    escalation: {trigger: ambiguous, reason: "No verdict", retry_required: true}
"#;
        crate::operational_store::save_document(&root, "run_plan", snapshot).unwrap();
        let report = RunReport::load(&root).unwrap().unwrap();
        let rows = serde_json::to_value(report.adjudications).unwrap();
        assert_eq!(rows.as_array().unwrap().len(), 3);
        assert_eq!(rows[0]["execution_id"], "choose");
        assert_eq!(rows[0]["decider"], "Architect");
        assert_eq!(
            rows[0]["decision"]["options"],
            json!(["branch-a (abc)", ".loopdeck/handoffs/design-b.md"])
        );
        assert_eq!(
            rows[0]["decision"]["verdict"],
            json!({"winner": 1, "rationale": "Lower cost"})
        );
        assert_eq!(rows[0]["decision"]["human_winner"], 2);
        assert_eq!(rows[0]["escalation"]["resolution"], "Design B");
        assert_eq!(rows[1]["decider"], "reviewer-id");
        assert_eq!(rows[1]["reviews"][0]["verdict"]["outcome"], "revise");
        assert_eq!(
            rows[1]["reviews"][1]["verdict"]["rationale"],
            "Estimate supplied"
        );
        assert_eq!(rows[2]["decider"], "Default agent");
        assert!(rows[2]["decision"].is_null());
        assert_eq!(
            rows[2]["escalation"],
            json!({"trigger":"ambiguous", "reason":"No verdict", "retry_required":true, "resolution":null})
        );

        let legacy = snapshot.split("  - execution_id: choose").next().unwrap();
        crate::operational_store::save_document(&root, "run_plan", legacy).unwrap();
        assert!(RunReport::load(&root)
            .unwrap()
            .unwrap()
            .adjudications
            .is_empty());
        std::fs::remove_dir(root).unwrap();
    }
}
