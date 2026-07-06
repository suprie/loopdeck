use crate::config::AgentConfig;
use crate::error::AppError;
use serde::{Deserialize, Serialize};
use std::process::Command;

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

#[derive(Debug, Clone, Serialize)]
pub struct UsageInfo {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub total_cost_usd: f64,
}

// ── NDJSON stream event types (internal) ───────────────────────────────────
//
// The `--output-format stream-json` flag produces one JSON object per line.
// We only model the fields we care about; serde ignores the rest.

#[derive(Debug, Deserialize)]
#[serde(tag = "type")]
enum StreamEvent {
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
struct AssistantMessage {
    content: Vec<ContentBlock>,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type")]
enum ContentBlock {
    #[serde(rename = "text")]
    Text { text: String },
    #[serde(rename = "thinking")]
    Thinking { thinking: String },
    #[serde(rename = "tool_use")]
    ToolUse,
}

#[derive(Debug, Deserialize)]
struct RawUsage {
    input_tokens: u64,
    output_tokens: u64,
}

// ── Parsing ────────────────────────────────────────────────────────────────

/// Parse a full NDJSON stream into a structured `AgentResponse`.
pub fn parse_response(ndjson: &str) -> Option<AgentResponse> {
    let mut text = String::new();
    let mut thinking: Option<String> = None;
    let mut result_text = String::new();
    let mut is_error = false;
    let mut duration_ms = 0u64;
    let mut usage: Option<UsageInfo> = None;
    let mut session_id = String::new();

    for event in parse_stream_events(ndjson) {
        match event {
            StreamEvent::Assistant {
                message,
                session_id: sid,
            } => {
                for block in message.content {
                    match block {
                        ContentBlock::Text { text: t } => text.push_str(&t),
                        ContentBlock::Thinking { thinking: th } => {
                            thinking.get_or_insert_default().push_str(&th);
                        }
                        ContentBlock::ToolUse => { /* not collected yet */ }
                    }
                }
                session_id = sid;
            }
            StreamEvent::Result {
                result: r,
                is_error: e,
                duration_ms: d,
                total_cost_usd,
                usage: u,
            } => {
                result_text = r;
                is_error = e;
                duration_ms = d;
                usage = u.map(|u| UsageInfo {
                    input_tokens: u.input_tokens,
                    output_tokens: u.output_tokens,
                    total_cost_usd: total_cost_usd.unwrap_or(0.0),
                });
            }
            StreamEvent::System => { /* metadata, skip */ }
        }
    }

    if result_text.is_empty() && text.is_empty() {
        return None;
    }

    Some(AgentResponse {
        text,
        thinking,
        result: result_text,
        usage,
        is_error,
        duration_ms,
        session_id: session_id,
    })
}

/// Lazily parse an NDJSON string into typed `StreamEvent` items.
fn parse_stream_events(ndjson: &str) -> impl Iterator<Item = StreamEvent> + '_ {
    ndjson
        .lines()
        .filter(|line| !line.trim().is_empty())
        .filter_map(|line| serde_json::from_str::<StreamEvent>(line).ok())
}

// ── Agent runner ───────────────────────────────────────────────────────────

/// Call claude with a prompt and return the structured response.
///
/// Uses `--print` for non-interactive mode and `--output-format stream-json --verbose`
/// to get structured JSON output that can be parsed for the response text.
pub fn call_agents(prompt: String, agent_config: &AgentConfig) -> Result<AgentResponse, AppError> {
    let mut cmd = Command::new("claude");
    // NOTE: do NOT pass --input-format stream-json — it makes claude read from stdin,
    // and Command::output() provides /dev/null on stdin, yielding zero output.
    cmd.args(["-p", "--output-format", "stream-json", "--verbose"]);
    cmd.arg(&prompt);

    // Provider / model configuration via environment variables
    if let Some(auth_token) = &agent_config.auth_token {
        cmd.env("ANTHROPIC_AUTH_TOKEN", auth_token);
    }

    if let Some(base_url) = &agent_config.base_url {
        cmd.env("ANTHROPIC_BASE_URL", base_url);
    }

    if let Some(model) = &agent_config.model {
        cmd.env("ANTROPHIC_MODEL", model);
    }

    debug_command(&cmd);

    let output = cmd
        .output()
        .map_err(|e| AppError::Agent(format!("failed to spawn claude: {}", e)))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(AppError::Agent(format!(
            "claude exited with error: {}",
            stderr
        )));
    }

    let raw = String::from_utf8(output.stdout)
        .map_err(|e| AppError::Agent(format!("non-UTF8 output: {}", e)))?;
    // stderr typically contains warnings (e.g. connector notices) — log them
    let stderr = String::from_utf8_lossy(&output.stderr);
    if !stderr.is_empty() {
        eprintln!("[claude stderr] {}", stderr);
    }

    let response =
        parse_response(&raw).ok_or_else(|| AppError::Agent("no response from claude".into()))?;

    println!("response:");
    println!("  text:      {}", response.text);
    println!(
        "  thinking:  {:?}",
        response.thinking.as_deref().unwrap_or("—")
    );
    println!("  result:    {}", response.result);
    println!("  is_error:  {}", response.is_error);
    println!("  duration:  {}ms", response.duration_ms);
    if let Some(ref u) = response.usage {
        println!("  tokens:    in={} out={}", u.input_tokens, u.output_tokens);
        println!("  cost:      ${:.6}", u.total_cost_usd);
    }

    Ok(response)
}

fn debug_command(cmd: &Command) {
    let program = cmd.get_program().to_string_lossy();
    let args: Vec<String> = cmd
        .get_args()
        .map(|arg| format!("{:?}", arg.to_string_lossy()))
        .collect();

    println!("executing {} {}", program, args.join(" "));
}

// ── Tests ──────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use crate::agents;

    use super::*;

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
