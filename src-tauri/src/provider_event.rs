//! Versioned provider-neutral run-event contract.
//!
//! Adapters convert Claude/Codex wire events into this envelope at their
//! boundary. During the normalization phase the legacy `ClaudeEvent` channel
//! keeps its exact payloads and every normalized event is dual-emitted on a
//! separate `RunEvent` stream, so the two surfaces can be compared before the
//! replay migration retires the legacy channel.
//!
//! The TypeScript bindings under `src/types/generated/` are generated from
//! these types with `ts-rs`; CI regenerates them and fails on drift.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

pub const RUN_EVENT_VERSION: u16 = 1;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, TS)]
#[serde(rename_all = "lowercase")]
#[ts(export, export_to = "../../src/types/generated/")]
pub enum Provider {
    Claude,
    Codex,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export, export_to = "../../src/types/generated/")]
pub enum Capability {
    Text,
    Thinking,
    ToolActivity,
    Questions,
    Approvals,
    PlanApproval,
    Usage,
    Interrupt,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, TS)]
#[ts(export, export_to = "../../src/types/generated/")]
pub struct CapabilitySet {
    pub provider: Provider,
    pub supported: Vec<Capability>,
}

impl CapabilitySet {
    pub fn for_provider(provider: Provider) -> Self {
        let supported = match provider {
            Provider::Claude => vec![
                Capability::Text,
                Capability::Thinking,
                Capability::ToolActivity,
                Capability::Questions,
                Capability::Approvals,
                Capability::PlanApproval,
                Capability::Usage,
                Capability::Interrupt,
            ],
            Provider::Codex => vec![
                Capability::Text,
                Capability::Thinking,
                Capability::ToolActivity,
                Capability::Questions,
                Capability::Approvals,
                Capability::Usage,
                Capability::Interrupt,
            ],
        };
        Self {
            provider,
            supported,
        }
    }
}

/// A diagnostic pointer, never a product identity and never the raw provider
/// payload. Raw payloads remain in provider-scoped diagnostics under the
/// existing redaction policy.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, TS)]
#[ts(export, export_to = "../../src/types/generated/")]
pub struct RawProviderRef {
    pub provider_event_id: Option<String>,
    pub payload_sha256: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export, export_to = "../../src/types/generated/")]
pub enum ContentKind {
    Text,
    Thinking,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export, export_to = "../../src/types/generated/")]
pub enum DecisionKind {
    Approval,
    Question,
    PlanApproval,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export, export_to = "../../src/types/generated/")]
pub enum DecisionState {
    Pending,
    Allowed,
    Denied,
    Answered,
    Expired,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export, export_to = "../../src/types/generated/")]
pub enum TerminalOutcome {
    Completed,
    Error,
    Interrupted,
    Cancelled,
    Killed,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, TS)]
#[serde(tag = "type", rename_all = "snake_case")]
#[ts(export, export_to = "../../src/types/generated/")]
pub enum RunEventPayload {
    Started {
        session_id: Option<String>,
        capabilities: CapabilitySet,
    },
    Content {
        kind: ContentKind,
        text: String,
    },
    Tool {
        name: String,
        input_redacted: String,
    },
    Decision {
        decision_id: String,
        kind: DecisionKind,
        state: DecisionState,
        /// Short redacted narration (tool/question/plan). Automatic policy
        /// approval is encoded here with the stable `auto-allow:` prefix.
        summary: Option<String>,
        /// Kind-specific redacted render detail: question specs, plan markdown,
        /// or the approval tool input as JSON. Present so a consumer never has
        /// to re-read the provider payload to render a decision card; it is
        /// redacted and truncated, never a raw provider payload.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        detail_redacted: Option<String>,
    },
    Usage {
        // Token counts stay JS `number` (well under 2^53) so the frontend
        // contract matches the existing UI shape; `bigint` would force a
        // conversion at every render site for no benefit.
        #[ts(type = "number")]
        input_tokens: u64,
        #[ts(type = "number")]
        output_tokens: u64,
        total_cost_usd: Option<f64>,
    },
    Retrying {
        attempt: u32,
        max_attempts: u32,
        error_redacted: String,
    },
    Terminal {
        outcome: TerminalOutcome,
        message_redacted: Option<String>,
    },
    Unsupported {
        capability: String,
        detail_redacted: Option<String>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, TS)]
#[ts(export, export_to = "../../src/types/generated/")]
pub struct RunEvent {
    pub version: u16,
    pub run_id: String,
    /// Reserved for the durable event store; adapters leave this `None` until
    /// the store assigns a monotonic per-run sequence.
    #[ts(type = "number")]
    pub sequence: Option<u64>,
    pub event_id: String,
    pub timestamp: DateTime<Utc>,
    pub provider: Provider,
    pub payload: RunEventPayload,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub raw_ref: Option<RawProviderRef>,
}

/// Transport envelope for the app-wide additive stream.
///
/// `event.run_id` identifies the LoopDeck run; `project_path` routes the
/// stream to the frontend's per-project stores, and `execution_id` names the
/// queued phase batch that produced it.
#[derive(Debug, Clone, Serialize, TS)]
#[ts(export, export_to = "../../src/types/generated/")]
pub struct RunEventEnvelope {
    pub project_path: String,
    pub execution_id: String,
    pub event: RunEvent,
}

impl RunEvent {
    pub fn new(
        run_id: impl Into<String>,
        event_id: impl Into<String>,
        timestamp: DateTime<Utc>,
        provider: Provider,
        payload: RunEventPayload,
    ) -> Self {
        Self {
            version: RUN_EVENT_VERSION,
            run_id: run_id.into(),
            sequence: None,
            event_id: event_id.into(),
            timestamp,
            provider,
            payload,
            raw_ref: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn envelope_round_trips_with_sequence_placeholder_and_raw_reference() {
        let mut event = RunEvent::new(
            "run-1",
            "event-1",
            DateTime::UNIX_EPOCH,
            Provider::Claude,
            RunEventPayload::Terminal {
                outcome: TerminalOutcome::Completed,
                message_redacted: Some("done".into()),
            },
        );
        event.raw_ref = Some(RawProviderRef {
            provider_event_id: Some("provider-42".into()),
            payload_sha256: Some("abc123".into()),
        });
        let json = serde_json::to_string(&event).unwrap();
        let decoded: RunEvent = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded, event);
        assert_eq!(decoded.version, RUN_EVENT_VERSION);
        assert_eq!(decoded.sequence, None);
    }

    #[test]
    fn capability_differences_are_explicit() {
        let claude = CapabilitySet::for_provider(Provider::Claude);
        let codex = CapabilitySet::for_provider(Provider::Codex);
        assert!(claude.supported.contains(&Capability::PlanApproval));
        assert!(!codex.supported.contains(&Capability::PlanApproval));
    }
}
