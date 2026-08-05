use crate::config::AgentConfig;
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

/// Shared per-phase token budget for a streaming unattended turn.
///
/// Claude reports per-model-call usage, so that adapter adds deltas. Codex
/// reports a cumulative total, so that adapter raises the observed floor.
/// Keeping the counter outside either session also makes retries consume the
/// same phase budget instead of resetting the cap for every attempt.
#[derive(Clone, Debug)]
pub(crate) struct TokenBudget {
    cap: u64,
    used: Arc<AtomicU64>,
}

impl TokenBudget {
    pub(crate) fn new(cap: u64) -> Self {
        Self {
            cap,
            used: Arc::new(AtomicU64::new(0)),
        }
    }

    pub(crate) fn used(&self) -> u64 {
        self.used.load(Ordering::SeqCst)
    }

    /// Record usage that is incremental to everything observed so far.
    pub(crate) fn add(&self, tokens: u64) -> bool {
        let previous = self
            .used
            .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |used| {
                Some(used.saturating_add(tokens))
            })
            .unwrap_or_else(|used| used);
        previous.saturating_add(tokens) > self.cap
    }

    /// Record a provider-reported cumulative total without double-counting it.
    pub(crate) fn observe_total(&self, tokens: u64) -> bool {
        self.used.fetch_max(tokens, Ordering::SeqCst);
        self.used() > self.cap
    }

    pub(crate) fn exceeded_message(&self) -> String {
        format!(
            "phase token budget exceeded: {} > {}",
            self.used(),
            self.cap
        )
    }
}

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

/// A single assistant content block, recorded in **arrival order**.
///
/// This is the persistable/transcript mirror of the internal `ContentBlock`:
/// `ToolUse` carries its input as a JSON string (like `ToolCallRecord`) so the
/// shape round-trips cleanly through `active.jsonl`. Keeping the full ordered
/// sequence — rather than flattening it into separate `text` / `thinking` /
/// `tool_calls` fields — lets the UI render a turn exactly as it unfolded
/// (e.g. thinking → text → tool_use → thinking → text), instead of a fixed
/// grouping that loses the interleaving.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ContentBlockRecord {
    /// One `ContentBlock::Text` from an assistant message.
    Text { text: String },
    /// One `ContentBlock::Thinking` from an assistant message.
    Thinking { thinking: String },
    /// One `ContentBlock::ToolUse` from an assistant message.
    ToolUse {
        /// Tool name, e.g. "Read", "Edit", "Bash".
        name: String,
        /// The raw tool input object, serialized to a JSON string.
        input: String,
    },
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
    /// Tool calls the assistant made during the turn (name + input), in order.
    /// Captured so the transcript can record how the model reached its answer,
    /// not just the final summary text.
    pub tool_calls: Vec<crate::conversation::ToolCallRecord>,
    /// The assistant content blocks for the turn, in **arrival order**. This is
    /// the canonical, order-preserving view used for rendering; `text` /
    /// `thinking` / `tool_calls` above are kept as flattened conveniences and as
    /// a fallback for callers that don't yet consume `blocks`.
    pub blocks: Vec<ContentBlockRecord>,
    /// Task lifecycle events (`tool_use_result.task`) observed during the turn,
    /// in arrival order. Persisted on the transcript so task creates / updates
    /// are recorded alongside the tool calls that produced them. Foundation for
    /// a future live Tasks panel — kept as a flat list per turn for now.
    pub tasks: Vec<crate::conversation::TaskRecord>,
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
    /// A task lifecycle event from a `tool_use_result.task` line — the agent
    /// created or updated a Task/TodoWrite item. Surfaced live so the streaming
    /// bubble can show "✓ Task #10 created: …" alongside tool activity, and
    /// persisted on the turn for replay. Foundation for a future Tasks panel.
    TaskUpdate {
        /// The structured task payload — id / subject / status. Mirrors
        /// `conversation::TaskRecord` so the persisted and live shapes match.
        task: crate::conversation::TaskRecord,
    },
    /// A permission decision made by the Selasar policy layer in response to a
    /// Claude `control_request`. Ephemeral — not part of the persisted
    /// transcript.
    ///
    /// **Three states for `decision`:**
    /// - `"pending"` — a mutating/executing tool (Bash/Edit/Write/…) needs
    ///   manual approval. Emitted BEFORE the `control_response` is written; the
    ///   read loop then parks until the user resolves it via
    ///   `agent_answer_permission`. The UI renders an Allow/Deny card and a
    ///   pending marker in the streaming bubble.
    /// - `"allow"` / `"deny"` — the final verdict. For manual-approval tools
    ///   this is emitted a second time (after the user's choice) so the UI's
    ///   pending marker becomes ✓/✗. For auto-decided tools (read-only tools
    ///   under allow-by-default, or destructive-floor denies) it's the only
    ///   emission and serves as post-hoc narration of the synchronous policy.
    PermissionRequest {
        request_id: String,
        tool_name: String,
        /// The raw tool input object, serialized to a JSON string.
        input: String,
        /// `"pending"` (awaiting user), `"allow"` / `"deny"` (resolved), or
        /// `"auto-allow"` (the autonomous-mode policy approved a floor-clearing
        /// mutating tool without parking). The UI uses `"auto-allow"` to tag
        /// unattended approvals in the transcript for the morning review.
        decision: String,
        /// Why Selasar allowed or denied. Empty for pending and plain allows.
        reason: String,
    },
    /// An `AskUserQuestion` tool call that needs the *human's* answer before
    /// the turn can proceed. Unlike `PermissionRequest` (auto-decided by the
    /// policy), this event is emitted **before** the `control_response` is
    /// written — the read loop parks on a oneshot until the frontend resolves
    /// it via the `agent_answer_question` command. Carries the structured
    /// questions/options so the UI can render option buttons directly.
    AskUserQuestion {
        /// Correlates to the originating `control_request`. The frontend echoes
        /// this back in `agent_answer_question`.
        request_id: String,
        /// Always `"AskUserQuestion"` — kept for symmetry with the wire shape.
        tool_name: String,
        /// The questions to surface, in order.
        questions: Vec<AskUserQuestionSpec>,
    },
    /// An `ExitPlanMode` tool call — Claude finished planning (while running
    /// under the CLI's `plan` permission mode, entered via
    /// `ClaudeSession::send_message_streaming`'s `plan_mode` flag) and wants to
    /// leave plan mode to start executing. Mirrors `PermissionRequest`'s
    /// pending/resolved shape rather than `AskUserQuestion`'s: there's no
    /// structured answer to feed back, just an allow/deny verdict, identical to
    /// a manual tool approval.
    ///
    /// **Four states for `decision`**, matching `PermissionRequest`:
    /// - `"pending"` — parked, awaiting the human's Approve/Reject via
    ///   `agent_answer_plan`. Emitted BEFORE the `control_response` is written.
    /// - `"allow"` — the human approved; the CLI's own `ExitPlanMode` handler
    ///   reverts the process out of plan mode.
    /// - `"deny"` — the human rejected (with optional feedback in `reason`);
    ///   the model stays in plan mode and is expected to revise and call
    ///   `ExitPlanMode` again.
    /// - `"auto-allow"` — an autonomous-mode project skipped the human review
    ///   entirely (same posture as `PermissionRequest`'s autonomous auto-allow).
    PlanApproval {
        request_id: String,
        /// The plan text Claude wrote, as passed to `ExitPlanMode`'s `plan`
        /// input field (markdown). Empty if the call was malformed.
        plan: String,
        /// `"pending"`, `"allow"`, `"deny"`, or `"auto-allow"`.
        decision: String,
        /// The human's feedback on a deny, surfaced to the model so it can
        /// revise the plan. Empty otherwise.
        reason: String,
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
    /// A transient provider error (e.g. `529 overloaded`) was hit and the turn
    /// is being retried after a backoff. Emitted *between* attempts on the
    /// streaming path, after the failed attempt's `Result` event but before the
    /// retry sends. Lets the UI show "Retrying 2/4 in 4s…" instead of a silent
    /// double-`Result`. The frontend treats the *final* `Result` (success or
    /// terminal failure) as authoritative.
    Retrying {
        /// 1-based index of the attempt that's about to run (e.g. `2` for the
        /// first retry after the initial attempt).
        attempt: u32,
        /// The configured maximum number of attempts.
        max_attempts: u32,
        /// The backoff duration in milliseconds that was slept before this event
        /// was emitted.
        backoff_ms: u64,
        /// The error message from the failed attempt that triggered the retry.
        error: String,
    },
}

/// One selectable option in an `AskUserQuestion` question.
///
/// Mirrors the wire shape of `AskUserQuestion`'s `options` array: a label the
/// user picks from, plus a longer description shown beneath it.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AskUserQuestionOption {
    /// Short pickable text, e.g. `"suprie/ngopi-yuk"`.
    pub label: String,
    /// Longer explanation of what the option entails.
    pub description: String,
}

/// One question surfaced to the user via the `AskUserQuestion` tool.
///
/// Wire shape (under `control_request.request.input.questions[]`): `question`,
/// `header`, `options[]`, `multiSelect`. We deserialize the questions array
/// into these so the UI can render structured option buttons rather than a
/// flattened string.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AskUserQuestionSpec {
    /// The full question text. Also the key used in the `answers` map sent back
    /// to Claude (`answers: { "<question>": <label|labels> }`).
    pub question: String,
    /// Short tag rendered as a chip above the question (e.g. `"GitHub Repo"`).
    pub header: String,
    /// The selectable options.
    pub options: Vec<AskUserQuestionOption>,
    /// Whether the user may select multiple options (checkbox vs radio UX).
    #[serde(rename = "multiSelect")]
    pub multi_select: bool,
}

/// Parse the `questions` array out of an `AskUserQuestion` tool input.
///
/// Returns an empty vec on any shape mismatch (and the caller falls back to a
/// deny), so a malformed request can't panic the read loop. Used by
/// `ClaudeSession::answer_control_request` to build the `AskUserQuestion` event.
pub(crate) fn parse_ask_user_questions(input: &serde_json::Value) -> Vec<AskUserQuestionSpec> {
    let Some(arr) = input.get("questions").and_then(|v| v.as_array()) else {
        return Vec::new();
    };
    arr.iter()
        .filter_map(|q| serde_json::from_value::<AskUserQuestionSpec>(q.clone()).ok())
        .collect()
}

/// Extract the plan text out of an `ExitPlanMode` tool input (`{"plan": "…"}`).
///
/// Returns `None` when the field is absent, non-string, or blank — the caller
/// (`ClaudeSession::answer_plan_approval`) falls back to an empty plan rather
/// than failing the whole request, since a malformed plan is still worth
/// surfacing to the user (better an empty card than a silent deny with no
/// explanation).
pub(crate) fn parse_exit_plan(input: &serde_json::Value) -> Option<String> {
    let plan = input.get("plan")?.as_str()?.trim();
    if plan.is_empty() {
        return None;
    }
    Some(plan.to_string())
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

    /// A `type: "user"` event carrying a `tool_use_result` for the
    /// Task/TodoWrite tool. Despite the `user` role, this is NOT a human turn —
    /// it's the tool's structured result routed back through the user role by
    /// Claude's stream protocol. We model it distinctly so the read loop never
    /// confuses it with real user input, and so its task payload can be
    /// surfaced + persisted.
    ///
    /// Only the task-bearing shape is modelled today; other `tool_use_result`
    /// payloads (file reads, command output) are still skipped as before, since
    /// their content already reaches us via the assistant `tool_use` block.
    #[serde(rename = "user")]
    ToolResult {
        /// The tool result body. `task` is populated when the originating tool
        /// was Task/TodoWrite; serde ignores the rest of the wire shape.
        #[serde(default)]
        tool_use_result: Option<ToolUseResult>,
        /// The `message.content[]` of the user event — each `tool_result`
        /// entry's plain text. We keep it because the create/update verb
        /// ("created" vs "updated") lives here rather than in the structured
        /// `task` object, mined by `task_status_from`.
        #[serde(default)]
        message: Option<UserMessage>,
    },

    /// A permission/approval request emitted via `--permission-prompt-tool stdio`.
    ///
    /// Claude sends this whenever a tool call doesn't match an allow rule, and
    /// then **blocks** until we write back a `control_response` with the same
    /// `request_id`. Before this variant existed, the read loop silently
    /// skipped these lines (they didn't match any known `type`) and Claude
    /// hung forever waiting for a response — see `claude_session.rs`.
    #[serde(rename = "control_request")]
    ControlRequest {
        /// Correlates the request to our response. Claude matches its next
        /// action off this id.
        request_id: String,
        /// The request body — we only model the fields we act on; serde
        /// ignores the rest (permission_suggestions, display_name, …).
        request: ControlRequestBody,
    },

    /// A `control_response` line — claude's reply to a `control_request`
    /// **we** sent. Only `set_permission_mode` currently reads this back
    /// (`ClaudeSession::write_set_permission_mode`); the `interrupt` subtype
    /// we also send has no reply and is fire-and-forget. Modeled so a
    /// CLI-side rejection (the installed CLI has explicit reject paths for
    /// `set_permission_mode`, e.g. "onSetPermissionMode callback not
    /// registered") surfaces as an error instead of being silently assumed
    /// to have succeeded once the request_id write itself didn't error.
    #[serde(rename = "control_response")]
    ControlResponse { response: ControlResponseBody },
}

/// The body of a `control_response` line — the inner `response` object.
///
/// Wire shape: `{"subtype":"success","request_id":"…"}` on success, or
/// `{"subtype":"error","request_id":"…","error":"…"}` on rejection. Lenient
/// on missing fields (defaults to empty/`None`) so a shape we don't fully
/// recognize still deserializes rather than being silently dropped as an
/// unparseable line.
#[derive(Debug, Deserialize)]
pub(crate) struct ControlResponseBody {
    #[serde(default)]
    pub(crate) subtype: String,
    #[serde(default)]
    pub(crate) request_id: String,
    #[serde(default)]
    pub(crate) error: Option<String>,
}

/// The body of a `control_request`. Only the `permission`/`can_use_tool`
/// subtype is modelled; `subtype` is kept as a free-form string so a future
/// subtype (e.g. a user-input prompt) deserializes instead of failing.
#[derive(Debug, Deserialize)]
pub(crate) struct ControlRequestBody {
    /// e.g. `"can_use_tool"`. We don't match on it yet — recorded for logs
    /// and assertions in tests, not read by production code.
    #[serde(default)]
    #[allow(dead_code)]
    pub(crate) subtype: String,
    /// Tool name, e.g. "Bash", "Edit". Falls back to empty if absent.
    #[serde(default)]
    pub(crate) tool_name: String,
    /// The raw tool input object (e.g. `{"command":"git add ."}`).
    #[serde(default = "default_tool_input")]
    pub(crate) input: serde_json::Value,
    /// The originating tool_use id, when present. Useful for log correlation.
    #[serde(default)]
    #[allow(dead_code)]
    pub(crate) tool_use_id: Option<String>,
}

/// The response we write back to Claude's stdin in reply to a `control_request`.
///
/// Wire shape (per the Claude control protocol):
/// ```json
/// { "type":"control_response", "response": {
///     "subtype":"success", "request_id":"<id>",
///     "response": { "behavior":"allow" }            // or
///     "response": { "behavior":"deny", "message":"…" }   // or
///     "response": { "behavior":"allow", "updatedInput": { … } }
/// }}
/// ```
/// Matched by `request_id`, not tool_use_id. Built from a `Decision` so the
/// `permission` module stays the single source of policy truth. The
/// `updated_input` arm is used by `AskUserQuestion`: the tool is allowed to
/// run, but with its input rewritten to carry the user's answers (per the
/// Claude Code "Handle approvals and user input" protocol).
pub(crate) struct ControlResponsePayload {
    request_id: String,
    behavior: &'static str,
    /// Present only on deny — Claude surfaces this to the model as the reason.
    message: Option<String>,
    /// Present only on the `AskUserQuestion` allow path — the rewritten tool
    /// input, carrying the user's answers. When set, this is serialized as
    /// `response.response.updatedInput` alongside `behavior: "allow"`.
    updated_input: Option<serde_json::Value>,
}

impl ControlResponsePayload {
    pub(crate) fn allow(request_id: impl Into<String>) -> Self {
        Self {
            request_id: request_id.into(),
            behavior: "allow",
            message: None,
            updated_input: None,
        }
    }

    pub(crate) fn deny(request_id: impl Into<String>, reason: impl Into<String>) -> Self {
        Self {
            request_id: request_id.into(),
            behavior: "deny",
            message: Some(reason.into()),
            updated_input: None,
        }
    }

    /// Allow the tool call but rewrite its input before it runs. Used to feed
    /// the user's answers back into `AskUserQuestion` — Claude executes the
    /// tool with `updated_input` (which contains `answers`), reads the result,
    /// and continues the turn. The envelope is identical to `allow()`; only
    /// the inner `response` object gains an `updatedInput` key.
    pub(crate) fn allow_with_updated_input(
        request_id: impl Into<String>,
        updated_input: serde_json::Value,
    ) -> Self {
        Self {
            request_id: request_id.into(),
            behavior: "allow",
            message: None,
            updated_input: Some(updated_input),
        }
    }

    /// Serialize to the single NDJSON line to write to stdin (trailing `\n`
    /// added by the caller).
    pub(crate) fn to_json(&self) -> serde_json::Value {
        // Allow-with-updatedInput: behavior + updatedInput, no message.
        if self.behavior == "allow" {
            if let Some(updated) = &self.updated_input {
                return serde_json::json!({
                    "type": "control_response",
                    "response": {
                        "subtype": "success",
                        "request_id": self.request_id,
                        "response": {
                            "behavior": "allow",
                            "updatedInput": updated,
                        },
                    }
                });
            }
        }
        let inner = match &self.message {
            Some(msg) => serde_json::json!({
                "behavior": self.behavior,
                "message": msg,
            }),
            None => serde_json::json!({ "behavior": self.behavior }),
        };
        serde_json::json!({
            "type": "control_response",
            "response": {
                "subtype": "success",
                "request_id": self.request_id,
                "response": inner,
            }
        })
    }
}

#[derive(Debug, Deserialize)]
pub(crate) struct AssistantMessage {
    pub(crate) content: Vec<ContentBlock>,
    /// Claude's stream-json assistant messages carry usage for that model
    /// invocation. Unlike the terminal result total, this arrives early enough
    /// to stop an agentic turn between tool calls.
    #[serde(default)]
    pub(crate) usage: Option<RawUsage>,
}

/// The `tool_use_result` body of a `type: "user"` tool-result event.
///
/// Only the `task` field is modelled — it's the structured payload the
/// Task/TodoWrite tool returns. Other tools' result bodies are ignored (their
/// content already arrives via the assistant `tool_use` block), so this is a
/// narrow lens rather than a general tool-result decoder.
#[derive(Debug, Deserialize)]
pub(crate) struct ToolUseResult {
    #[serde(default)]
    pub(crate) task: Option<TaskWire>,
}

/// The structured task payload inside a `tool_use_result`.
///
/// Wire shape (under `tool_use_result.task`): `id` + `subject`. The
/// create/update verb isn't carried here — it's mined from the surrounding
/// text content by `task_status_from`.
#[derive(Debug, Deserialize)]
pub(crate) struct TaskWire {
    #[serde(default)]
    pub(crate) id: String,
    #[serde(default)]
    pub(crate) subject: String,
}

/// The `message.content[]` entry of a `type: "user"` tool-result event.
///
/// Each entry is a `tool_result` carrying plain-text `content`. We extract the
/// text from all entries to mine the create/update verb (the structured `task`
/// object doesn't carry it). Other shapes are ignored via the catch-all.
#[derive(Debug, Deserialize)]
#[serde(tag = "type")]
pub(crate) enum UserContentItem {
    #[serde(rename = "tool_result")]
    ToolResult {
        #[serde(default)]
        content: serde_json::Value,
    },
    /// Any other shape — ignored. Keeping a catch-all (rather than failing)
    /// means a new content-item type can't break parsing.
    #[serde(other)]
    Other,
}

/// The `message` envelope of a `type: "user"` event.
///
/// Mirrors `AssistantMessage`'s shape but for the user role: a `content` array
/// whose entries are `tool_result` items (or other shapes we ignore). Used only
/// to mine the create/update verb text from task tool results.
#[derive(Debug, Deserialize)]
pub(crate) struct UserMessage {
    #[serde(default)]
    pub(crate) content: Vec<UserContentItem>,
}

/// Read the plain-text content out of a `tool_result` content item, regardless
/// of whether it's a string or an array of `{type:"text", text:"…"}` blocks
/// (Claude uses both shapes). Returns `None` for non-text payloads.
fn user_content_item_text(content: &serde_json::Value) -> Option<String> {
    if let Some(s) = content.as_str() {
        return Some(s.to_string());
    }
    if let Some(arr) = content.as_array() {
        let mut texts = Vec::new();
        for item in arr {
            if item.get("type").and_then(|v| v.as_str()) == Some("text") {
                if let Some(t) = item.get("text").and_then(|v| v.as_str()) {
                    texts.push(t.to_string());
                }
            }
        }
        if !texts.is_empty() {
            return Some(texts.join("\n"));
        }
    }
    None
}

/// Best-effort mine the task verb ("created"/"updated"/…) from a tool-result
/// text like `"Task #10 created successfully: …"`. Falls back to `"updated"`
/// when we can't classify — creating a task is the rarer, more notable event,
/// so we only claim "created" when the text explicitly says so; everything else
/// is conservatively an "update". Never returns empty so the UI always has a
/// label.
fn task_status_from(text: &str) -> &'static str {
    let lower = text.to_ascii_lowercase();
    if lower.contains("created") {
        "created"
    } else if lower.contains("updated") || lower.contains("modified") {
        "updated"
    } else if lower.contains("completed") || lower.contains("done") {
        "completed"
    } else if lower.contains("deleted") || lower.contains("removed") {
        "deleted"
    } else {
        "updated"
    }
}

/// Extract a `TaskRecord` from a parsed `StreamEvent::ToolResult`, if it
/// carries a task payload.
///
/// Single source of truth for "tool-result line → task record" so the live
/// streaming path (`send_message_streaming`) and the accumulator
/// (`ingest_event`) can't drift apart — both call this. Returns `None` for
/// tool results without a task (the common case: file reads, command output).
pub(crate) fn extract_task_from_tool_result(
    event: &StreamEvent,
) -> Option<crate::conversation::TaskRecord> {
    let StreamEvent::ToolResult {
        tool_use_result,
        message,
    } = event
    else {
        return None;
    };
    let TaskWire { id, subject } = tool_use_result.as_ref()?.task.as_ref()?;
    if id.is_empty() && subject.is_empty() {
        return None;
    }
    let status = message
        .as_ref()
        .and_then(|m| {
            m.content.iter().find_map(|item| match item {
                UserContentItem::ToolResult { content } => user_content_item_text(content),
                _ => None,
            })
        })
        .map(|t| task_status_from(&t).to_string())
        .unwrap_or_else(|| String::from("updated"));
    Some(crate::conversation::TaskRecord {
        id: id.clone(),
        subject: subject.clone(),
        status,
    })
}

/// Extract a `TaskRecord` from an assistant `tool_use` block, if it is a
/// `TaskUpdate` call.
///
/// This is the *second* task signal — the first is `extract_task_from_tool_result`
/// above, which fires on a `TaskCreate` **result** (`tool_use_result.task`). But
/// Claude's `TaskUpdate` tool carries its state change in the **call input**
/// (`{taskId, status}`), not in the result payload: when the agent marks a task
/// in_progress / completed / deleted, the result echoes nothing we can parse as a
/// task, so without this helper every update was dropped (the TaskPanel froze at
/// "created" forever).
///
/// Reads `taskId` + `status` + optional `subject` straight off the structured
/// input. `subject` is usually absent on an update (only the status changes), so
/// the frontend merges it onto the prior record rather than blanking it — see
/// `streamingState.applyTask`. Returns `None` for any other tool name
/// (including `TaskCreate`, whose input has no id).
pub(crate) fn extract_task_from_tool_use(
    name: &str,
    input: &serde_json::Value,
) -> Option<crate::conversation::TaskRecord> {
    if name != "TaskUpdate" {
        return None;
    }
    let id = input.get("taskId").and_then(|v| v.as_str())?.to_string();
    if id.is_empty() {
        return None;
    }
    let subject = input
        .get("subject")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    // `status` is required-by-convention on TaskUpdate but the schema only
    // mandates `taskId`; fall back to "updated" so the panel always has a label
    // even for a malformed/best-effort call.
    let status = input
        .get("status")
        .and_then(|v| v.as_str())
        .unwrap_or("updated")
        .to_string();
    Some(crate::conversation::TaskRecord {
        id,
        subject,
        status,
    })
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
    pub(crate) input_tokens: u64,
    pub(crate) output_tokens: u64,
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
    /// Tool calls collected from assistant `tool_use` blocks, in arrival order.
    /// Populated alongside the live ClaudeEvent::ToolUse emissions so the
    /// persisted transcript can record how the model reached its answer.
    tool_calls: Vec<crate::conversation::ToolCallRecord>,
    /// The assistant content blocks for the turn, in **arrival order**. Each
    /// `ContentBlock` from each assistant message is pushed here as it's
    /// ingested, so the flattened `text`/`thinking`/`tool_calls` fields above
    /// can be reconstructed *without* losing how the blocks interleaved.
    blocks: Vec<ContentBlockRecord>,
    /// Task lifecycle events (`tool_use_result.task`) collected from `type:
    /// "user"` tool-result lines, in arrival order. Populated alongside the
    /// live ClaudeEvent::TaskUpdate emissions so the persisted transcript
    /// records task creates / updates.
    tasks: Vec<crate::conversation::TaskRecord>,
    /// Running byte total of the accumulated content strings (text, thinking,
    /// tool-call input, task payloads). Bounded by [`crate::limits::ACCUMULATOR_MAX_BYTES`]
    /// (PRD FR4) so a runaway agent turn can't grow the persisted response
    /// without limit. `blocks` mirror the same content and are bounded
    /// separately by [`crate::limits::ACCUMULATOR_MAX_BLOCKS`].
    bytes: usize,
    /// Set once [`ACCUMULATOR_MAX_BLOCKS`] or [`ACCUMULATOR_MAX_BYTES`] is
    /// breached. Further content is dropped; [`finish`] / [`clone_partial`]
    /// stamp a truncation marker so the (persisted + streamed) result shows the
    /// turn was bounded. The terminal `Result` fields are still recorded.
    over_limit: bool,
}

/// Truncation marker appended to the `result` text when the accumulator hit a
/// block/byte budget (PRD FR4). Visible in both the transcript and the streamed
/// `ClaudeEvent::Result` so the user can tell the response was bounded.
const ACCUMULATOR_TRUNCATION_MARKER: &str =
    "\n\n[Selasar: response truncated — exceeded block or byte accumulation limit]";

impl ResponseAccumulator {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    /// Whether the accumulator has breached a block/byte budget (PRD FR4).
    /// Once true, [`ingest_event`] drops further content pushes (the terminal
    /// `Result` fields are still recorded) and sets [`over_limit`] so the
    /// finished response carries a truncation marker.
    fn at_limit(&self) -> bool {
        self.over_limit
            || self.blocks.len() >= crate::limits::ACCUMULATOR_MAX_BLOCKS
            || self.bytes >= crate::limits::ACCUMULATOR_MAX_BYTES
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
                    // Budget guard (PRD FR4): once a block/byte ceiling is
                    // breached, stop ingesting further content for this turn —
                    // drop the remaining blocks, record the breach, and keep
                    // the session_id (still set below). The terminal Result
                    // event is unaffected, so the turn finishes naturally with
                    // a truncated response rather than aborting mid-stream.
                    if self.at_limit() {
                        self.over_limit = true;
                        break;
                    }
                    match block {
                        ContentBlock::Text { text: t } => {
                            self.bytes += t.len();
                            self.text.push_str(&t);
                            self.blocks.push(ContentBlockRecord::Text { text: t });
                        }
                        ContentBlock::Thinking { thinking: th } => {
                            self.bytes += th.len();
                            self.thinking.get_or_insert_default().push_str(&th);
                            self.blocks
                                .push(ContentBlockRecord::Thinking { thinking: th });
                        }
                        // Capture tool calls for the aggregated response so the
                        // transcript records them. Live streaming still emits
                        // ClaudeEvent::ToolUse separately in send_message_streaming.
                        //
                        // `AskUserQuestion` is intentionally NOT persisted as a
                        // tool_use block: it's surfaced to the user via the
                        // pinned question card (driven by the
                        // `ask_user_question` channel event), and a persisted
                        // `› AskUserQuestion · {json}` row would just be noise
                        // on reload. Skip both `tool_calls` and `blocks`.
                        ContentBlock::ToolUse { name, input } => {
                            if name == "AskUserQuestion" {
                                continue;
                            }
                            let input_str = input.to_string();
                            self.bytes += name.len() + input_str.len();
                            self.tool_calls.push(crate::conversation::ToolCallRecord {
                                name: name.clone(),
                                input: input_str.clone(),
                            });
                            self.blocks.push(ContentBlockRecord::ToolUse {
                                name: name.clone(),
                                input: input_str,
                            });
                            // TaskUpdate carries its state change in the *call*
                            // input, so capture it here (mirrors the live
                            // emission in send_message_streaming). TaskCreate's
                            // id+subject arrive via the tool *result* path below.
                            if let Some(task) = extract_task_from_tool_use(&name, &input) {
                                self.bytes +=
                                    task.id.len() + task.subject.len() + task.status.len();
                                self.tasks.push(task);
                            }
                        }
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
            // Control requests are answered inline in the read loop
            // (`send_message[_streaming]`) before reaching the accumulator — by
            // the time we get here, the response has already been written. They
            // never carry assistant content and are never terminal, so this arm
            // is a no-op that exists to keep the match exhaustive.
            StreamEvent::ControlRequest { .. } => false,
            // A `control_response` line — the reply to a `control_request` we
            // sent. `write_set_permission_mode` consumes these directly via
            // its own dedicated read, ahead of the accumulator ever seeing
            // stream lines for a turn; this arm exists only to keep the match
            // exhaustive (a response line seen here — e.g. a stray echo
            // outside that dedicated wait — is harmless to ignore).
            StreamEvent::ControlResponse { .. } => false,
            // A `type: "user"` tool-result line. We only act on the
            // task-bearing shape: extract the structured task via the shared
            // helper (same one the streaming path uses for the live event) and
            // push a `TaskRecord`. Other tool results (file reads, command
            // output) are silently ignored here — their content already reached
            // us via the assistant `tool_use` block.
            StreamEvent::ToolResult { .. } => {
                if let Some(task) = extract_task_from_tool_result(&event) {
                    self.tasks.push(task);
                }
                false
            }
        }
    }

    /// Produce the final response. Returns `None` if we never saw any
    /// assistant text or a result event — useful for the caller to detect
    /// "claude produced nothing".
    pub(crate) fn finish(mut self) -> Option<AgentResponse> {
        if self.result_text.is_empty() && self.text.is_empty() {
            return None;
        }
        // Stamp the truncation marker (PRD FR4) onto the visible result text so
        // the user/transcript can see the turn was bounded by a block/byte
        // ceiling. `result_text` is set by the terminal Result event; if it's
        // empty the marker alone still surfaces the condition.
        if self.over_limit {
            self.result_text.push_str(ACCUMULATOR_TRUNCATION_MARKER);
        }

        Some(AgentResponse {
            text: self.text,
            thinking: self.thinking,
            result: self.result_text,
            usage: self.usage,
            is_error: self.is_error,
            duration_ms: self.duration_ms,
            session_id: self.session_id,
            tool_calls: self.tool_calls,
            blocks: self.blocks,
            tasks: self.tasks,
        })
    }

    /// Snapshot the accumulator's state as a partial `AgentResponse`.
    ///
    /// Used by the streaming loop on graceful interrupt: the terminal `result`
    /// event never arrives (the turn was stopped), so we synthesize one from
    /// whatever was accumulated so far and emit it as a `ClaudeEvent::Result`
    /// so the frontend reconciles (clears the streaming bubble + reloads the
    /// transcript). `result_text` is empty until the real result event, so the
    /// caller stamps a "(interrupted)" marker onto the synthesized result.
    pub(crate) fn clone_partial(&self) -> AgentResponse {
        // Stamp the truncation marker (PRD FR4) so an interrupted turn that had
        // already breached a budget still surfaces it in the synthesized result.
        let mut result = self.result_text.clone();
        if self.over_limit {
            result.push_str(ACCUMULATOR_TRUNCATION_MARKER);
        }
        AgentResponse {
            text: self.text.clone(),
            thinking: self.thinking.clone(),
            result,
            usage: self.usage.clone(),
            is_error: self.is_error,
            duration_ms: self.duration_ms,
            session_id: self.session_id.clone(),
            tool_calls: self.tool_calls.clone(),
            blocks: self.blocks.clone(),
            tasks: self.tasks.clone(),
        }
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
///
/// Test-only helper: drains a canned NDJSON blob into one `AgentResponse`. The
/// production paths feed the `ResponseAccumulator` line-by-line as events
/// arrive, so this whole-blob entry point has no non-test caller.
#[cfg(test)]
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
///
/// If `model` is unset, a default is injected so the `claude` CLI doesn't
/// silently fall back to an unknown model — the default is logged at spawn
/// and can be overridden from Settings.
pub(crate) fn apply_agent_config<C: CommandEnv>(cmd: &mut C, agent_config: &AgentConfig) {
    if let Some(auth_token) = &agent_config.auth_token {
        cmd.env("ANTHROPIC_AUTH_TOKEN", auth_token);
    }

    if let Some(base_url) = &agent_config.base_url {
        cmd.env("ANTHROPIC_BASE_URL", base_url);
    }

    // Fall back to a default model so the CLI never uses an unknown one
    // silently. The user can override in Settings or via ANTHROPIC_MODEL
    // in their shell env (which the child inherits if we don't set it).
    let model = agent_config.model.as_deref().unwrap_or(DEFAULT_MODEL);
    cmd.env("ANTHROPIC_MODEL", model);

    if let Some(effort) = &agent_config.effort {
        cmd.env("CLAUDE_CODE_EFFORT_LEVEL", effort);
    }
}

/// Default model id used when `AgentConfig.model` is unset. Mirrors the
/// documented default in `.env.example`.
pub const DEFAULT_MODEL: &str = "claude-sonnet-4-5";

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
        let json =
            r#"{"type":"tool_use","id":"toolu_01","name":"Read","input":{"file_path":"/a/b.rs"}}"#;
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

    // ── control protocol (permission requests) ──────────────────────────

    /// Regression guard for the stall: a `control_request` line MUST parse into
    /// `StreamEvent::ControlRequest` (with its `request_id`), not fall through
    /// to `None`. Before this variant existed, the read loop skipped these lines
    /// silently and Claude hung waiting for a response that was never coming.
    #[test]
    fn test_parse_control_request_from_prd_sample() {
        // Verbatim shape from the PRD's "Sample → Event" block.
        let line = r#"{"type":"control_request","request_id":"4cce7fc0-bcde-4297-9689-ebd53b1ada0e","request":{"subtype":"can_use_tool","tool_name":"Bash","display_name":"Bash","input":{"command":"which golangci-lint 2>/dev/null && golangci-lint --version 2>/dev/null || echo \"golangci-lint not installed locally (will run in CI)\"","description":"Check golangci-lint availability"},"description":"Check golangci-lint availability","permission_suggestions":[{"type":"addRules","rules":[{"toolName":"Bash","ruleContent":"golangci-lint --version"}],"behavior":"allow","destination":"localSettings"}],"decision_reason_type":"subcommandResults","tool_use_id":"call_01_pOZIcY2erlyvGrQqTLMT0680"}}"#;

        let event = parse_stream_line(line).expect("control_request must parse");
        match event {
            StreamEvent::ControlRequest {
                request_id,
                request,
            } => {
                assert_eq!(request_id, "4cce7fc0-bcde-4297-9689-ebd53b1ada0e");
                assert_eq!(request.subtype, "can_use_tool");
                assert_eq!(request.tool_name, "Bash");
                let cmd = request.input["command"].as_str().expect("command field");
                assert!(cmd.contains("golangci-lint"));
                assert_eq!(
                    request.tool_use_id.as_deref(),
                    Some("call_01_pOZIcY2erlyvGrQqTLMT0680")
                );
            }
            other => panic!("expected ControlRequest, got {other:?}"),
        }
    }

    #[test]
    fn test_parse_control_request_lenient_on_missing_fields() {
        // A minimal request must still parse — serde ignores unknown fields and
        // defaults the optional ones so a malformed/abbreviated request can't
        // take down the read loop.
        let line = r#"{"type":"control_request","request_id":"req-1","request":{"tool_name":"Edit","input":{"file_path":"/a.rs"}}}"#;
        let event = parse_stream_line(line).expect("minimal control_request must parse");
        match event {
            StreamEvent::ControlRequest {
                request_id,
                request,
            } => {
                assert_eq!(request_id, "req-1");
                assert_eq!(request.subtype, "", "missing subtype → empty default");
                assert_eq!(request.tool_name, "Edit");
                assert!(request.tool_use_id.is_none());
            }
            other => panic!("expected ControlRequest, got {other:?}"),
        }
    }

    #[test]
    fn test_parse_control_request_input_defaults_to_empty_object() {
        // No `input` at all → empty object, so downstream `input[...]` lookups
        // are safe (mirrors the tool_use input default).
        let line =
            r#"{"type":"control_request","request_id":"req-2","request":{"tool_name":"Bash"}}"#;
        let event = parse_stream_line(line).expect("control_request must parse");
        match event {
            StreamEvent::ControlRequest { request, .. } => {
                assert!(request.input.is_object());
                assert!(request.input.as_object().unwrap().is_empty());
            }
            other => panic!("expected ControlRequest, got {other:?}"),
        }
    }

    // ── control protocol (control_response — claude's reply to OUR requests) ──

    /// Regression guard for the "write succeeded ⇒ assume the mode switch
    /// succeeded" gap: a `control_response` with `subtype: "success"` must
    /// parse into `StreamEvent::ControlResponse` with the matching
    /// `request_id`, so `write_set_permission_mode` can confirm it against
    /// the request it just sent.
    #[test]
    fn test_parse_control_response_success() {
        let line = r#"{"type":"control_response","response":{"subtype":"success","request_id":"set-mode-abc"}}"#;
        let event = parse_stream_line(line).expect("control_response must parse");
        match event {
            StreamEvent::ControlResponse { response } => {
                assert_eq!(response.subtype, "success");
                assert_eq!(response.request_id, "set-mode-abc");
                assert!(response.error.is_none());
            }
            other => panic!("expected ControlResponse, got {other:?}"),
        }
    }

    /// The installed CLI has explicit `set_permission_mode` rejection paths
    /// (e.g. "onSetPermissionMode callback not registered") that reply with
    /// `subtype: "error"` + a human-readable `error` string — this must
    /// parse so `write_set_permission_mode` can surface the real reason
    /// instead of assuming success.
    #[test]
    fn test_parse_control_response_error_carries_reason() {
        let line = r#"{"type":"control_response","response":{"subtype":"error","request_id":"set-mode-xyz","error":"onSetPermissionMode callback not registered"}}"#;
        let event = parse_stream_line(line).expect("control_response must parse");
        match event {
            StreamEvent::ControlResponse { response } => {
                assert_eq!(response.subtype, "error");
                assert_eq!(response.request_id, "set-mode-xyz");
                assert_eq!(
                    response.error.as_deref(),
                    Some("onSetPermissionMode callback not registered")
                );
            }
            other => panic!("expected ControlResponse, got {other:?}"),
        }
    }

    /// A minimal/malformed control_response (missing fields) must still
    /// parse — lenient defaults so an unrecognized shape can't panic the
    /// read loop, mirroring `test_parse_control_request_lenient_on_missing_fields`.
    #[test]
    fn test_parse_control_response_lenient_on_missing_fields() {
        let line = r#"{"type":"control_response","response":{}}"#;
        let event = parse_stream_line(line).expect("control_response must parse");
        match event {
            StreamEvent::ControlResponse { response } => {
                assert_eq!(response.subtype, "");
                assert_eq!(response.request_id, "");
                assert!(response.error.is_none());
            }
            other => panic!("expected ControlResponse, got {other:?}"),
        }
    }

    #[test]
    fn test_control_response_allow_shape_matches_protocol() {
        // The exact wire shape Claude expects (PRD "Sample → Response").
        let payload = ControlResponsePayload::allow("4cce7fc0-bcde-4297-9689-ebd53b1ada0e");
        let json = payload.to_json();
        assert_eq!(json["type"], "control_response");
        assert_eq!(json["response"]["subtype"], "success");
        assert_eq!(
            json["response"]["request_id"],
            "4cce7fc0-bcde-4297-9689-ebd53b1ada0e"
        );
        assert_eq!(json["response"]["response"]["behavior"], "allow");
        // Allow carries no message.
        assert!(
            json["response"]["response"]["message"].is_null(),
            "allow must not carry a message"
        );
    }

    #[test]
    fn test_control_response_deny_carries_reason() {
        let payload =
            ControlResponsePayload::deny("req-9", "destructive command blocked by policy floor");
        let json = payload.to_json();
        assert_eq!(json["response"]["request_id"], "req-9");
        assert_eq!(json["response"]["response"]["behavior"], "deny");
        assert_eq!(
            json["response"]["response"]["message"],
            "destructive command blocked by policy floor"
        );
    }

    #[test]
    fn test_control_response_allow_with_updated_input_shape() {
        // The AskUserQuestion response: same envelope as allow(), but the inner
        // `response` carries `behavior: "allow"` + `updatedInput` (the rewritten
        // tool input, here with the user's answers merged in). This is the exact
        // shape Claude expects per the "Handle approvals and user input" docs.
        let updated = serde_json::json!({
            "questions": [{"question": "Repo?", "header": "GitHub", "options": [], "multiSelect": false}],
            "answers": { "Repo?": "suprie/ngopi-yuk" }
        });
        let payload = ControlResponsePayload::allow_with_updated_input("req-q", updated.clone());
        let json = payload.to_json();

        // Envelope unchanged.
        assert_eq!(json["type"], "control_response");
        assert_eq!(json["response"]["subtype"], "success");
        assert_eq!(json["response"]["request_id"], "req-q");

        // Inner response carries both behavior and the rewritten input.
        assert_eq!(json["response"]["response"]["behavior"], "allow");
        assert_eq!(json["response"]["response"]["updatedInput"], updated);
        // Must NOT carry a `message` (that's the deny path).
        assert!(
            json["response"]["response"]["message"].is_null(),
            "allow-with-updatedInput must not carry a message"
        );
        // And the plain-allow keys must be absent.
        assert!(
            json["response"]["response"].get("updatedInput").is_some(),
            "updatedInput key must be present"
        );
    }

    #[test]
    fn test_plain_allow_does_not_carry_updated_input() {
        // Regression guard: the original allow() path must not accidentally
        // emit `updatedInput`. Only allow_with_updated_input does.
        let json = ControlResponsePayload::allow("req-a").to_json();
        assert!(
            json["response"]["response"].get("updatedInput").is_none(),
            "plain allow must not carry updatedInput"
        );
    }

    #[test]
    fn test_parse_ask_user_questions_extracts_structured_options() {
        // A realistic AskUserQuestion input: the questions array carries
        // question/header/options/multiSelect each. We extract them into
        // structured specs so the UI can render option buttons directly.
        let input = serde_json::json!({
            "questions": [
                {
                    "question": "Where should I push this repo?",
                    "header": "GitHub Repo",
                    "options": [
                        {"label": "suprie/ngopi-yuk", "description": "personal account"},
                        {"label": "Create new org repo", "description": "you provide org/repo"}
                    ],
                    "multiSelect": false
                }
            ]
        });
        let specs = parse_ask_user_questions(&input);
        assert_eq!(specs.len(), 1);
        let q = &specs[0];
        assert_eq!(q.question, "Where should I push this repo?");
        assert_eq!(q.header, "GitHub Repo");
        assert!(!q.multi_select);
        assert_eq!(q.options.len(), 2);
        assert_eq!(q.options[0].label, "suprie/ngopi-yuk");
        assert_eq!(q.options[1].description, "you provide org/repo");
    }

    #[test]
    fn test_parse_ask_user_questions_lenient_on_bad_shape() {
        // Missing questions array / wrong type / malformed entry: never panic,
        // just return an empty (or partial) vec. The caller falls back to deny.
        assert!(parse_ask_user_questions(&serde_json::json!({})).is_empty());
        assert!(parse_ask_user_questions(&serde_json::json!({"questions": "nope"})).is_empty());
        // A malformed entry is skipped; a well-formed one alongside it survives.
        let mixed = serde_json::json!({
            "questions": [
                {"question": "ok", "header": "h", "options": [], "multiSelect": true},
                {"wrong": "shape"}
            ]
        });
        let specs = parse_ask_user_questions(&mixed);
        assert_eq!(specs.len(), 1);
        assert_eq!(specs[0].question, "ok");
    }

    // ── ExitPlanMode parsing ─────────────────────────────────────────────

    #[test]
    fn test_parse_exit_plan_extracts_plan_text() {
        let input = serde_json::json!({ "plan": "1. Read the file\n2. Edit it" });
        assert_eq!(
            parse_exit_plan(&input).as_deref(),
            Some("1. Read the file\n2. Edit it")
        );
    }

    #[test]
    fn test_parse_exit_plan_trims_whitespace() {
        let input = serde_json::json!({ "plan": "  do the thing  \n" });
        assert_eq!(parse_exit_plan(&input).as_deref(), Some("do the thing"));
    }

    #[test]
    fn test_parse_exit_plan_none_on_missing_or_blank() {
        assert!(parse_exit_plan(&serde_json::json!({})).is_none());
        assert!(parse_exit_plan(&serde_json::json!({"plan": ""})).is_none());
        assert!(parse_exit_plan(&serde_json::json!({"plan": "   "})).is_none());
        assert!(parse_exit_plan(&serde_json::json!({"plan": 42})).is_none());
    }

    // ── tool-result task parsing ───────────────────────────────────────

    #[test]
    fn test_parse_tool_result_extracts_task_create() {
        // The headline case from the user's sample JSON: a `type: "user"`
        // tool-result carrying `tool_use_result.task`. Must parse into a
        // `StreamEvent::ToolResult` whose extracted `TaskRecord` carries the
        // id + subject + "created" status (mined from the text content).
        let line = r#"{"type":"user","message":{"role":"user","content":[{"tool_use_id":"call_e944","type":"tool_result","content":"Task #10 created successfully: Test truncate helper (list.go)"}]},"tool_use_result":{"task":{"id":"10","subject":"Test truncate helper (list.go)"}}}"#;
        let event = parse_stream_line(line).expect("tool-result line must parse");
        let task = extract_task_from_tool_result(&event).expect("task must extract");
        assert_eq!(task.id, "10");
        assert_eq!(task.subject, "Test truncate helper (list.go)");
        assert_eq!(task.status, "created");
    }

    #[test]
    fn test_parse_tool_result_mines_update_verb() {
        // An update carries the same task shape but the text says "updated" —
        // the status field must reflect that, not default or mis-classify.
        let line = r#"{"type":"user","message":{"role":"user","content":[{"type":"tool_result","content":"Task #3 updated"}]},"tool_use_result":{"task":{"id":"3","subject":"Wire up the thing"}}}"#;
        let event = parse_stream_line(line).expect("update line must parse");
        let task = extract_task_from_tool_result(&event).expect("task must extract");
        assert_eq!(task.id, "3");
        assert_eq!(task.status, "updated");
    }

    #[test]
    fn test_parse_tool_result_handles_array_content_shape() {
        // Claude sometimes emits tool_result `content` as an array of text
        // blocks rather than a plain string. The verb mining must handle both.
        let line = r#"{"type":"user","message":{"content":[{"type":"tool_result","content":[{"type":"text","text":"Task #1 completed"}]}]},"tool_use_result":{"task":{"id":"1","subject":"Done thing"}}}"#;
        let event = parse_stream_line(line).expect("array-content line must parse");
        let task = extract_task_from_tool_result(&event).expect("task must extract");
        assert_eq!(task.status, "completed");
    }

    #[test]
    fn test_parse_tool_result_without_task_returns_none() {
        // The common case: a tool result that is NOT a task (file read,
        // command output). It still parses as `StreamEvent::ToolResult` (so
        // the read loop doesn't skip it), but extracts no task — keeping task
        // logic from firing on unrelated tool results.
        let line = r#"{"type":"user","message":{"content":[{"type":"tool_result","content":"(Bash completed with no output)"}]},"tool_use_result":{}}"#;
        let event = parse_stream_line(line).expect("non-task result must still parse");
        assert!(extract_task_from_tool_result(&event).is_none());
    }

    #[test]
    fn test_accumulator_collects_tasks_in_arrival_order() {
        // Two task events in one turn must both land on the accumulated
        // `AgentResponse.tasks`, in arrival order — this is what gets persisted
        // on the `ConversationTurn`.
        let mut acc = ResponseAccumulator::new();
        let create = parse_stream_line(
            r#"{"type":"user","message":{"content":[{"type":"tool_result","content":"Task #1 created: A"}]},"tool_use_result":{"task":{"id":"1","subject":"A"}}}"#,
        ).unwrap();
        let update = parse_stream_line(
            r#"{"type":"user","message":{"content":[{"type":"tool_result","content":"Task #1 updated"}]},"tool_use_result":{"task":{"id":"1","subject":"A"}}}"#,
        ).unwrap();
        acc.ingest_event(create);
        acc.ingest_event(update);
        // Drive a Result so finish() returns Some.
        acc.ingest_event(StreamEvent::Result {
            result: "done".into(),
            is_error: false,
            duration_ms: 1,
            total_cost_usd: None,
            usage: None,
        });
        let resp = acc.finish().expect("finish must produce a response");
        assert_eq!(resp.tasks.len(), 2);
        assert_eq!(resp.tasks[0].status, "created");
        assert_eq!(resp.tasks[1].status, "updated");
    }

    #[test]
    fn test_extract_task_from_tool_use_status_transitions() {
        // `TaskUpdate` carries its state change in the call input, not the
        // result. The helper must parse `taskId` + `status` (+ optional subject)
        // off the structured input. This is the path that was previously dropped
        // entirely — the regression this test guards.
        let inp = serde_json::json!({"taskId": "1", "status": "in_progress"});
        let task = extract_task_from_tool_use("TaskUpdate", &inp)
            .expect("TaskUpdate input must extract a task");
        assert_eq!(task.id, "1");
        assert_eq!(task.status, "in_progress");
        // subject is absent on a status-only update — empty here, merged in the UI
        assert_eq!(task.subject, "");

        // Completed transition.
        let inp = serde_json::json!({"taskId": "2", "status": "completed"});
        let task = extract_task_from_tool_use("TaskUpdate", &inp).unwrap();
        assert_eq!(task.status, "completed");

        // A subject-bearing update (rename) passes through.
        let inp =
            serde_json::json!({"taskId": "3", "status": "in_progress", "subject": "New title"});
        let task = extract_task_from_tool_use("TaskUpdate", &inp).unwrap();
        assert_eq!(task.subject, "New title");

        // Non-TaskUpdate tools (incl. TaskCreate, whose input has no id) → None.
        assert!(extract_task_from_tool_use("TaskCreate", &serde_json::json!({})).is_none());
        assert!(
            extract_task_from_tool_use("Read", &serde_json::json!({"file_path": "/a"})).is_none()
        );
        // Missing taskId → None.
        assert!(extract_task_from_tool_use(
            "TaskUpdate",
            &serde_json::json!({"status": "completed"})
        )
        .is_none());
    }

    #[test]
    fn test_accumulator_collects_task_update_from_tool_use_input() {
        // The realistic wire shape: a `TaskCreate` lands via a tool RESULT
        // (carrying id+subject), then a `TaskUpdate` lands via a tool_use INPUT
        // (carrying the new status). Before the fix, the update was silently
        // dropped because its result carried no task payload — so both must now
        // land on `AgentResponse.tasks` in arrival order.
        let mut acc = ResponseAccumulator::new();
        // Create: tool_use_result.task (id + subject), status mined as "created".
        let create = parse_stream_line(
            r#"{"type":"user","message":{"content":[{"type":"tool_result","content":"Task #1 created: A"}]},"tool_use_result":{"task":{"id":"1","subject":"A"}}}"#,
        ).unwrap();
        // Update: an assistant tool_use block named TaskUpdate with the new status.
        let update = parse_stream_line(
            r#"{"type":"assistant","message":{"content":[{"type":"tool_use","id":"toolu_1","name":"TaskUpdate","input":{"taskId":"1","status":"completed"}}]},"session_id":"s"}"#,
        ).unwrap();
        acc.ingest_event(create);
        acc.ingest_event(update);
        acc.ingest_event(StreamEvent::Result {
            result: "done".into(),
            is_error: false,
            duration_ms: 1,
            total_cost_usd: None,
            usage: None,
        });
        let resp = acc.finish().expect("finish must produce a response");
        assert_eq!(resp.tasks.len(), 2);
        assert_eq!(resp.tasks[0].id, "1");
        assert_eq!(resp.tasks[0].status, "created");
        // The update landed — status from the call input, subject empty (merged in UI).
        assert_eq!(resp.tasks[1].id, "1");
        assert_eq!(resp.tasks[1].status, "completed");
        assert_eq!(resp.tasks[1].subject, "");
    }

    #[test]
    fn test_parse_response_blocks_preserve_arrival_order() {
        // The headline reason `blocks` exists: a turn that interleaves
        // thinking → text → tool_use → thinking → text must keep that exact
        // sequence. Flattening into separate fields would lose the interleaving
        // and the UI would fall back to a fixed thinking→tools→text grouping.
        // Two assistant messages on separate lines model the real stream, where
        // each tool round-trip comes back as its own assistant message.
        let ndjson = concat!(
            r#"{"type":"assistant","message":{"content":["#,
            r#"{"type":"thinking","thinking":"plan"},"#,
            r#"{"type":"text","text":"let me check"},"#,
            r#"{"type":"tool_use","name":"Read","input":{"file_path":"/a.rs"}}"#,
            r#"]},"session_id":"session_id"}"#,
            "\n",
            r#"{"type":"assistant","message":{"content":["#,
            r#"{"type":"thinking","thinking":"now edit"},"#,
            r#"{"type":"text","text":"done"}"#,
            r#"]},"session_id":"session_id"}"#,
            "\n",
            r#"{"type":"result","result":"done","is_error":false,"duration_ms":99}"#,
        );

        let response = parse_response(ndjson).expect("should parse");
        let blocks = &response.blocks;
        assert_eq!(
            blocks.len(),
            5,
            "all five blocks should be recorded in order"
        );

        assert!(
            matches!(&blocks[0], ContentBlockRecord::Thinking { thinking } if thinking == "plan")
        );
        assert!(matches!(&blocks[1], ContentBlockRecord::Text { text } if text == "let me check"));
        assert!(matches!(
            &blocks[2],
            ContentBlockRecord::ToolUse { name, input } if name == "Read" && input.contains("/a.rs")
        ));
        assert!(
            matches!(&blocks[3], ContentBlockRecord::Thinking { thinking } if thinking == "now edit")
        );
        assert!(matches!(&blocks[4], ContentBlockRecord::Text { text } if text == "done"));

        // The flattened fields are still populated (back-compat / fallback).
        assert_eq!(response.text, "let me checkdone");
        assert_eq!(response.thinking.as_deref(), Some("plannow edit"));
        assert_eq!(response.tool_calls.len(), 1);
    }

    // ── accumulator resource budgets (PRD FR4) ──────────────────────────

    #[test]
    fn accumulator_caps_blocks_and_marks_truncation() {
        // Push well over the block cap (one tiny text block per assistant
        // message), then a terminal Result. The accumulator must stop ingesting
        // at the cap and stamp the truncation marker onto the visible result.
        let mut acc = ResponseAccumulator::new();
        for _ in 0..(crate::limits::ACCUMULATOR_MAX_BLOCKS + 1000) {
            let _ = acc.ingest_event(StreamEvent::Assistant {
                message: AssistantMessage {
                    content: vec![ContentBlock::Text { text: "x".into() }],
                    usage: None,
                },
                session_id: "s".into(),
            });
        }
        let _ = acc.ingest_event(StreamEvent::Result {
            result: "done".into(),
            is_error: false,
            duration_ms: 1,
            total_cost_usd: None,
            usage: None,
        });
        let resp = acc.finish().expect("finish must produce a response");
        assert!(
            resp.blocks.len() <= crate::limits::ACCUMULATOR_MAX_BLOCKS,
            "blocks should be capped at ACCUMULATOR_MAX_BLOCKS, got {}",
            resp.blocks.len()
        );
        assert!(
            resp.result.contains("truncated"),
            "result should carry the truncation marker: {}",
            resp.result
        );
    }

    #[test]
    fn accumulator_clone_partial_stamps_truncation_when_over_limit() {
        // An interrupted turn that already breached the budget must still
        // surface the marker via clone_partial (the synthesized Result path).
        let mut acc = ResponseAccumulator::new();
        for _ in 0..(crate::limits::ACCUMULATOR_MAX_BLOCKS + 100) {
            let _ = acc.ingest_event(StreamEvent::Assistant {
                message: AssistantMessage {
                    content: vec![ContentBlock::Text { text: "y".into() }],
                    usage: None,
                },
                session_id: "s".into(),
            });
        }
        let partial = acc.clone_partial();
        assert!(partial.result.contains("truncated"));
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
            ..Default::default()
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
    fn test_apply_agent_config_empty_fields_set_default_model() {
        let config = AgentConfig {
            auth_token: None,
            base_url: None,
            model: None,
            effort: None,
            ..Default::default()
        };

        let mut cmd = Command::new("claude");

        // Record env vars that *already exist* after construction (inherited from
        // the test process). Then apply config and verify only the default model
        // was added — everything else stays unset.
        let before: Vec<String> = cmd
            .get_envs()
            .map(|(k, _)| k.to_string_lossy().into_owned())
            .collect();

        apply_agent_config(&mut cmd, &config);

        let envs: Vec<(String, String)> = cmd
            .get_envs()
            .filter_map(|(k, v)| {
                v.map(|val| {
                    (
                        k.to_string_lossy().into_owned(),
                        val.to_string_lossy().into_owned(),
                    )
                })
            })
            .collect();

        // The default model IS set (no silent CLI fallback).
        assert!(
            envs.contains(&("ANTHROPIC_MODEL".into(), DEFAULT_MODEL.into())),
            "expected ANTHROPIC_MODEL={DEFAULT_MODEL}, got: {envs:?}"
        );

        // The other three were NOT added (they're still None in the config).
        for var in [
            "ANTHROPIC_AUTH_TOKEN",
            "ANTHROPIC_BASE_URL",
            "CLAUDE_CODE_EFFORT_LEVEL",
        ] {
            let was_present_before = before.contains(&var.to_string());
            let is_present_after = envs.iter().any(|(k, _)| k == var);
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
            ..Default::default()
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

    #[test]
    fn token_budget_accumulates_claude_deltas_across_calls() {
        let budget = TokenBudget::new(100);
        assert!(!budget.add(40));
        assert!(!budget.add(60));
        assert!(budget.add(1));
        assert_eq!(budget.used(), 101);
    }

    #[test]
    fn token_budget_observes_codex_totals_without_double_counting() {
        let budget = TokenBudget::new(100);
        assert!(!budget.observe_total(80));
        assert!(!budget.observe_total(60));
        assert!(budget.observe_total(101));
        assert_eq!(budget.used(), 101);
    }

    #[test]
    fn assistant_stream_event_exposes_usage_before_terminal_result() {
        let event = parse_stream_line(
            r#"{"type":"assistant","session_id":"s","message":{"content":[],"usage":{"input_tokens":80,"output_tokens":21}}}"#,
        )
        .expect("assistant event should parse");
        let StreamEvent::Assistant { message, .. } = event else {
            panic!("expected assistant event");
        };
        let usage = message.usage.expect("assistant usage should be retained");
        assert_eq!(usage.input_tokens + usage.output_tokens, 101);
    }
}
