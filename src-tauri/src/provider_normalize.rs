//! Provider-boundary normalization: Claude/Codex wire events → `RunEvent`.
//!
//! The normalization phase is deliberately additive. Every adapter still
//! emits its existing `ClaudeEvent` channel payload unchanged; alongside it
//! the same code path feeds a [`RunEventNormalizer`], whose output is sent on
//! a separate `Channel<RunEvent>`. That dual-emit makes the two surfaces
//! comparable without changing any legacy consumer, and gives the fixture
//! contract tests a provider-independent input shape.
//!
//! Rules enforced here (see `docs/epics/execution-foundations/provider-event-contract.md`):
//! - exactly one durable `terminal` event per run segment (`terminal` is gated
//!   on [`RunEventNormalizer::terminal`], so a duplicated provider terminal or
//!   a synthesized interrupt terminal cannot create a second one);
//! - tool inputs, decision summaries/details, and terminal messages pass
//!   through the same secret patterns as the pre-push scan;
//! - automatic policy approval is `DecisionState::Allowed` with the stable
//!   `auto-allow:` summary prefix — no enum change, and the morning report can
//!   still distinguish it from a human allow;
//! - a capability the provider does not expose becomes `unsupported`, never a
//!   silently different payload (notably Codex plan approval).
//!
//! Event IDs are `"<run_id>:<segment>:<n>"`. A retry attempt is a new segment
//! (the command layer creates a fresh normalizer per `send_message_streaming`
//! call), which is why each attempt can carry its own terminal without
//! violating the one-terminal-per-segment rule.

use crate::agents::{
    parse_ask_user_questions, parse_exit_plan, parse_stream_line, AskUserQuestionSpec,
    ContentBlock, StreamEvent,
};
use crate::provider_event::{
    CapabilitySet, ContentKind, DecisionKind, DecisionState, Provider, RawProviderRef, RunEvent,
    RunEventPayload, TerminalOutcome,
};
use crate::secret_scan::redact_secrets;
use chrono::Utc;
use serde_json::Value;
use std::collections::HashSet;
use tauri::ipc::Channel;
use uuid::Uuid;

/// Stable summary prefix for a policy-approved action that skipped the human
/// review gate (autonomous mode or an explicit role rule). Consumers — the
/// morning report included — match this prefix instead of an enum variant.
pub const AUTO_ALLOW_PREFIX: &str = "auto-allow:";

/// Durable detail strings are redacted and truncated before they become
/// product state. Long enough for a plan or question set, short enough that a
/// tool input cannot smuggle an unbounded provider payload into the store.
const MAX_DETAIL_CHARS: usize = 4_000;

/// A `Channel<RunEvent>` plus the run identity it belongs to.
///
/// Passed from the command layer into a session with
/// [`crate::harness::HarnessSession::attach_run_events`]. `Channel` is `Clone`,
/// so a cached session can keep it for the lifetime of a run.
#[derive(Clone)]
pub struct RunEventChannel {
    pub run_id: String,
    pub channel: Channel<RunEvent>,
}

impl RunEventChannel {
    pub fn new(run_id: impl Into<String>, channel: Channel<RunEvent>) -> Self {
        Self {
            run_id: run_id.into(),
            channel,
        }
    }
}

fn truncate_chars(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        return text.to_owned();
    }
    let mut out: String = text.chars().take(max).collect();
    out.push_str("…[truncated]");
    out
}

/// Redact + truncate free text before it enters a durable event.
pub fn redact_text(text: &str) -> String {
    truncate_chars(&redact_secrets(text), MAX_DETAIL_CHARS)
}

/// Redact + truncate a JSON value serialized as compact text.
pub fn redact_value(value: &Value) -> String {
    redact_text(&value.to_string())
}

/// Renderable approval detail. The pending card needs the tool name and the
/// redacted input; keeping both in one JSON object means a consumer never has
/// to parse narration text to reconstruct the payload.
fn approval_detail(tool_name: &str, input: &Value) -> Value {
    serde_json::json!({ "tool_name": tool_name, "input": input })
}

fn first_line(text: &str) -> String {
    text.lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .unwrap_or_default()
        .to_owned()
}

/// Human-readable narration for an approval payload. Description wins over
/// reason over command, so the summary explains *why* the agent needs the
/// tool when the provider supplied that context.
fn input_narration(input: &Value) -> Option<String> {
    for key in ["description", "reason", "command"] {
        if let Some(text) = input.get(key).and_then(Value::as_str) {
            if !text.trim().is_empty() {
                return Some(truncate_chars(text.trim(), 200));
            }
        }
    }
    None
}

/// One run segment's normalizer. Stateful only for id generation, duplicate
/// start suppression, Codex delta/tool dedupe, and the one-terminal gate.
pub struct RunEventNormalizer {
    run_id: String,
    provider: Provider,
    segment: String,
    counter: u64,
    started_emitted: bool,
    terminal_emitted: bool,
    /// Codex items whose text arrived as deltas (the completed item would be
    /// a duplicate) and tools already surfaced on `item/started`.
    codex_delta_items: HashSet<String>,
    codex_tool_items: HashSet<String>,
}

impl RunEventNormalizer {
    pub fn new(run_id: impl Into<String>, provider: Provider) -> Self {
        Self {
            run_id: run_id.into(),
            provider,
            segment: Uuid::new_v4().to_string(),
            counter: 0,
            started_emitted: false,
            terminal_emitted: false,
            codex_delta_items: HashSet::new(),
            codex_tool_items: HashSet::new(),
        }
    }

    #[cfg(test)]
    pub fn terminal_emitted(&self) -> bool {
        self.terminal_emitted
    }

    fn next(&mut self, payload: RunEventPayload) -> RunEvent {
        self.next_with_ref(payload, None)
    }

    fn next_with_ref(
        &mut self,
        payload: RunEventPayload,
        raw_ref: Option<RawProviderRef>,
    ) -> RunEvent {
        self.counter = self.counter.saturating_add(1);
        let mut event = RunEvent::new(
            self.run_id.clone(),
            format!("{}:{}:{}", self.run_id, self.segment, self.counter),
            Utc::now(),
            self.provider,
            payload,
        );
        event.raw_ref = raw_ref;
        event
    }

    /// Run segment start. Only the first start in a segment is durable; a
    /// provider that repeats its init line cannot create a second `started`.
    pub fn started(&mut self, session_id: Option<String>) -> Option<RunEvent> {
        if self.started_emitted {
            return None;
        }
        self.started_emitted = true;
        Some(self.next(RunEventPayload::Started {
            session_id,
            capabilities: CapabilitySet::for_provider(self.provider),
        }))
    }

    pub fn content(&mut self, kind: ContentKind, text: impl Into<String>) -> RunEvent {
        self.next(RunEventPayload::Content {
            kind,
            text: text.into(),
        })
    }

    pub fn tool(&mut self, name: impl Into<String>, input: &Value) -> RunEvent {
        self.next(RunEventPayload::Tool {
            name: name.into(),
            input_redacted: redact_value(input),
        })
    }

    pub fn usage(
        &mut self,
        input_tokens: u64,
        output_tokens: u64,
        total_cost_usd: Option<f64>,
    ) -> RunEvent {
        self.next(RunEventPayload::Usage {
            input_tokens,
            output_tokens,
            total_cost_usd,
        })
    }

    pub fn retrying(
        &mut self,
        attempt: u32,
        max_attempts: u32,
        error_redacted: impl Into<String>,
    ) -> RunEvent {
        self.next(RunEventPayload::Retrying {
            attempt,
            max_attempts,
            error_redacted: error_redacted.into(),
        })
    }

    pub fn unsupported(
        &mut self,
        capability: impl Into<String>,
        detail_redacted: Option<String>,
    ) -> RunEvent {
        self.next(RunEventPayload::Unsupported {
            capability: capability.into(),
            detail_redacted,
        })
    }

    pub fn decision(
        &mut self,
        decision_id: impl Into<String>,
        kind: DecisionKind,
        state: DecisionState,
        summary: Option<String>,
        detail_redacted: Option<String>,
    ) -> RunEvent {
        self.next(RunEventPayload::Decision {
            decision_id: decision_id.into(),
            kind,
            state,
            summary: summary.map(|s| redact_text(&s)),
            detail_redacted,
        })
    }

    /// The one durable terminal for this segment. Every later terminal (a
    /// retried provider terminal, a synthesized interrupt, an error-path
    /// terminal after a real result) is dropped here.
    pub fn terminal(
        &mut self,
        outcome: TerminalOutcome,
        message_redacted: Option<String>,
    ) -> Option<RunEvent> {
        if self.terminal_emitted {
            return None;
        }
        self.terminal_emitted = true;
        Some(self.next(RunEventPayload::Terminal {
            outcome,
            message_redacted: message_redacted.map(|m| redact_text(&m)),
        }))
    }

    // ── Claude ─────────────────────────────────────────────────────────────

    /// Convert one Claude `stream-json` line. Unknown/unparseable lines yield
    /// no events (the adapter's own line parser skips them too).
    pub fn convert_claude_line(&mut self, line: &str) -> Vec<RunEvent> {
        let Ok(value) = serde_json::from_str::<Value>(line.trim()) else {
            return Vec::new();
        };
        let Some(event) = parse_stream_line(line) else {
            return Vec::new();
        };
        match event {
            StreamEvent::System => self
                .started(
                    value
                        .get("session_id")
                        .and_then(Value::as_str)
                        .map(ToOwned::to_owned),
                )
                .into_iter()
                .collect(),
            StreamEvent::Assistant { message, .. } => {
                let mut events = Vec::new();
                for block in &message.content {
                    match block {
                        ContentBlock::Text { text } => {
                            if !text.is_empty() {
                                events.push(self.content(ContentKind::Text, text.clone()));
                            }
                        }
                        ContentBlock::Thinking { thinking } => {
                            if !thinking.is_empty() {
                                events.push(self.content(ContentKind::Thinking, thinking.clone()));
                            }
                        }
                        ContentBlock::ToolUse { name, input } => {
                            // `AskUserQuestion` is a decision, not generic tool
                            // activity — the pending `control_request` carries
                            // the renderable payload.
                            if name == crate::claude_session::ASK_USER_QUESTION_TOOL {
                                continue;
                            }
                            events.push(self.tool(name.clone(), input));
                        }
                    }
                }
                events
            }
            StreamEvent::Result {
                result,
                is_error,
                usage,
                total_cost_usd,
                ..
            } => {
                let mut events = Vec::new();
                // The result line carries cumulative per-turn usage. Assistant
                // lines also carry per-message usage; emitting both would
                // double count, so the turn-level result is authoritative.
                if let Some(usage) = usage {
                    events.push(self.usage(
                        usage.input_tokens,
                        usage.output_tokens,
                        total_cost_usd,
                    ));
                }
                let outcome = if !is_error {
                    TerminalOutcome::Completed
                } else if result.contains("Request interrupted by user") {
                    // Claude reports a user interrupt as an error result with
                    // this marker. `cancelled`/`killed` stay LoopDeck-owned and
                    // are never inferred from provider text.
                    TerminalOutcome::Interrupted
                } else {
                    TerminalOutcome::Error
                };
                if let Some(terminal) = self.terminal(outcome, Some(result)) {
                    events.push(terminal);
                }
                events
            }
            StreamEvent::ControlRequest {
                request_id,
                request,
            } => vec![self.claude_control_pending(&request_id, &request)],
            StreamEvent::ToolResult { .. } | StreamEvent::ControlResponse { .. } => Vec::new(),
        }
    }

    /// Pending narration for a Claude `control_request`.
    pub fn claude_control_pending(
        &mut self,
        request_id: &str,
        request: &crate::agents::ControlRequestBody,
    ) -> RunEvent {
        if request.tool_name == crate::claude_session::EXIT_PLAN_MODE_TOOL {
            let plan = parse_exit_plan(&request.input).unwrap_or_default();
            return self.plan_pending(request_id, &plan);
        }
        if request.tool_name == crate::claude_session::ASK_USER_QUESTION_TOOL {
            let questions = parse_ask_user_questions(&request.input);
            return self.question_pending(request_id, &questions);
        }
        self.approval_pending(request_id, &request.tool_name, &request.input)
    }

    // ── Codex ──────────────────────────────────────────────────────────────

    /// Convert one Codex app-server message or notification.
    pub fn convert_codex_message(&mut self, message: &Value) -> Vec<RunEvent> {
        let Some(method) = message.get("method").and_then(Value::as_str) else {
            return Vec::new();
        };
        let params = message.get("params").unwrap_or(&Value::Null);
        let request_id = message.get("id").map(display_request_id);
        match method {
            "turn/started" => self
                .started(
                    params
                        .get("threadId")
                        .and_then(Value::as_str)
                        .map(ToOwned::to_owned),
                )
                .into_iter()
                .collect(),
            "item/agentMessage/delta" => {
                let Some(delta) = params.get("delta").and_then(Value::as_str) else {
                    return Vec::new();
                };
                if let Some(id) = params.get("itemId").and_then(Value::as_str) {
                    self.codex_delta_items.insert(id.to_owned());
                }
                if delta.is_empty() {
                    Vec::new()
                } else {
                    vec![self.content(ContentKind::Text, delta.to_owned())]
                }
            }
            "item/reasoning/summaryTextDelta" | "item/reasoning/textDelta" => {
                let Some(delta) = params.get("delta").and_then(Value::as_str) else {
                    return Vec::new();
                };
                if delta.is_empty() {
                    Vec::new()
                } else {
                    vec![self.content(ContentKind::Thinking, delta.to_owned())]
                }
            }
            "item/started" => {
                let Some(item) = params.get("item") else {
                    return Vec::new();
                };
                let Some((name, input)) = crate::codex_session::tool_from_item(item) else {
                    return Vec::new();
                };
                let Some(id) = item.get("id").and_then(Value::as_str) else {
                    return vec![self.tool(name, &input)];
                };
                if !self.codex_tool_items.insert(id.to_owned()) {
                    return Vec::new();
                }
                vec![self.tool(name, &input)]
            }
            "item/completed" => {
                let Some(item) = params.get("item") else {
                    return Vec::new();
                };
                if let Some((name, input)) = crate::codex_session::collab_tool_completion(item) {
                    let id = item.get("id").and_then(Value::as_str).unwrap_or_default();
                    if !self.codex_tool_items.insert(format!("completed:{id}")) {
                        return Vec::new();
                    }
                    return vec![self.tool(name, &input)];
                }
                if item.get("type").and_then(Value::as_str) != Some("agentMessage") {
                    return Vec::new();
                }
                let Some(text) = item.get("text").and_then(Value::as_str) else {
                    return Vec::new();
                };
                let id = item.get("id").and_then(Value::as_str).unwrap_or_default();
                // Deltas are authoritative when present; the completed item is
                // the fallback for servers that suppress notifications.
                if text.is_empty() || self.codex_delta_items.contains(id) {
                    return Vec::new();
                }
                vec![self.content(ContentKind::Text, text.to_owned())]
            }
            "thread/tokenUsage/updated" => {
                let Some(last) = params.pointer("/tokenUsage/last") else {
                    return Vec::new();
                };
                vec![self.usage(
                    nonnegative_u64(last.get("inputTokens")),
                    nonnegative_u64(last.get("outputTokens")),
                    None,
                )]
            }
            "turn/completed" => {
                let turn = params.get("turn").unwrap_or(&Value::Null);
                let status = turn
                    .get("status")
                    .and_then(Value::as_str)
                    .unwrap_or("failed");
                let message = turn
                    .pointer("/error/message")
                    .and_then(Value::as_str)
                    .or_else(|| params.pointer("/error/message").and_then(Value::as_str));
                let outcome = match status {
                    "completed" => TerminalOutcome::Completed,
                    "interrupted" => TerminalOutcome::Interrupted,
                    "cancelled" => TerminalOutcome::Cancelled,
                    "killed" => TerminalOutcome::Killed,
                    _ => TerminalOutcome::Error,
                };
                self.terminal(outcome, message.map(ToOwned::to_owned))
                    .into_iter()
                    .collect()
            }
            "item/tool/requestUserInput" => {
                let questions = crate::codex_session::codex_questions(params);
                vec![self.question_pending(
                    request_id.as_deref().unwrap_or("codex-question"),
                    &questions,
                )]
            }
            "item/commandExecution/requestApproval"
            | "item/fileChange/requestApproval"
            | "item/permissions/requestApproval" => {
                let (tool_name, input) = crate::codex_session::approval_tool_context(
                    method,
                    params,
                    &Default::default(),
                );
                vec![self.approval_pending(
                    request_id.as_deref().unwrap_or("codex-approval"),
                    &tool_name,
                    &input,
                )]
            }
            // Codex has no plan-approval capability. If a future server sends
            // one, it is an explicit gap, not a relabelled ordinary approval.
            "item/plan/requestApproval" | "turn/plan/requestApproval" => {
                vec![self.unsupported("plan_approval", Some(redact_value(params)))]
            }
            "item/tool/call" => {
                vec![self.unsupported("client_declared_dynamic_tool", Some(redact_value(params)))]
            }
            // A bare `error` notification is diagnostic; the authoritative
            // outcome arrives on `turn/completed`.
            _ => Vec::new(),
        }
    }

    // ── Provider-neutral decision helpers ──────────────────────────────────

    pub fn approval_pending(
        &mut self,
        decision_id: &str,
        tool_name: &str,
        input: &Value,
    ) -> RunEvent {
        let summary = match input_narration(input) {
            Some(narration) => format!("{tool_name}: {narration}"),
            None => format!("{tool_name} approval requested"),
        };
        self.decision(
            decision_id,
            DecisionKind::Approval,
            DecisionState::Pending,
            Some(summary),
            Some(redact_value(&approval_detail(tool_name, input))),
        )
    }

    /// Resolved approval. `automatic` means Selasar's autonomous policy (or an
    /// explicit role rule) approved without parking the user — encoded in the
    /// summary, not in an enum variant.
    pub fn approval_resolved(
        &mut self,
        decision_id: &str,
        tool_name: &str,
        input: &Value,
        state: DecisionState,
        reason: &str,
        automatic: bool,
    ) -> RunEvent {
        let summary = match state {
            DecisionState::Allowed if automatic => format!("{AUTO_ALLOW_PREFIX} {tool_name}"),
            DecisionState::Allowed => {
                if reason.is_empty() {
                    format!("allow: {tool_name}")
                } else {
                    format!("allow: {tool_name}: {reason}")
                }
            }
            DecisionState::Denied => {
                if reason.is_empty() {
                    format!("deny: {tool_name}")
                } else {
                    format!("deny: {tool_name}: {reason}")
                }
            }
            DecisionState::Expired => format!("expired: {tool_name}"),
            DecisionState::Pending | DecisionState::Answered => {
                format!("resolved: {tool_name}")
            }
        };
        self.decision(
            decision_id,
            DecisionKind::Approval,
            state,
            Some(summary),
            Some(redact_value(&approval_detail(tool_name, input))),
        )
    }

    pub fn question_pending(
        &mut self,
        decision_id: &str,
        questions: &[AskUserQuestionSpec],
    ) -> RunEvent {
        let summary = questions.first().map(|question| question.question.clone());
        let detail = serde_json::to_value(questions)
            .ok()
            .map(|value| redact_value(&value));
        self.decision(
            decision_id,
            DecisionKind::Question,
            DecisionState::Pending,
            summary,
            detail,
        )
    }

    pub fn question_answered(
        &mut self,
        decision_id: &str,
        questions: &[AskUserQuestionSpec],
    ) -> RunEvent {
        let summary = questions
            .first()
            .map(|question| format!("answered: {}", question.question));
        self.decision(
            decision_id,
            DecisionKind::Question,
            DecisionState::Answered,
            summary,
            None,
        )
    }

    pub fn plan_pending(&mut self, decision_id: &str, plan: &str) -> RunEvent {
        let summary = format!("plan: {}", first_line(plan));
        self.decision(
            decision_id,
            DecisionKind::PlanApproval,
            DecisionState::Pending,
            Some(summary),
            Some(redact_text(plan)),
        )
    }

    pub fn plan_resolved(
        &mut self,
        decision_id: &str,
        plan: &str,
        state: DecisionState,
        reason: &str,
        automatic: bool,
    ) -> RunEvent {
        let summary = match state {
            DecisionState::Allowed if automatic => format!("{AUTO_ALLOW_PREFIX} plan"),
            DecisionState::Allowed => "allow: plan".to_owned(),
            DecisionState::Denied => format!("deny: plan: {reason}"),
            _ => "resolved: plan".to_owned(),
        };
        self.decision(
            decision_id,
            DecisionKind::PlanApproval,
            state,
            Some(summary),
            Some(redact_text(plan)),
        )
    }
}

/// Transient retry signal emitted by the command layer between provider
/// attempts. It is deliberately standalone: no adapter turn is active during
/// the backoff, and the retry is not a durable terminal.
pub fn retrying_event(
    run_id: &str,
    provider: Provider,
    attempt: u32,
    max_attempts: u32,
    error: &str,
) -> RunEvent {
    let mut normalizer = RunEventNormalizer::new(run_id, provider);
    normalizer.retrying(attempt, max_attempts, error)
}

/// Map an adapter error to the LoopDeck-owned terminal it represents.
///
/// `cancelled`/`killed` are never inferred from provider text — they come from
/// the adapter's own error taxonomy: a budget kill is `killed`, a turn parked
/// past its deadline (nobody answered) is `cancelled`, and an explicit user
/// interrupt is `interrupted`.
pub fn terminal_outcome_for_error(error: &crate::error::AppError) -> TerminalOutcome {
    match error {
        crate::error::AppError::Limit(_) => TerminalOutcome::Killed,
        crate::error::AppError::TurnParked { .. } => TerminalOutcome::Cancelled,
        crate::error::AppError::Agent(message)
            if message.to_ascii_lowercase().contains("interrupted") =>
        {
            TerminalOutcome::Interrupted
        }
        _ => TerminalOutcome::Error,
    }
}

fn nonnegative_u64(value: Option<&Value>) -> u64 {
    value.and_then(Value::as_u64).unwrap_or(0)
}

fn display_request_id(id: &Value) -> String {
    match id {
        Value::String(value) => value.clone(),
        other => other.to_string(),
    }
}

/// Outbound `RunEvent` sink used by an adapter for one turn.
///
/// Owns the normalizer and channel so adapter call sites are a single
/// `if let Some(sink) = self.run_events.as_mut() { sink... }`.
pub struct RunEventSink {
    normalizer: RunEventNormalizer,
    channel: Channel<RunEvent>,
}

impl RunEventSink {
    pub fn new(run_id: impl Into<String>, provider: Provider, channel: Channel<RunEvent>) -> Self {
        Self {
            normalizer: RunEventNormalizer::new(run_id, provider),
            channel,
        }
    }

    pub fn normalizer(&mut self) -> &mut RunEventNormalizer {
        &mut self.normalizer
    }

    /// Send one event. Channel failures are intentionally best-effort, exactly
    /// like the legacy channel: a closed UI must not abort the turn.
    pub fn send(&self, event: RunEvent) {
        let _ = self.channel.send(event);
    }

    pub fn send_all(&self, events: Vec<RunEvent>) {
        for event in events {
            self.send(event);
        }
    }

    pub fn convert_claude_line(&mut self, line: &str) {
        let events = self.normalizer.convert_claude_line(line);
        self.send_all(events);
    }

    pub fn convert_codex_message(&mut self, message: &Value) {
        let events = self.normalizer.convert_codex_message(message);
        self.send_all(events);
    }

    pub fn terminal(&mut self, outcome: TerminalOutcome, message: Option<String>) {
        if let Some(event) = self.normalizer.terminal(outcome, message) {
            self.send(event);
        }
    }

    pub fn approval_pending(&mut self, decision_id: &str, tool_name: &str, input: &Value) {
        let event = self
            .normalizer
            .approval_pending(decision_id, tool_name, input);
        self.send(event);
    }

    #[allow(clippy::too_many_arguments)]
    pub fn approval_resolved(
        &mut self,
        decision_id: &str,
        tool_name: &str,
        input: &Value,
        state: DecisionState,
        reason: &str,
        automatic: bool,
    ) {
        let event = self.normalizer.approval_resolved(
            decision_id,
            tool_name,
            input,
            state,
            reason,
            automatic,
        );
        self.send(event);
    }

    pub fn question_pending(&mut self, decision_id: &str, questions: &[AskUserQuestionSpec]) {
        let event = self.normalizer.question_pending(decision_id, questions);
        self.send(event);
    }

    pub fn question_answered(&mut self, decision_id: &str, questions: &[AskUserQuestionSpec]) {
        let event = self.normalizer.question_answered(decision_id, questions);
        self.send(event);
    }

    pub fn unsupported(&mut self, capability: &str, detail: Option<String>) {
        let event = self.normalizer.unsupported(capability, detail);
        self.send(event);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn claude_normalizer() -> RunEventNormalizer {
        RunEventNormalizer::new("run-claude", Provider::Claude)
    }

    fn codex_normalizer() -> RunEventNormalizer {
        RunEventNormalizer::new("run-codex", Provider::Codex)
    }

    #[test]
    fn claude_completed_line_produces_one_terminal_and_usage() {
        let mut normalizer = claude_normalizer();
        let lines = [
            r#"{"type":"system","subtype":"init","session_id":"s-1"}"#,
            r#"{"type":"assistant","message":{"content":[{"type":"text","text":"hi"},{"type":"tool_use","id":"t1","name":"Read","input":{"file_path":"/repo/a.rs"}}]},"session_id":"s-1"}"#,
            r#"{"type":"result","result":"done","is_error":false,"duration_ms":10,"total_cost_usd":0.1,"usage":{"input_tokens":5,"output_tokens":3}}"#,
        ];
        let mut payloads = Vec::new();
        for line in lines {
            payloads.extend(normalizer.convert_claude_line(line));
        }
        assert!(matches!(
            payloads[0].payload,
            RunEventPayload::Started { .. }
        ));
        assert!(matches!(
            payloads[1].payload,
            RunEventPayload::Content { ref text, .. } if text == "hi"
        ));
        assert!(matches!(
            payloads[2].payload,
            RunEventPayload::Tool { ref name, .. } if name == "Read"
        ));
        assert!(matches!(
            payloads[3].payload,
            RunEventPayload::Usage {
                input_tokens: 5,
                output_tokens: 3,
                total_cost_usd: Some(cost),
            } if (cost - 0.1).abs() < f64::EPSILON
        ));
        assert!(matches!(
            payloads[4].payload,
            RunEventPayload::Terminal {
                outcome: TerminalOutcome::Completed,
                ..
            }
        ));
        assert_eq!(payloads.len(), 5);
    }

    #[test]
    fn duplicate_terminal_lines_are_suppressed_per_segment() {
        let mut normalizer = claude_normalizer();
        let line = r#"{"type":"result","result":"failed","is_error":true,"duration_ms":1,"usage":{"input_tokens":1,"output_tokens":1}}"#;
        let first = normalizer.convert_claude_line(line);
        let second = normalizer.convert_claude_line(line);
        assert_eq!(first.len(), 2, "usage + terminal");
        assert_eq!(second.len(), 1, "usage only; terminal is gated");
        assert!(normalizer.terminal_emitted());
        assert!(normalizer
            .terminal(TerminalOutcome::Interrupted, None)
            .is_none());
    }

    #[test]
    fn claude_interrupt_marker_is_interrupted_not_error() {
        let mut normalizer = claude_normalizer();
        let events = normalizer.convert_claude_line(
            r#"{"type":"result","result":"[Request interrupted by user]","is_error":true,"duration_ms":1,"usage":{"input_tokens":1,"output_tokens":1}}"#,
        );
        assert!(matches!(
            events.last().expect("terminal").payload,
            RunEventPayload::Terminal {
                outcome: TerminalOutcome::Interrupted,
                ..
            }
        ));
    }

    #[test]
    fn automatic_approval_uses_allowed_state_and_marker_summary() {
        let mut normalizer = claude_normalizer();
        let event = normalizer.approval_resolved(
            "req-1",
            "Bash",
            &json!({"command": "cargo test"}),
            DecisionState::Allowed,
            "",
            true,
        );
        match event.payload {
            RunEventPayload::Decision { state, summary, .. } => {
                assert_eq!(state, DecisionState::Allowed);
                assert!(summary.expect("summary").starts_with(AUTO_ALLOW_PREFIX));
            }
            other => panic!("expected decision, got {other:?}"),
        }
    }

    #[test]
    fn tool_inputs_and_terminal_messages_are_redacted() {
        // Build the credential-shaped value at runtime so this test's own
        // source cannot trip the pre-push secret scan it exercises.
        let fake_token = format!("gh{}_1234567890abcdefghijklmnopqrstuvwx", 'p');
        let line = format!(
            r#"{{"type":"assistant","message":{{"content":[{{"type":"tool_use","id":"t1","name":"Bash","input":{{"command":"export TOKEN={fake_token}"}}}}]}},"session_id":"s"}}"#
        );
        let mut normalizer = claude_normalizer();
        let events = normalizer.convert_claude_line(&line);
        match &events[0].payload {
            RunEventPayload::Tool { input_redacted, .. } => {
                assert!(!input_redacted.contains(&fake_token));
                assert!(input_redacted.contains("[redacted]"));
            }
            other => panic!("expected tool, got {other:?}"),
        }
    }

    #[test]
    fn codex_subagent_activity_preserves_call_and_child_status_without_finishing_turn() {
        let mut normalizer = codex_normalizer();
        let mut item = json!({
            "id": "spawn-1", "type": "collabAgentToolCall", "tool": "spawnAgent",
            "status": "inProgress", "model": "small-model", "prompt": "Check recovery",
            "receiverThreadIds": [], "agentsStates": {}
        });
        let started = json!({"method": "item/started", "params": {"item": item.clone()}});
        let events = normalizer.convert_codex_message(&started);
        assert!(
            matches!(&events[0].payload, RunEventPayload::Tool { name, .. } if name == "Sub-agent spawnAgent")
        );
        assert!(normalizer.convert_codex_message(&started).is_empty());
        item["status"] = json!("completed");
        item["receiverThreadIds"] = json!(["child-1"]);
        item["agentsStates"] = json!({"child-1": {"status": "running"}});
        let completed = json!({"method": "item/completed", "params": {"item": item.clone()}});
        let events = normalizer.convert_codex_message(&completed);
        match &events[0].payload {
            RunEventPayload::Tool {
                name,
                input_redacted,
                ..
            } => {
                assert_eq!(name, "Sub-agent spawnAgent result");
                let input: Value = serde_json::from_str(input_redacted).unwrap();
                assert_eq!(input["status"], "completed");
                assert_eq!(input["agentsStates"]["child-1"]["status"], "running");
            }
            other => panic!("expected tool activity, got {other:?}"),
        }
        assert!(normalizer.convert_codex_message(&completed).is_empty());
        assert!(!normalizer.terminal_emitted);
        // Some servers suppress start notifications. Keep the result anyway.
        item["id"] = json!("wait-1");
        item["tool"] = json!("wait");
        item["status"] = json!("failed");
        assert_eq!(
            normalizer
                .convert_codex_message(&json!({"method":"item/completed", "params":{"item":item}}))
                .len(),
            1
        );
    }

    #[test]
    fn codex_deltas_replace_the_completed_agent_message() {
        let mut normalizer = codex_normalizer();
        let messages = [
            json!({"method":"item/agentMessage/delta","params":{"itemId":"m1","delta":"hello "}}),
            json!({"method":"item/agentMessage/delta","params":{"itemId":"m1","delta":"world"}}),
            json!({"method":"item/completed","params":{"item":{"id":"m1","type":"agentMessage","text":"hello world"}}}),
        ];
        let mut texts = Vec::new();
        for message in messages {
            texts.extend(
                normalizer
                    .convert_codex_message(&message)
                    .into_iter()
                    .filter_map(|event| match event.payload {
                        RunEventPayload::Content { text, .. } => Some(text),
                        _ => None,
                    }),
            );
        }
        assert_eq!(texts, vec!["hello ", "world"]);
    }

    #[test]
    fn codex_plan_request_is_unsupported() {
        let mut normalizer = codex_normalizer();
        let events = normalizer.convert_codex_message(&json!({
            "id": 7,
            "method": "item/plan/requestApproval",
            "params": {"plan": "do it"}
        }));
        assert!(matches!(
            events[0].payload,
            RunEventPayload::Unsupported {
                ref capability, ..
            } if capability == "plan_approval"
        ));
    }

    #[test]
    fn codex_terminal_status_maps_to_terminal_outcome() {
        for (status, expected) in [
            ("completed", TerminalOutcome::Completed),
            ("failed", TerminalOutcome::Error),
            ("interrupted", TerminalOutcome::Interrupted),
            ("cancelled", TerminalOutcome::Cancelled),
            ("killed", TerminalOutcome::Killed),
        ] {
            let mut normalizer = codex_normalizer();
            let events = normalizer.convert_codex_message(&json!({
                "method": "turn/completed",
                "params": {"turn": {"status": status}}
            }));
            assert!(matches!(
                events.last().expect("terminal").payload,
                RunEventPayload::Terminal { ref outcome, .. } if *outcome == expected
            ));
        }
    }

    #[test]
    fn retry_events_are_transient_and_do_not_reset_terminal_gate() {
        let mut normalizer = claude_normalizer();
        let _ = normalizer.terminal(TerminalOutcome::Error, None);
        let retrying = normalizer.retrying(1, 4, "overloaded");
        assert!(matches!(
            retrying.payload,
            RunEventPayload::Retrying { attempt: 1, .. }
        ));
        assert!(normalizer.terminal_emitted());
    }
}
