//! Versioned provider-neutral run-event contract.
//!
//! Adapters will convert Claude/Codex wire events into this envelope before
//! persistence. The current `ClaudeEvent` channel remains unchanged until the
//! provider-normalization phase.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

pub const RUN_EVENT_VERSION: u16 = 1;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Provider {
    Claude,
    Codex,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
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
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RawProviderRef {
    pub provider_event_id: Option<String>,
    pub payload_sha256: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ContentKind {
    Text,
    Thinking,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DecisionKind {
    Approval,
    Question,
    PlanApproval,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DecisionState {
    Pending,
    Allowed,
    Denied,
    Answered,
    Expired,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TerminalOutcome {
    Completed,
    Error,
    Interrupted,
    Cancelled,
    Killed,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
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
        summary: Option<String>,
    },
    Usage {
        input_tokens: u64,
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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RunEvent {
    pub version: u16,
    pub run_id: String,
    /// Reserved for the durable event store; adapters leave this `None` until
    /// the store assigns a monotonic per-run sequence.
    pub sequence: Option<u64>,
    pub event_id: String,
    pub timestamp: DateTime<Utc>,
    pub provider: Provider,
    pub payload: RunEventPayload,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub raw_ref: Option<RawProviderRef>,
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
