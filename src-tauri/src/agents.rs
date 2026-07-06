use crate::config::AgentConfig;
use serde::{Deserialize, Serialize};
use tokio::process::Command;

pub(crate) trait CommandEnv {
    fn env<K, V>(&mut self, key: K, val: V) -> &mut Self
    where
        K: AsRef<std::ffi::OsStr>,
        V: AsRef<std::ffi::OsStr>;
}

impl CommandEnv for std::process::Command {
    fn env<K, V>(&mut self, key: K, val: V) -> &mut Self
    where
        K: AsRef<std::ffi::OsStr>,
        V: AsRef<std::ffi::OsStr>,
    {
        std::process::Command::env(self, key, val)
    }
}

impl CommandEnv for tokio::process::Command {
    fn env<K, V>(&mut self, key: K, val: V) -> &mut Self
    where
        K: AsRef<std::ffi::OsStr>,
        V: AsRef<std::ffi::OsStr>,
    {
        tokio::process::Command::env(self, key, val)
    }
}

/// Structured result from a `call_agents` invocation.
#[derive(Debug, Clone, Serialize)]
pub struct AgentResponse {
    /// The concatenated text from assistant `text` blocks (streaming deltas).
    pub text: String,
    /// Raw thinking content, if the model returned it.
    pub thinking: Option<String>,
    /// The final `result` event's `result` field (the complete answer).
    pub result: String,
    /// Token usage and cost from the final `result` event.
    pub usage: Option<UsageInfo>,
    /// Whether the final result event indicated an error.
    pub is_error: bool,
    /// Wall-clock duration in milliseconds.
    pub duration_ms: u64,

    pub session_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct UsageInfo {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub total_cost_usd: f64,
}

// ── Streaming events (frontend-facing) ─────────────────────────────────────

/// Events emitted to the frontend via `Channel<ClaudeEvent>` during a
/// streaming turn. Each event maps to one content block from the NDJSON stream
/// (or the terminal `result`), so the UI can render tokens as they arrive.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ClaudeEvent {
    /// A text delta — one `ContentBlock::Text` from an `assistant` message.
    TextDelta { text: String },
    /// A thinking delta — one `ContentBlock::Thinking` from an `assistant`
    /// message (only present when the model uses extended thinking).
    ThinkingDelta { thinking: String },
    /// A tool call — one `ContentBlock::ToolUse` from an `assistant` message.
    /// Surfaced so the UI can show live activity ("Reading X", "Editing Y")
    /// during long agentic turns where text deltas are sparse.
    ToolUse {
        /// Tool name, e.g. "Read", "Edit", "Bash".
        name: String,
        /// The raw tool input object, serialized to a JSON string. The UI
        /// decides how much to show (path, command, etc.).
        input: String,
    },
    /// The terminal event signalling the turn is complete. Carries the full
    /// aggregated `AgentResponse` so the UI can reconcile its streamed deltas
    /// and display usage/duration in a single payload.
    Result {
        /// Concatenated text from all text deltas in this turn.
        text: String,
        /// Aggregated thinking (if any). May be empty when thinking is absent.
        thinking: Option<String>,
        /// The final `result` field from the `result` event.
        result: String,
        /// Token usage + cost from the `result` event, when available.
        usage: Option<UsageInfo>,
        /// Whether this turn errored (from the `result` event).
        is_error: bool,
        /// Wall-clock duration of the turn in milliseconds.
        duration_ms: u64,
        /// The session id (drives `--resume`). Present on all assistant turns.
        session_id: String,
    },
}

// ── NDJSON stream event types (internal) ───────────────────────────────────
//
// The `--output-format stream-json` flag produces one JSON object per line.
// We only model the fields we care about; serde ignores the rest.

#[derive(Debug, Deserialize)]
#[serde(tag = "type")]
pub(crate) enum StreamEvent {
    #[serde(rename = "system")]
    System,
    #[serde(rename = "assistant")]
    Assistant {
        message: AssistantMessage,
        session_id: String,
    },

    #[serde(rename = "result")]
    Result {
        result: String,
        is_error: bool,
        duration_ms: u64,
        #[serde(default)]
        total_cost_usd: Option<f64>,
        #[serde(default)]
        usage: Option<RawUsage>,
    },
}

#[derive(Debug, Deserialize)]
pub(crate) struct AssistantMessage {
    pub(crate) content: Vec<ContentBlock>,
}

/// Default for `ContentBlock::ToolUse::input` when the block omits it.
/// An empty object (not `null`) so downstream `input["field"]` lookups are safe.
fn default_tool_input() -> serde_json::Value {
    serde_json::json!({})
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type")]
pub(crate) enum ContentBlock {
    #[serde(rename = "text")]
    Text { text: String },
    #[serde(rename = "thinking")]
    Thinking { thinking: String },
    // `name` is always present on a tool_use block; `input` may be absent or
    // null in edge cases, so default to an empty object. We capture both so
    // the streaming UI can show live activity during agentic turns.
    #[serde(rename = "tool_use")]
    ToolUse {
        #[serde(default)]
        name: String,
        #[serde(default = "default_tool_input")]
        input: serde_json::Value,
    },
}

#[derive(Debug, Deserialize)]
pub(crate) struct RawUsage {
    input_tokens: u64,
    output_tokens: u64,
}

// ── Parsing ────────────────────────────────────────────────────────────────

/// Incrementally accumulate stream events into an `AgentResponse`.
///
/// Shared between `parse_response` (which drains a full NDJSON blob at once)
/// and `ClaudeSession::send_message` (which feeds events line-by-line as they
/// arrive over the persistent process's stdout). Keeping one accumulation
/// implementation means the two paths can never drift.
#[derive(Default)]
pub(crate) struct ResponseAccumulator {
    text: String,
    thinking: Option<String>,
    result_text: String,
    is_error: bool,
    duration_ms: u64,
    usage: Option<UsageInfo>,
    session_id: String,
}

impl ResponseAccumulator {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    /// Feed one parsed stream event. Returns true if this event was the
    /// terminal `Result` event — callers drive their read loop off this.
    #[must_use]
    pub(crate) fn ingest_event(&mut self, event: StreamEvent) -> bool {
        match event {
            StreamEvent::Assistant {
                message,
                session_id: sid,
            } => {
                for block in message.content {
                    match block {
                        ContentBlock::Text { text: t } => self.text.push_str(&t),
                        ContentBlock::Thinking { thinking: th } => {
                            self.thinking.get_or_insert_default().push_str(&th);
                        }
                        // Tool-use blocks aren't part of the aggregated text —
                        // they're streamed live via ClaudeEvent::ToolUse in
                        // `send_message_streaming`, so nothing to accumulate here.
                        ContentBlock::ToolUse { .. } => {}
                    }
                }
                self.session_id = sid;
                false
            }
            StreamEvent::Result {
                result: r,
                is_error: e,
                duration_ms: d,
                total_cost_usd,
                usage: u,
            } => {
                self.result_text = r;
                self.is_error = e;
                self.duration_ms = d;
                self.usage = u.map(|u| UsageInfo {
                    input_tokens: u.input_tokens,
                    output_tokens: u.output_tokens,
                    total_cost_usd: total_cost_usd.unwrap_or(0.0),
                });
                true
            }
            StreamEvent::System => false,
        }
    }

    /// Produce the final response. Returns `None` if we never saw any
    /// assistant text or a result event — useful for the caller to detect
    /// "claude produced nothing".
    pub(crate) fn finish(self) -> Option<AgentResponse> {
        if self.result_text.is_empty() && self.text.is_empty() {
            return None;
        }

        Some(AgentResponse {
            text: self.text,
            thinking: self.thinking,
            result: self.result_text,
            usage: self.usage,
            is_error: self.is_error,
            duration_ms: self.duration_ms,
            session_id: self.session_id,
        })
    }
}

/// Parse a single NDJSON line into a `StreamEvent`, if it is one.
///
/// Non-JSON / unrecognized lines (blank lines, stray stderr leaking into the
/// stream, etc.) silently yield `None` — callers decide whether to skip or err.
pub(crate) fn parse_stream_line(line: &str) -> Option<StreamEvent> {
    serde_json::from_str::<StreamEvent>(line.trim()).ok()
}

/// Parse a full NDJSON string into a structured `AgentResponse`.
fn parse_response(ndjson: &str) -> Option<AgentResponse> {
    let mut acc = ResponseAccumulator::new();
    for line in ndjson.lines().map(str::trim).filter(|l| !l.is_empty()) {
        if let Some(event) = parse_stream_line(line) {
            // The returned "is_result" flag is meaningless here: we drain the
            // whole blob regardless. Batch parse doesn't short-circuit.
            let _ = acc.ingest_event(event);
        }
    }
    acc.finish()
}

// ── Agent runner ───────────────────────────────────────────────────────────

/// Apply agent config as environment variables on a `Command`.
///
/// Extracted for testability — allows verifying env vars without spawning.
/// Also reused by `ClaudeSession::spawn`, so both the single-shot
/// (`call_agents`) and persistent (`ClaudeSession`) paths stay in sync.
pub(crate) fn apply_agent_config<C: CommandEnv>(cmd: &mut C, agent_config: &AgentConfig) {
    if let Some(auth_token) = &agent_config.auth_token {
        cmd.env("ANTHROPIC_AUTH_TOKEN", auth_token);
    }

    if let Some(base_url) = &agent_config.base_url {
        cmd.env("ANTHROPIC_BASE_URL", base_url);
    }

    if let Some(model) = &agent_config.model {
        cmd.env("ANTHROPIC_MODEL", model);
    }

    if let Some(effort) = &agent_config.effort {
        cmd.env("CLAUDE_CODE_EFFORT_LEVEL", effort);
    }
}

// ── Tests ──────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;

    #[test]
    fn test_parse_response_extracts_text_blocks() {
        let ndjson = concat!(
            r#"{"type":"system","subtype":"init"}"#,
            "\n",
            r#"{"type":"assistant","message":{"content":[{"type":"text","text":"Hello "}]}, "session_id":"session_id"}"#,
            "\n",
            r#"{"type":"assistant","message":{"content":[{"type":"text","text":"world"}]}, "session_id":"session_id"}"#,
            "\n",
            r#"{"type":"result","result":"Hello world","is_error":false,"duration_ms":1500,"usage":{"input_tokens":100,"output_tokens":20},"total_cost_usd":0.005}"#,
        );

        let response = parse_response(ndjson).expect("should parse");

        assert_eq!(response.text, "Hello world");
        assert_eq!(response.result, "Hello world");
        assert!(!response.is_error);
        assert_eq!(response.duration_ms, 1500);

        let usage = response.usage.expect("should have usage");
        assert_eq!(usage.input_tokens, 100);
        assert_eq!(usage.output_tokens, 20);
        assert_eq!(usage.total_cost_usd, 0.005);
    }

    #[test]
    fn test_deser_single_text_block() {
        // Verify ContentBlock parses a text block directly
        let json = r#"{"type":"text","text":"hi"}"#;
        let block: ContentBlock = serde_json::from_str(json).expect("parse text block");
        match block {
            ContentBlock::Text { text } => assert_eq!(text, "hi"),
            other => panic!("expected Text, got {:?}", other),
        }
    }

    #[test]
    fn test_deser_tool_use_block_captures_name_and_input() {
        // Regression guard for the silent-activity bug: tool_use blocks must
        // parse their `name` and `input` so the streaming UI can show live
        // activity during agentic turns. If this regresses to a unit variant,
        // long turns show only a spinner again.
        let json = r#"{"type":"tool_use","id":"toolu_01","name":"Read","input":{"file_path":"/a/b.rs"}}"#;
        let block: ContentBlock = serde_json::from_str(json).expect("parse tool_use block");
        match block {
            ContentBlock::ToolUse { name, input } => {
                assert_eq!(name, "Read");
                assert_eq!(input["file_path"], "/a/b.rs");
            }
            other => panic!("expected ToolUse, got {:?}", other),
        }
    }

    #[test]
    fn test_deser_tool_use_block_handles_missing_input() {
        // `input` defaults to an empty object — a tool_use without input must
        // not fail to parse (would drop the whole assistant message).
        let json = r#"{"type":"tool_use","id":"toolu_02","name":"Bash"}"#;
        let block: ContentBlock = serde_json::from_str(json).expect("parse tool_use block");
        match block {
            ContentBlock::ToolUse { name, input } => {
                assert_eq!(name, "Bash");
                assert!(input.is_object());
                assert!(input.as_object().unwrap().is_empty());
            }
            other => panic!("expected ToolUse, got {:?}", other),
        }
    }

    #[test]
    fn test_deser_assistant_with_thinking_and_text() {
        // Verify the full assistant message with mixed content parses
        let json = r#"{"type":"assistant","message":{"content":[
            {"type":"thinking","thinking":"I should greet the user"},
            {"type":"text","text":"hi"}
        ]},"session_id":"session_id"}"#;
        let event: StreamEvent = serde_json::from_str(json).expect("parse event");
        match event {
            StreamEvent::Assistant {
                message,
                session_id,
            } => {
                assert_eq!(
                    message.content.len(),
                    2,
                    "expected 2 content blocks, got {:?}",
                    message.content
                );
                assert!(!session_id.is_empty());
            }
            other => panic!("expected Assistant, got {:?}", other),
        }
    }

    #[test]
    fn test_parse_response_captures_thinking() {
        // NDJSON = each JSON object on a SINGLE line, no pretty-printing
        let ndjson = concat!(
            r#"{"type":"assistant","message":{"content":[{"type":"thinking","thinking":"I should greet the user"},{"type":"text","text":"hi"}]}, "session_id":"session_id"}"#,
            "\n",
            r#"{"type":"result","result":"hi","is_error":false,"duration_ms":42}"#,
        );

        let response = parse_response(ndjson).expect("should parse");

        assert_eq!(response.text, "hi");
        assert_eq!(
            response.thinking.as_deref(),
            Some("I should greet the user")
        );
    }

    #[test]
    fn test_parse_response_empty_input() {
        assert!(parse_response("").is_none());
        assert!(parse_response("garbage\n").is_none());
    }

    // ── apply_agent_config tests ─────────────────────────────────────────

    /// Helper: collect env vars from a Command as Vec<(String, String)>.
    fn get_envs(cmd: &Command) -> Vec<(String, String)> {
        cmd.get_envs()
            .filter_map(|(k, v)| {
                Some((
                    k.to_string_lossy().into_owned(),
                    v?.to_string_lossy().into_owned(),
                ))
            })
            .collect()
    }

    #[test]
    fn test_apply_agent_config_all_fields() {
        let config = AgentConfig {
            auth_token: Some("sk-test-token".into()),
            base_url: Some("https://api.example.com".into()),
            model: Some("claude-opus-4-8".into()),
            effort: Some("max".into()),
        };

        let mut cmd = Command::new("claude");
        apply_agent_config(&mut cmd, &config);

        let envs = get_envs(&cmd);
        assert!(envs.contains(&("ANTHROPIC_AUTH_TOKEN".into(), "sk-test-token".into())));
        assert!(envs.contains(&(
            "ANTHROPIC_BASE_URL".into(),
            "https://api.example.com".into()
        )));
        assert!(envs.contains(&("ANTHROPIC_MODEL".into(), "claude-opus-4-8".into())));
        assert!(envs.contains(&("CLAUDE_CODE_EFFORT_LEVEL".into(), "max".into())));
    }

    #[test]
    fn test_apply_agent_config_empty_fields_set_nothing() {
        let config = AgentConfig {
            auth_token: None,
            base_url: None,
            model: None,
            effort: None,
        };

        let mut cmd = Command::new("claude");

        // Record env vars that *already exist* after construction (inherited from
        // the test process). Then apply config and verify no NEW agent-specific vars
        // were added.
        let before: Vec<String> = cmd
            .get_envs()
            .map(|(k, _)| k.to_string_lossy().into_owned())
            .collect();

        apply_agent_config(&mut cmd, &config);

        let after: Vec<String> = cmd
            .get_envs()
            .map(|(k, _)| k.to_string_lossy().into_owned())
            .collect();

        let agent_vars = [
            "ANTHROPIC_AUTH_TOKEN",
            "ANTHROPIC_BASE_URL",
            "ANTHROPIC_MODEL",
            "CLAUDE_CODE_EFFORT_LEVEL",
        ];

        for var in agent_vars {
            let was_present_before = before.contains(&var.to_string());
            let is_present_after = after.contains(&var.to_string());
            assert_eq!(
                was_present_before, is_present_after,
                "{var} should not have been added"
            );
        }
    }

    #[test]
    fn test_apply_agent_config_partial_fields() {
        // Only set base_url and model — auth_token and effort remain None
        let config = AgentConfig {
            auth_token: None,
            base_url: Some("https://api.deepseek.com/anthropic".into()),
            model: Some("deepseek-v4-pro[1m]".into()),
            effort: None,
        };

        let mut cmd = Command::new("claude");
        apply_agent_config(&mut cmd, &config);

        let envs = get_envs(&cmd);

        // Should contain the fields we set
        assert!(envs.contains(&(
            "ANTHROPIC_BASE_URL".into(),
            "https://api.deepseek.com/anthropic".into()
        )));
        assert!(envs.contains(&("ANTHROPIC_MODEL".into(), "deepseek-v4-pro[1m]".into())));

        // Should NOT contain auth_token or effort
        let env_keys: Vec<&str> = envs.iter().map(|(k, _)| k.as_str()).collect();
        assert!(!env_keys.contains(&"ANTHROPIC_AUTH_TOKEN"));
        assert!(!env_keys.contains(&"CLAUDE_CODE_EFFORT_LEVEL"));
    }

    // Integration test, disable since it rely on calling the real agent
    // #[test]
    // fn test_call_agents() {
    //     let config = AgentConfig {
    //         base_url: Some(String::from("https://api.deepseek.com/anthropic")),
    //         model: Some(String::from("deepseek-v4-pro[1m]")),
    //         auth_token: Some(String::from("sk-64a7220f24e241dc8139ba445cd634f0")),
    //         effort: Some(String::from("max")),
    //     };
    //     let result = call_agents(String::from("reply with exactly: hello"), &config);
    //     assert!(result.is_ok(), "call_agents failed: {:?}", result.err());

    //     let response = result.unwrap();
    //     assert!(!response.text.is_empty(), "text should not be empty");
    //     assert!(
    //         response.text.contains("hello"),
    //         "text should contain 'hello', got: {}",
    //         response.text
    //     );
    //     assert!(!response.result.is_empty(), "result should not be empty");
    //     assert!(!response.is_error, "should not be an error");
    // }
}
