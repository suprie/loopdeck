//! Persistent Codex app-server adapter.
//!
//! The app-server protocol is newline-delimited JSON-RPC over stdio. One
//! process owns one Selasar project thread; turns reuse that thread, while a
//! persisted `codex:<thread-id>` resumes it after an app restart.

use crate::agents::{
    AgentResponse, AskUserQuestionOption, AskUserQuestionSpec, ClaudeEvent, ContentBlockRecord,
    TokenBudget, UsageInfo,
};
use crate::claude_session::{
    InterruptSlot, ParkSlots, PendingPermission, PendingQuestion, PermissionSlot, QuestionAnswers,
    QuestionSlot,
};
use crate::config::AgentConfig;
use crate::conversation::{Attachment, ToolCallRecord};
use crate::error::AppError;
use crate::harness::HarnessAdapter;
use crate::permission::{Decision, PermissionPolicy};
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::{Duration, Instant};
use tauri::ipc::Channel;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin, ChildStdout, Command};
use tokio::sync::oneshot;
use tokio::task::JoinHandle;

/// Bound app-server setup handshakes so a broken child cannot hold the first
/// turn forever. Once a turn has been accepted, protocol silence is valid:
/// Codex may spend longer than this inside a tool without emitting JSONL.
const HANDSHAKE_READ_TIMEOUT: Duration = Duration::from_secs(300);
/// A graceful Stop should complete quickly. If app-server itself is wedged,
/// waiting forever would leave the project locked in "Agent is working".
const INTERRUPT_GRACE_TIMEOUT: Duration = Duration::from_secs(15);
/// Tool arguments are diagnostic data and can be large. Keep logs useful
/// without allowing a single request to consume an unbounded amount of the
/// rolling application log.
const CODEX_TOOL_LOG_MAX_CHARS: usize = 8_192;

pub struct CodexSession {
    child: Option<Child>,
    stdin: Option<ChildStdin>,
    stdout: BufReader<ChildStdout>,
    stderr_drain: Option<JoinHandle<()>>,
    cwd: PathBuf,
    model: Option<String>,
    effort: Option<String>,
    resume_thread_id: Option<String>,
    thread_id: Option<String>,
    next_request_id: u64,
    initialized: bool,
    /// App-server-provided Plan preset, used only for interactive interviews.
    plan_collaboration_mode: Option<Value>,
    policy: PermissionPolicy,
    /// Rendered role charter, pending injection. Codex's app-server protocol
    /// has no system-prompt override, so the charter is prepended to the
    /// *first* task prompt sent on this session (role identity first, task
    /// second) and consumed — later turns already share the thread's context.
    charter_prompt: Option<String>,
}

impl CodexSession {
    pub fn spawn(
        cwd: &Path,
        config: &AgentConfig,
        resume_thread_id: Option<&str>,
        policy: PermissionPolicy,
    ) -> Result<Self, AppError> {
        let binary = crate::binary::codex()?;
        tracing::info!(
            target: "loopdeck::codex",
            binary = %binary.display(),
            cwd = %cwd.display(),
            resume = resume_thread_id.is_some(),
            "spawning Codex app-server harness"
        );

        let mut command = Command::new(binary);
        command
            .arg("app-server")
            .arg("--listen")
            .arg("stdio://")
            .current_dir(cwd)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);

        let mut child = match command.spawn() {
            Ok(child) => child,
            Err(error) => {
                return Err(AppError::Agent(format!(
                    "failed to start Codex app-server: {error}"
                )))
            }
        };
        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| AppError::Agent("Codex app-server stdin unavailable".into()))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| AppError::Agent("Codex app-server stdout unavailable".into()))?;
        let stderr = child
            .stderr
            .take()
            .ok_or_else(|| AppError::Agent("Codex app-server stderr unavailable".into()))?;
        let stderr_drain = tokio::spawn(async move {
            let mut lines = BufReader::new(stderr).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                tracing::debug!(target: "loopdeck::codex_stderr", "{line}");
            }
        });

        Ok(Self {
            child: Some(child),
            stdin: Some(stdin),
            stdout: BufReader::new(stdout),
            stderr_drain: Some(stderr_drain),
            cwd: cwd.to_path_buf(),
            model: config.model.clone().filter(|v| !v.is_empty()),
            effort: config.effort.clone().filter(|v| !v.is_empty()),
            resume_thread_id: resume_thread_id.map(ToOwned::to_owned),
            thread_id: None,
            next_request_id: 1,
            initialized: false,
            plan_collaboration_mode: None,
            policy,
            charter_prompt: config
                .charter
                .as_ref()
                .filter(|charter| !charter.is_empty())
                .map(crate::config::RoleCharter::render),
        })
    }

    /// `slots.plan` is unused here — Codex has no `ExitPlanMode` concept (its
    /// own approval model is always `readOnly` + `on-request`), so only the
    /// question/permission slots this session actually parks on are threaded
    /// down to `send_turn`. `ParkSlots` is accepted (rather than separate
    /// `question_slot`/`permission_slot` params) purely so `HarnessSession`
    /// can hand the same bundle to either backend.
    pub async fn send_message(
        &mut self,
        text: &str,
        attachments: &[Attachment],
        slots: &ParkSlots<'_>,
        interrupt_slot: &InterruptSlot,
    ) -> Result<AgentResponse, AppError> {
        self.send_turn(
            text,
            attachments,
            None,
            slots.question,
            slots.permission,
            interrupt_slot,
            None,
            false,
        )
        .await
    }

    /// Codex Plan collaboration mode is used for interactive interviews;
    /// ordinary agent turns remain in the default mode.
    pub async fn send_message_streaming(
        &mut self,
        text: &str,
        attachments: &[Attachment],
        channel: &Channel<ClaudeEvent>,
        slots: &ParkSlots<'_>,
        interrupt_slot: &InterruptSlot,
        plan_mode: bool,
        token_budget: Option<&TokenBudget>,
    ) -> Result<AgentResponse, AppError> {
        self.send_turn(
            text,
            attachments,
            Some(channel),
            slots.question,
            slots.permission,
            interrupt_slot,
            token_budget,
            plan_mode,
        )
        .await
    }

    async fn ensure_initialized(&mut self) -> Result<(), AppError> {
        if self.initialized {
            return Ok(());
        }

        let initialize_id = self.next_id();
        self.write_message(json!({
            "method": "initialize",
            "id": initialize_id,
            "params": initialize_params()
        }))
        .await?;
        self.write_message(json!({"method": "initialized", "params": {}}))
            .await?;
        self.wait_for_response(initialize_id).await?;

        let modes_id = self.next_id();
        self.write_message(json!({
            "method": "collaborationMode/list",
            "id": modes_id,
            "params": {}
        }))
        .await?;
        let modes = self.wait_for_response(modes_id).await?;

        // A LoopDeck harness profile may intentionally leave its model unset.
        // Ask Codex for its recommended default instead of requiring the
        // profile to duplicate the app-server configuration.
        let models_id = self.next_id();
        self.write_message(json!({
            "method": "model/list",
            "id": models_id,
            "params": { "limit": 20, "includeHidden": false }
        }))
        .await?;
        let models = self.wait_for_response(models_id).await?;
        self.plan_collaboration_mode = plan_collaboration_mode(
            &modes,
            &models,
            self.model.as_deref(),
            self.effort.as_deref(),
        );

        let thread_id = self.next_id();
        let mut params = if let Some(resume_id) = self.resume_thread_id.take() {
            json!({
                "threadId": resume_id,
                "cwd": self.cwd,
                "approvalPolicy": "on-request",
                "sandbox": "workspace-write",
            })
        } else {
            json!({
                "cwd": self.cwd,
                "approvalPolicy": "on-request",
                "sandbox": "workspace-write",
            })
        };
        if let Some(model) = &self.model {
            params["model"] = Value::String(model.clone());
        }
        let method = if params.get("threadId").is_some() {
            "thread/resume"
        } else {
            "thread/start"
        };
        self.write_message(json!({"method": method, "id": thread_id, "params": params}))
            .await?;
        let result = self.wait_for_response(thread_id).await?;
        let id = result
            .pointer("/thread/id")
            .and_then(Value::as_str)
            .ok_or_else(|| {
                AppError::Agent(format!(
                    "Codex {method} response did not include a thread id"
                ))
            })?;
        self.thread_id = Some(id.to_owned());
        self.initialized = true;
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    async fn send_turn(
        &mut self,
        text: &str,
        attachments: &[Attachment],
        channel: Option<&Channel<ClaudeEvent>>,
        question_slot: &QuestionSlot,
        permission_slot: &PermissionSlot,
        interrupt_slot: &InterruptSlot,
        token_budget: Option<&TokenBudget>,
        plan_mode: bool,
    ) -> Result<AgentResponse, AppError> {
        let result = self
            .send_turn_inner(
                text,
                attachments,
                channel,
                question_slot,
                permission_slot,
                interrupt_slot,
                token_budget,
                plan_mode,
            )
            .await;
        let _ = interrupt_slot.lock().ok().and_then(|mut g| g.take());
        let _ = question_slot.lock().ok().and_then(|mut g| g.take());
        let _ = permission_slot.lock().ok().and_then(|mut g| g.take());
        result
    }

    #[allow(clippy::too_many_arguments)]
    async fn send_turn_inner(
        &mut self,
        text: &str,
        attachments: &[Attachment],
        channel: Option<&Channel<ClaudeEvent>>,
        question_slot: &QuestionSlot,
        permission_slot: &PermissionSlot,
        interrupt_slot: &InterruptSlot,
        token_budget: Option<&TokenBudget>,
        plan_mode: bool,
    ) -> Result<AgentResponse, AppError> {
        self.ensure_initialized().await?;
        let started = Instant::now();
        let thread_id = self
            .thread_id
            .clone()
            .ok_or_else(|| AppError::Agent("Codex thread was not initialized".into()))?;
        let request_id = self.next_id();
        // Charter injection: a pending charter (set at spawn) is prepended to
        // this first task prompt — role identity first, task second. Consumed
        // on take, so subsequent turns on the same thread don't repeat it.
        let text = match self.charter_prompt.take() {
            Some(charter) => format!("{charter}\n\n{text}"),
            None => text.to_string(),
        };
        let params = turn_start_params(
            &thread_id,
            &text,
            attachments,
            &self.cwd,
            self.model.as_deref(),
            self.effort.as_deref(),
            if plan_mode {
                Some(self.plan_collaboration_mode.as_ref().ok_or_else(|| {
                    AppError::Agent(
                        "Codex did not provide a model for interactive pre-flight questions".into(),
                    )
                })?)
            } else {
                None
            },
        );
        self.write_message(json!({
            "method": "turn/start",
            "id": request_id,
            "params": params
        }))
        .await?;

        let (interrupt_tx, mut interrupt_rx) = oneshot::channel::<()>();
        *interrupt_slot.lock().map_err(|_| AppError::LockError)? = Some(interrupt_tx);

        let mut text_acc = String::new();
        let mut thinking_acc = String::new();
        let mut tool_calls = Vec::new();
        let mut blocks = Vec::new();
        let mut seen_tools = HashSet::new();
        let mut item_tools: HashMap<String, (String, Value)> = HashMap::new();
        let mut usage: Option<UsageInfo> = None;
        let mut final_answer: Option<String> = None;
        let mut active_turn_id: Option<String> = None;
        let mut start_response_seen = false;
        let mut interrupt_requested = false;
        let mut interrupt_sent = false;
        let mut interrupt_deadline: Option<tokio::time::Instant> = None;
        let mut budget_exceeded: Option<String> = None;

        loop {
            let stop_deadline = interrupt_deadline;
            let message = tokio::select! {
                biased;
                _ = async move {
                    match stop_deadline {
                        Some(deadline) => tokio::time::sleep_until(deadline).await,
                        None => std::future::pending::<()>().await,
                    }
                } => {
                    self.abort_child();
                    return Err(match budget_exceeded {
                        Some(reason) => AppError::Limit(reason),
                        None => AppError::Agent(format!(
                            "Codex did not complete the interrupted turn within {}s; the wedged app-server was terminated",
                            INTERRUPT_GRACE_TIMEOUT.as_secs()
                        )),
                    });
                }
                interrupt = &mut interrupt_rx, if !interrupt_requested => {
                    if interrupt.is_ok() {
                        interrupt_requested = true;
                        interrupt_deadline =
                            Some(tokio::time::Instant::now() + INTERRUPT_GRACE_TIMEOUT);
                        if let Some(turn_id) = active_turn_id.as_deref() {
                            let id = self.next_id();
                            self.write_message(json!({
                                "method": "turn/interrupt",
                                "id": id,
                                "params": { "threadId": thread_id, "turnId": turn_id }
                            })).await?;
                            interrupt_sent = true;
                        }
                    }
                    continue;
                }
                message = async {
                    if start_response_seen {
                        self.read_message().await
                    } else {
                        self.read_message_with_timeout(Some(HANDSHAKE_READ_TIMEOUT)).await
                    }
                } => message?,
            };

            if message.get("id").and_then(Value::as_u64) == Some(request_id) {
                start_response_seen = true;
                if let Some(error) = message.get("error") {
                    return Err(AppError::Agent(format_rpc_error(error)));
                }
                active_turn_id = message
                    .pointer("/result/turn/id")
                    .and_then(Value::as_str)
                    .map(ToOwned::to_owned);
                if interrupt_requested && !interrupt_sent {
                    if let Some(turn_id) = active_turn_id.as_deref() {
                        let id = self.next_id();
                        self.write_message(json!({
                            "method": "turn/interrupt",
                            "id": id,
                            "params": { "threadId": thread_id, "turnId": turn_id }
                        }))
                        .await?;
                        interrupt_sent = true;
                        interrupt_deadline =
                            Some(tokio::time::Instant::now() + INTERRUPT_GRACE_TIMEOUT);
                    }
                }
                continue;
            }

            if message.get("id").is_some() && message.get("method").is_some() {
                self.handle_server_request(
                    &message,
                    channel,
                    question_slot,
                    permission_slot,
                    &item_tools,
                )
                .await?;
                continue;
            }

            let Some(method) = message.get("method").and_then(Value::as_str) else {
                continue;
            };
            let params = message.get("params").cloned().unwrap_or(Value::Null);
            match method {
                "turn/started" => {
                    active_turn_id = params
                        .pointer("/turn/id")
                        .and_then(Value::as_str)
                        .map(ToOwned::to_owned);
                }
                "item/agentMessage/delta" => {
                    if let Some(delta) = params.get("delta").and_then(Value::as_str) {
                        text_acc.push_str(delta);
                        append_text_block(&mut blocks, delta);
                        if let Some(channel) = channel {
                            let _ = channel.send(ClaudeEvent::TextDelta {
                                text: delta.to_owned(),
                            });
                        }
                    }
                }
                "item/reasoning/summaryTextDelta" | "item/reasoning/textDelta" => {
                    if let Some(delta) = params.get("delta").and_then(Value::as_str) {
                        thinking_acc.push_str(delta);
                        append_thinking_block(&mut blocks, delta);
                        if let Some(channel) = channel {
                            let _ = channel.send(ClaudeEvent::ThinkingDelta {
                                thinking: delta.to_owned(),
                            });
                        }
                    }
                }
                "item/started" => {
                    if let Some(item) = params.get("item") {
                        if item.get("type").and_then(Value::as_str) == Some("dynamicToolCall") {
                            log_dynamic_tool_call("started", item);
                        }
                        if let Some((name, input)) = tool_from_item(item) {
                            if let Some(item_id) = item.get("id").and_then(Value::as_str) {
                                item_tools
                                    .insert(item_id.to_owned(), (name.clone(), input.clone()));
                                if seen_tools.insert(item_id.to_owned()) {
                                    let input_string = input.to_string();
                                    tool_calls.push(ToolCallRecord {
                                        name: name.clone(),
                                        input: input_string.clone(),
                                    });
                                    blocks.push(ContentBlockRecord::ToolUse {
                                        name: name.clone(),
                                        input: input_string.clone(),
                                    });
                                    if let Some(channel) = channel {
                                        let _ = channel.send(ClaudeEvent::ToolUse {
                                            name,
                                            input: input_string,
                                        });
                                    }
                                }
                            }
                        }
                    }
                }
                "item/completed" => {
                    if let Some(item) = params.get("item") {
                        if item.get("type").and_then(Value::as_str) == Some("agentMessage") {
                            if let Some(final_text) = item.get("text").and_then(Value::as_str) {
                                if item.get("phase").and_then(Value::as_str) == Some("final_answer")
                                {
                                    final_answer = Some(final_text.to_owned());
                                }
                                // Deltas are expected, but the completed item
                                // is authoritative and can be the only text on
                                // older/newer servers or after notification
                                // suppression. Preserve a usable transcript.
                                if text_acc.is_empty() && !final_text.is_empty() {
                                    text_acc.push_str(final_text);
                                    append_text_block(&mut blocks, final_text);
                                    if let Some(channel) = channel {
                                        let _ = channel.send(ClaudeEvent::TextDelta {
                                            text: final_text.to_owned(),
                                        });
                                    }
                                }
                            }
                        }
                    }
                }
                "thread/tokenUsage/updated" => {
                    let last = params.pointer("/tokenUsage/last");
                    if let Some(last) = last {
                        usage = Some(UsageInfo {
                            input_tokens: nonnegative_u64(last.get("inputTokens")),
                            output_tokens: nonnegative_u64(last.get("outputTokens")),
                            total_cost_usd: 0.0,
                        });
                    }
                    if budget_exceeded.is_none() {
                        if let (Some(budget), Some(total)) =
                            (token_budget, params.pointer("/tokenUsage/total"))
                        {
                            let tokens = nonnegative_u64(total.get("inputTokens"))
                                .saturating_add(nonnegative_u64(total.get("outputTokens")));
                            if budget.observe_total(tokens) {
                                budget_exceeded = Some(budget.exceeded_message());
                                interrupt_requested = true;
                                interrupt_deadline =
                                    Some(tokio::time::Instant::now() + INTERRUPT_GRACE_TIMEOUT);
                                if let Some(turn_id) = active_turn_id.as_deref() {
                                    let id = self.next_id();
                                    self.write_message(json!({
                                        "method": "turn/interrupt",
                                        "id": id,
                                        "params": { "threadId": thread_id, "turnId": turn_id }
                                    }))
                                    .await?;
                                    interrupt_sent = true;
                                }
                            }
                        }
                    }
                }
                "error" => {
                    let error = params.get("error").unwrap_or(&params);
                    tracing::warn!(target: "loopdeck::codex", "Codex turn error: {error}");
                }
                "turn/completed" => {
                    let turn = params.get("turn").unwrap_or(&Value::Null);
                    let status = turn
                        .get("status")
                        .and_then(Value::as_str)
                        .unwrap_or("failed");
                    let error_message = turn
                        .pointer("/error/message")
                        .and_then(Value::as_str)
                        .unwrap_or_default();
                    let is_error = status == "failed";
                    let result = if is_error && !error_message.is_empty() {
                        error_message.to_owned()
                    } else if interrupt_requested || status == "interrupted" {
                        format!("(interrupted) {text_acc}")
                    } else {
                        final_answer.unwrap_or_else(|| text_acc.clone())
                    };
                    let duration_ms =
                        u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
                    let response = AgentResponse {
                        text: text_acc.clone(),
                        thinking: (!thinking_acc.is_empty()).then_some(thinking_acc.clone()),
                        result: result.clone(),
                        usage: usage.clone(),
                        is_error,
                        duration_ms,
                        session_id: format!("codex:{thread_id}"),
                        tool_calls,
                        blocks,
                        tasks: Vec::new(),
                    };
                    if let Some(channel) = channel {
                        let _ = channel.send(ClaudeEvent::Result {
                            text: response.text.clone(),
                            thinking: response.thinking.clone(),
                            result: response.result.clone(),
                            usage: response.usage.clone(),
                            is_error: response.is_error,
                            duration_ms: response.duration_ms,
                            session_id: response.session_id.clone(),
                        });
                    }
                    if let Some(reason) = budget_exceeded {
                        return Err(AppError::Limit(reason));
                    }
                    if interrupt_requested || status == "interrupted" {
                        return Err(AppError::Agent("turn interrupted by user".into()));
                    }
                    return Ok(response);
                }
                _ => {}
            }

            if interrupt_requested && !interrupt_sent {
                if let Some(turn_id) = active_turn_id.as_deref() {
                    let id = self.next_id();
                    self.write_message(json!({
                        "method": "turn/interrupt",
                        "id": id,
                        "params": { "threadId": thread_id, "turnId": turn_id }
                    }))
                    .await?;
                    interrupt_sent = true;
                    interrupt_deadline =
                        Some(tokio::time::Instant::now() + INTERRUPT_GRACE_TIMEOUT);
                }
            }

            if !start_response_seen && self.child_exited()? {
                return Err(AppError::Agent(
                    "Codex app-server exited before accepting the turn".into(),
                ));
            }
        }
    }

    async fn handle_server_request(
        &mut self,
        message: &Value,
        channel: Option<&Channel<ClaudeEvent>>,
        question_slot: &QuestionSlot,
        permission_slot: &PermissionSlot,
        item_tools: &HashMap<String, (String, Value)>,
    ) -> Result<(), AppError> {
        let id = message
            .get("id")
            .cloned()
            .ok_or_else(|| AppError::Agent("Codex server request missing id".into()))?;
        let request_id = display_request_id(&id);
        let method = message
            .get("method")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let params = message.get("params").cloned().unwrap_or(Value::Null);

        match method {
            "item/tool/call" => {
                let tool = params.get("tool").and_then(Value::as_str).unwrap_or("Tool");
                let arguments = params
                    .get("arguments")
                    .map(Value::to_string)
                    .unwrap_or_else(|| "null".into());
                let call_id = params
                    .get("callId")
                    .and_then(Value::as_str)
                    .unwrap_or("unknown");
                tracing::info!(
                    target: "loopdeck::codex",
                    tool,
                    call_id,
                    arguments = %truncate_log_value(&arguments),
                    "Codex dynamic tool call requested"
                );
                // Code Mode's host currently owns execution. This response is
                // retained as a truthful fallback for client-declared dynamic
                // tools that Selasar has not registered.
                self.write_message(json!({
                    "id": id,
                    "error": {
                        "code": -32601,
                        "message": "Selasar does not implement client-declared dynamic tools"
                    }
                }))
                .await?;
            }
            "item/tool/requestUserInput" => {
                let questions = codex_questions(&params);
                let (sender, receiver) = oneshot::channel::<QuestionAnswers>();
                *question_slot.lock().map_err(|_| AppError::LockError)? = Some(PendingQuestion {
                    request_id: request_id.clone(),
                    questions: questions.clone(),
                    sender: Some(sender),
                });
                if let Some(channel) = channel {
                    let _ = channel.send(ClaudeEvent::AskUserQuestion {
                        request_id,
                        tool_name: "AskUserQuestion".into(),
                        questions: questions.clone(),
                    });
                }
                let answers = receiver.await.map_err(|_| {
                    AppError::Agent("Codex user-question answer channel closed".into())
                })?;
                let mut mapped = serde_json::Map::new();
                for (index, question) in questions.iter().enumerate() {
                    let key = params
                        .pointer(&format!("/questions/{index}/id"))
                        .and_then(Value::as_str)
                        .unwrap_or(&question.question);
                    let answer = answers
                        .get(&question.question)
                        .map(|answer| answer.as_multi())
                        .unwrap_or_default();
                    mapped.insert(key.to_owned(), json!({ "answers": answer }));
                }
                self.write_message(json!({
                    "id": id,
                    "result": { "answers": mapped }
                }))
                .await?;
            }
            "item/commandExecution/requestApproval" | "item/fileChange/requestApproval" => {
                let (tool_name, input) = approval_tool_context(method, &params, item_tools);
                let decision = self
                    .resolve_permission(request_id, tool_name, input, channel, permission_slot)
                    .await?;
                let wire_decision = match decision {
                    Decision::Allow => "accept",
                    Decision::Deny(_) => "decline",
                };
                self.write_message(json!({
                    "id": id,
                    "result": { "decision": wire_decision }
                }))
                .await?;
            }
            "item/permissions/requestApproval" => {
                let input = json!({
                    "permissions": params.get("permissions").cloned().unwrap_or_else(|| json!({})),
                    "cwd": params.get("cwd").cloned().unwrap_or(Value::Null),
                });
                let decision = self
                    .resolve_permission(
                        request_id,
                        "RequestPermissions".into(),
                        input,
                        channel,
                        permission_slot,
                    )
                    .await?;
                let permissions = match decision {
                    Decision::Allow => params
                        .get("permissions")
                        .cloned()
                        .unwrap_or_else(|| json!({})),
                    Decision::Deny(_) => json!({}),
                };
                self.write_message(json!({
                    "id": id,
                    "result": { "permissions": permissions, "scope": "turn" }
                }))
                .await?;
            }
            _ => {
                // Unknown client-side request: return a JSON-RPC-style error so
                // Codex can recover instead of hanging forever.
                self.write_message(json!({
                    "id": id,
                    "error": {
                        "code": -32601,
                        "message": format!("Selasar does not implement {method}")
                    }
                }))
                .await?;
            }
        }
        Ok(())
    }

    async fn resolve_permission(
        &self,
        request_id: String,
        tool_name: String,
        input: Value,
        channel: Option<&Channel<ClaudeEvent>>,
        permission_slot: &PermissionSlot,
    ) -> Result<Decision, AppError> {
        if let Some(decision) = automatic_permission_decision(&self.policy, &tool_name, &input) {
            match &decision {
                Decision::Deny(reason) => {
                    emit_permission(channel, &request_id, &tool_name, &input, "deny", reason);
                }
                Decision::Allow => {
                    emit_permission(channel, &request_id, &tool_name, &input, "auto-allow", "");
                }
            }
            return Ok(decision);
        }

        let (sender, receiver) = oneshot::channel::<Decision>();
        *permission_slot.lock().map_err(|_| AppError::LockError)? = Some(PendingPermission {
            request_id: request_id.clone(),
            tool_name: tool_name.clone(),
            input: input.to_string(),
            sender: Some(sender),
        });
        emit_permission(channel, &request_id, &tool_name, &input, "pending", "");
        let decision = receiver
            .await
            .map_err(|_| AppError::Agent("Codex permission answer channel closed".into()))?;
        emit_permission(
            channel,
            &request_id,
            &tool_name,
            &input,
            decision.behavior(),
            decision.reason(),
        );
        Ok(decision)
    }

    fn next_id(&mut self) -> u64 {
        let id = self.next_request_id;
        self.next_request_id = self.next_request_id.saturating_add(1);
        id
    }

    async fn write_message(&mut self, message: Value) -> Result<(), AppError> {
        tracing::debug!(target: "loopdeck::codex_wire", "→ codex: {message}");
        let stdin = self
            .stdin
            .as_mut()
            .ok_or_else(|| AppError::Agent("Codex app-server stdin already closed".into()))?;
        let mut bytes = serde_json::to_vec(&message)
            .map_err(|e| AppError::Agent(format!("encode Codex request failed: {e}")))?;
        bytes.push(b'\n');
        stdin
            .write_all(&bytes)
            .await
            .map_err(|e| AppError::Agent(format!("write to Codex failed: {e}")))?;
        stdin
            .flush()
            .await
            .map_err(|e| AppError::Agent(format!("flush Codex request failed: {e}")))
    }

    async fn read_message(&mut self) -> Result<Value, AppError> {
        self.read_message_with_timeout(None).await
    }

    async fn read_message_with_timeout(
        &mut self,
        timeout: Option<Duration>,
    ) -> Result<Value, AppError> {
        let mut line = String::new();
        let bytes = match timeout {
            Some(timeout) => tokio::time::timeout(timeout, self.stdout.read_line(&mut line))
                .await
                .map_err(|_| {
                    AppError::Agent(format!(
                        "Codex app-server did not respond to a handshake within {}s",
                        timeout.as_secs()
                    ))
                })?
                .map_err(|e| AppError::Agent(format!("read from Codex failed: {e}")))?,
            None => self
                .stdout
                .read_line(&mut line)
                .await
                .map_err(|e| AppError::Agent(format!("read from Codex failed: {e}")))?,
        };
        if bytes == 0 {
            return Err(AppError::Agent(
                "Codex app-server closed stdout unexpectedly".into(),
            ));
        }
        if bytes > crate::limits::STREAM_LINE_MAX_BYTES {
            return Err(AppError::Limit(format!(
                "Codex stream line exceeded {} bytes",
                crate::limits::STREAM_LINE_MAX_BYTES
            )));
        }
        let trimmed = line.trim();
        tracing::debug!(target: "loopdeck::codex_wire", "← codex: {trimmed}");
        serde_json::from_str(trimmed)
            .map_err(|e| AppError::Agent(format!("invalid Codex JSON message: {e}")))
    }

    async fn wait_for_response(&mut self, id: u64) -> Result<Value, AppError> {
        loop {
            let message = self
                .read_message_with_timeout(Some(HANDSHAKE_READ_TIMEOUT))
                .await?;
            if message.get("id").and_then(Value::as_u64) != Some(id) {
                continue;
            }
            if let Some(error) = message.get("error") {
                return Err(AppError::Agent(format_rpc_error(error)));
            }
            return Ok(message.get("result").cloned().unwrap_or(Value::Null));
        }
    }

    fn child_exited(&mut self) -> Result<bool, AppError> {
        let Some(child) = self.child.as_mut() else {
            return Ok(true);
        };
        child
            .try_wait()
            .map(|status| status.is_some())
            .map_err(|e| AppError::Agent(format!("failed to inspect Codex process: {e}")))
    }

    pub(crate) fn is_usable(&mut self) -> bool {
        matches!(self.child_exited(), Ok(false))
    }

    fn abort_child(&mut self) {
        self.stdin.take();
        if let Some(mut child) = self.child.take() {
            let _ = child.start_kill();
        }
        self.initialized = false;
    }
}

impl HarnessAdapter for CodexSession {
    fn spawn(
        cwd: &Path,
        config: &AgentConfig,
        resume_session_id: Option<&str>,
        policy: PermissionPolicy,
    ) -> Result<Self, AppError> {
        Self::spawn(cwd, config, resume_session_id, policy)
    }

    fn is_usable(&mut self) -> bool {
        Self::is_usable(self)
    }

    async fn send_message(
        &mut self,
        text: &str,
        attachments: &[Attachment],
        slots: &ParkSlots<'_>,
        interrupt_slot: &InterruptSlot,
    ) -> Result<AgentResponse, AppError> {
        Self::send_message(self, text, attachments, slots, interrupt_slot).await
    }

    async fn send_message_streaming(
        &mut self,
        text: &str,
        attachments: &[Attachment],
        channel: &Channel<ClaudeEvent>,
        slots: &ParkSlots<'_>,
        interrupt_slot: &InterruptSlot,
        plan_mode: bool,
        token_budget: Option<&TokenBudget>,
    ) -> Result<AgentResponse, AppError> {
        Self::send_message_streaming(
            self,
            text,
            attachments,
            channel,
            slots,
            interrupt_slot,
            plan_mode,
            token_budget,
        )
        .await
    }
}

impl Drop for CodexSession {
    fn drop(&mut self) {
        self.stdin.take();
        if let Some(task) = self.stderr_drain.take() {
            task.abort();
        }
        if let Some(mut child) = self.child.take() {
            let _ = child.start_kill();
        }
    }
}

fn log_dynamic_tool_call(phase: &str, item: &Value) {
    let tool = item.get("tool").and_then(Value::as_str).unwrap_or("Tool");
    let arguments = item
        .get("arguments")
        .map(Value::to_string)
        .unwrap_or_else(|| "null".into());
    let item_id = item.get("id").and_then(Value::as_str).unwrap_or("unknown");
    tracing::info!(
        target: "loopdeck::codex",
        phase,
        tool,
        item_id,
        arguments = %truncate_log_value(&arguments),
        "Codex dynamic tool call"
    );
}

fn truncate_log_value(value: &str) -> String {
    if value.len() <= CODEX_TOOL_LOG_MAX_CHARS {
        return value.to_owned();
    }
    let mut end = CODEX_TOOL_LOG_MAX_CHARS;
    while !value.is_char_boundary(end) {
        end -= 1;
    }
    format!(
        "{}… [truncated after {CODEX_TOOL_LOG_MAX_CHARS} chars]",
        &value[..end]
    )
}

/// Preserve semantic block ordering without persisting one block per streamed
/// token. A tool/thinking block remains a boundary, so only adjacent text
/// deltas are coalesced.
fn append_text_block(blocks: &mut Vec<ContentBlockRecord>, delta: &str) {
    match blocks.last_mut() {
        Some(ContentBlockRecord::Text { text }) => text.push_str(delta),
        _ => blocks.push(ContentBlockRecord::Text {
            text: delta.to_owned(),
        }),
    }
}

/// Thinking deltas follow the same adjacency rule as assistant text.
fn append_thinking_block(blocks: &mut Vec<ContentBlockRecord>, delta: &str) {
    match blocks.last_mut() {
        Some(ContentBlockRecord::Thinking { thinking }) => thinking.push_str(delta),
        _ => blocks.push(ContentBlockRecord::Thinking {
            thinking: delta.to_owned(),
        }),
    }
}

fn emit_permission(
    channel: Option<&Channel<ClaudeEvent>>,
    request_id: &str,
    tool_name: &str,
    input: &Value,
    decision: &str,
    reason: &str,
) {
    if let Some(channel) = channel {
        let _ = channel.send(ClaudeEvent::PermissionRequest {
            request_id: request_id.to_owned(),
            tool_name: tool_name.to_owned(),
            input: input.to_string(),
            decision: decision.to_owned(),
            reason: reason.to_owned(),
        });
    }
}

fn codex_questions(params: &Value) -> Vec<AskUserQuestionSpec> {
    params
        .get("questions")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|question| {
            let text = question.get("question")?.as_str()?.to_owned();
            let options = question
                .get("options")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(|option| {
                    Some(AskUserQuestionOption {
                        label: option.get("label")?.as_str()?.to_owned(),
                        description: option
                            .get("description")
                            .and_then(Value::as_str)
                            .unwrap_or_default()
                            .to_owned(),
                    })
                })
                .collect();
            Some(AskUserQuestionSpec {
                question: text,
                header: question
                    .get("header")
                    .and_then(Value::as_str)
                    .unwrap_or("Question")
                    .to_owned(),
                options,
                multi_select: false,
            })
        })
        .collect()
}

fn tool_from_item(item: &Value) -> Option<(String, Value)> {
    match item.get("type")?.as_str()? {
        "commandExecution" => Some((
            "Bash".into(),
            json!({
                "command": item.get("command").cloned().unwrap_or(Value::Null),
                "cwd": item.get("cwd").cloned().unwrap_or(Value::Null),
            }),
        )),
        "fileChange" => Some((
            "Edit".into(),
            json!({ "changes": item.get("changes").cloned().unwrap_or_else(|| json!([])) }),
        )),
        "mcpToolCall" => {
            let server = item.get("server").and_then(Value::as_str).unwrap_or("mcp");
            let tool = item.get("tool").and_then(Value::as_str).unwrap_or("tool");
            Some((
                format!("mcp__{server}__{tool}"),
                item.get("arguments").cloned().unwrap_or_else(|| json!({})),
            ))
        }
        "dynamicToolCall" => Some((
            item.get("tool")
                .and_then(Value::as_str)
                .unwrap_or("Tool")
                .to_owned(),
            item.get("arguments").cloned().unwrap_or_else(|| json!({})),
        )),
        "webSearch" => Some((
            "WebSearch".into(),
            json!({ "query": item.get("query").cloned().unwrap_or(Value::Null) }),
        )),
        "imageView" => Some((
            "Read".into(),
            json!({ "path": item.get("path").cloned().unwrap_or(Value::Null) }),
        )),
        _ => None,
    }
}

/// Build the app-server capability contract advertised by Selasar.
///
/// `tool/requestUserInput` is an experimental app-server request. Selasar
/// handles it directly (without declaring client-hosted dynamic tools), so
/// turn on the capability that lets Codex ask users before unattended work.
fn initialize_params() -> Value {
    json!({
        "clientInfo": {
            "name": "loopdeck",
            "title": "Selasar",
            "version": env!("CARGO_PKG_VERSION")
        },
        "capabilities": { "experimentalApi": true }
    })
}

/// Build the per-turn security boundary and optional model overrides.
///
/// `readOnly` is intentional even for autonomous projects. It makes Codex
/// request approval before commands or edits, allowing Selasar's
/// `PermissionPolicy` to decide whether to park for the user or auto-allow,
/// while always retaining the destructive-command floor.
fn turn_start_params(
    thread_id: &str,
    text: &str,
    attachments: &[Attachment],
    cwd: &Path,
    model: Option<&str>,
    effort: Option<&str>,
    collaboration_mode: Option<&Value>,
) -> Value {
    let mut input: Vec<Value> = vec![json!({ "type": "text", "text": text })];
    input.extend(attachments.iter().map(|a| {
        json!({
            "type": "image",
            "url": format!("data:{};base64,{}", a.media_type, a.data),
        })
    }));
    let mut params = json!({
        "threadId": thread_id,
        "input": input,
        "cwd": cwd,
        "approvalPolicy": "on-request",
        "sandboxPolicy": {
            "type": "readOnly"
        }
    });
    if let Some(model) = model.filter(|value| !value.is_empty()) {
        params["model"] = Value::String(model.to_owned());
    }
    if let Some(effort) = effort.filter(|value| !value.is_empty()) {
        params["effort"] = Value::String(effort.to_owned());
    }
    if let Some(collaboration_mode) = collaboration_mode {
        params["collaborationMode"] = collaboration_mode.clone();
    }
    params
}

fn plan_collaboration_mode(
    response: &Value,
    models_response: &Value,
    configured_model: Option<&str>,
    configured_effort: Option<&str>,
) -> Option<Value> {
    let modes = response.get("data")?.as_array()?;
    let plan = modes
        .iter()
        .find(|mode| mode.get("mode").and_then(Value::as_str) == Some("plan"));
    // Some app-server versions list only the default preset even though the
    // documented `plan` mode remains valid. In that case, reuse the selected
    // Codex model rather than falsely reporting that Plan mode is unavailable.
    let default = modes
        .iter()
        .find(|mode| mode.get("mode").and_then(Value::as_str) == Some("default"));
    let available_model = preferred_codex_model(models_response);
    let model = plan
        .and_then(|mode| mode.get("model"))
        .and_then(Value::as_str)
        .or_else(|| configured_model.filter(|model| !model.is_empty()))
        .or_else(|| {
            default
                .and_then(|mode| mode.get("model"))
                .and_then(Value::as_str)
        })
        .or_else(|| {
            available_model
                .and_then(|model| model.get("model"))
                .and_then(Value::as_str)
        })
        .or_else(|| {
            available_model
                .and_then(|model| model.get("id"))
                .and_then(Value::as_str)
        })?;
    let effort = plan
        .and_then(|mode| mode.get("reasoning_effort"))
        .cloned()
        .or_else(|| {
            configured_effort
                .filter(|effort| !effort.is_empty())
                .map(|effort| Value::String(effort.to_owned()))
        })
        .or_else(|| {
            default
                .and_then(|mode| mode.get("reasoning_effort"))
                .cloned()
        })
        .or_else(|| {
            available_model
                .and_then(|model| model.get("defaultReasoningEffort"))
                .cloned()
        })
        .unwrap_or(Value::Null);
    Some(json!({
        "mode": "plan",
        "settings": {
            "model": model,
            "reasoning_effort": effort,
            "developer_instructions": Value::Null,
        }
    }))
}

/// Return Codex's recommended model, falling back to the first visible model
/// for older app-server catalogs that do not identify a default.
fn preferred_codex_model(response: &Value) -> Option<&Value> {
    let models = response.get("data")?.as_array()?;
    models
        .iter()
        .find(|model| model.get("isDefault").and_then(Value::as_bool) == Some(true))
        .or_else(|| models.first())
}

/// Resolve the approval card's tool and context.
///
/// App-server documents `item/started` before its approval request, so the
/// item map is authoritative. The request-only fallback keeps all context the
/// approval payload itself exposes if a future server violates that ordering.
fn approval_tool_context(
    method: &str,
    params: &Value,
    item_tools: &HashMap<String, (String, Value)>,
) -> (String, Value) {
    let item_id = params
        .get("itemId")
        .and_then(Value::as_str)
        .unwrap_or_default();
    if let Some(context) = item_tools.get(item_id) {
        return context.clone();
    }

    if method.contains("commandExecution") {
        (
            "Bash".to_owned(),
            json!({
                "command": params.get("command").cloned().unwrap_or(Value::Null),
                "cwd": params.get("cwd").cloned().unwrap_or(Value::Null),
                "reason": params.get("reason").cloned().unwrap_or(Value::Null),
            }),
        )
    } else {
        (
            "Edit".to_owned(),
            json!({
                "itemId": item_id,
                "reason": params.get("reason").cloned().unwrap_or(Value::Null),
                "grantRoot": params.get("grantRoot").cloned().unwrap_or(Value::Null),
            }),
        )
    }
}

/// Return an immediate policy decision, or `None` when the user must decide.
///
/// The destructive floor is evaluated before autonomous mode so autonomy can
/// never bypass Selasar's hard-deny rules. A role rule that explicitly covers
/// the request grants the same immediate allow (`decide` has already run the
/// floor by this point).
fn automatic_permission_decision(
    policy: &PermissionPolicy,
    tool_name: &str,
    input: &Value,
) -> Option<Decision> {
    match policy.decide(tool_name, input) {
        denied @ Decision::Deny(_) => Some(denied),
        Decision::Allow if policy.is_autonomous() || policy.role_allows(tool_name, input) => {
            Some(Decision::Allow)
        }
        Decision::Allow => None,
    }
}

fn display_request_id(id: &Value) -> String {
    id.as_str()
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| id.to_string())
}

fn format_rpc_error(error: &Value) -> String {
    error
        .get("message")
        .and_then(Value::as_str)
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| format!("Codex RPC error: {error}"))
}

fn nonnegative_u64(value: Option<&Value>) -> u64 {
    value
        .and_then(Value::as_i64)
        .and_then(|value| u64::try_from(value).ok())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn streamed_deltas_coalesce_without_crossing_content_boundaries() {
        let mut blocks = Vec::new();

        append_text_block(&mut blocks, "I");
        append_text_block(&mut blocks, "'ll check");
        blocks.push(ContentBlockRecord::ToolUse {
            name: "Bash".into(),
            input: r#"{"command":"pwd"}"#.into(),
        });
        append_text_block(&mut blocks, "Done");
        append_text_block(&mut blocks, ".");
        append_thinking_block(&mut blocks, "Need");
        append_thinking_block(&mut blocks, " verify");

        assert_eq!(blocks.len(), 4);
        assert!(matches!(
            &blocks[0],
            ContentBlockRecord::Text { text } if text == "I'll check"
        ));
        assert!(matches!(
            &blocks[1],
            ContentBlockRecord::ToolUse { name, .. } if name == "Bash"
        ));
        assert!(matches!(
            &blocks[2],
            ContentBlockRecord::Text { text } if text == "Done."
        ));
        assert!(matches!(
            &blocks[3],
            ContentBlockRecord::Thinking { thinking } if thinking == "Need verify"
        ));
    }

    #[test]
    fn maps_codex_command_item_to_loopdeck_bash_tool() {
        let (name, input) = tool_from_item(&json!({
            "type": "commandExecution",
            "id": "item-1",
            "command": "cargo test",
            "cwd": "/repo"
        }))
        .expect("tool");
        assert_eq!(name, "Bash");
        assert_eq!(input["command"], "cargo test");
        assert_eq!(input["cwd"], "/repo");
    }

    #[test]
    fn maps_codex_questions_to_existing_ui_shape() {
        let questions = codex_questions(&json!({
            "questions": [{
                "id": "scope",
                "header": "Scope",
                "question": "Which scope?",
                "options": [{"label": "Small", "description": "Minimal change"}]
            }]
        }));
        assert_eq!(questions.len(), 1);
        assert_eq!(questions[0].question, "Which scope?");
        assert_eq!(questions[0].options[0].label, "Small");
    }

    #[test]
    fn request_ids_preserve_strings_and_numbers() {
        assert_eq!(display_request_id(&json!("req-1")), "req-1");
        assert_eq!(display_request_id(&json!(42)), "42");
    }

    #[test]
    fn initialize_opts_into_native_user_input_requests() {
        let params = initialize_params();

        assert_eq!(params["clientInfo"]["name"], "loopdeck");
        assert_eq!(params["capabilities"]["experimentalApi"], true);
    }

    #[test]
    fn plan_mode_uses_the_app_server_advertised_model() {
        let mode = plan_collaboration_mode(
            &json!({
                "data": [{
                    "name": "Plan",
                    "mode": "plan",
                    "model": "gpt-5.3-codex",
                    "reasoning_effort": "high"
                }]
            }),
            &json!({ "data": [] }),
            None,
            None,
        )
        .expect("plan mode");

        assert_eq!(mode["mode"], "plan");
        assert_eq!(mode["settings"]["model"], "gpt-5.3-codex");
        assert_eq!(mode["settings"]["developer_instructions"], Value::Null);
    }

    #[test]
    fn plan_mode_falls_back_to_the_configured_model_when_not_listed() {
        let mode = plan_collaboration_mode(
            &json!({
                "data": [{ "name": "Default", "mode": "default", "model": null }]
            }),
            &json!({ "data": [] }),
            Some("gpt-5.3-codex"),
            Some("medium"),
        )
        .expect("plan mode");

        assert_eq!(mode["mode"], "plan");
        assert_eq!(mode["settings"]["model"], "gpt-5.3-codex");
        assert_eq!(mode["settings"]["reasoning_effort"], "medium");
    }

    #[test]
    fn plan_mode_uses_codex_default_model_when_profile_is_unset() {
        let mode = plan_collaboration_mode(
            &json!({
                "data": [{ "name": "Default", "mode": "default", "model": null }]
            }),
            &json!({
                "data": [{
                    "id": "gpt-5.6-sol",
                    "model": "gpt-5.6-sol",
                    "defaultReasoningEffort": "low",
                    "isDefault": true
                }]
            }),
            None,
            None,
        )
        .expect("plan mode");

        assert_eq!(mode["settings"]["model"], "gpt-5.6-sol");
        assert_eq!(mode["settings"]["reasoning_effort"], "low");
    }

    #[test]
    fn turn_start_routes_commands_and_edits_through_loopdeck_approval() {
        let params = turn_start_params(
            "thread-1",
            "Make the change",
            &[],
            Path::new("/repo"),
            Some("gpt-test"),
            Some("high"),
            None,
        );

        assert_eq!(params["approvalPolicy"], "on-request");
        assert_eq!(params["sandboxPolicy"]["type"], "readOnly");
        assert_eq!(params["model"], "gpt-test");
        assert_eq!(params["effort"], "high");
    }

    #[test]
    fn turn_start_omits_empty_model_and_effort_overrides() {
        let params = turn_start_params(
            "thread-1",
            "Inspect",
            &[],
            Path::new("/repo"),
            Some(""),
            Some(""),
            None,
        );

        assert!(params.get("model").is_none());
        assert!(params.get("effort").is_none());
    }

    #[test]
    fn turn_start_input_is_text_only_without_attachments() {
        let params = turn_start_params(
            "thread-1",
            "Inspect",
            &[],
            Path::new("/repo"),
            None,
            None,
            None,
        );

        assert_eq!(
            params["input"],
            json!([{ "type": "text", "text": "Inspect" }])
        );
    }

    #[test]
    fn turn_start_appends_image_input_items_as_data_urls() {
        let attachments = [Attachment {
            media_type: "image/png".to_owned(),
            data: "Zm9v".to_owned(),
        }];
        let params = turn_start_params(
            "thread-1",
            "What is this?",
            &attachments,
            Path::new("/repo"),
            None,
            None,
            None,
        );

        assert_eq!(
            params["input"],
            json!([
                { "type": "text", "text": "What is this?" },
                { "type": "image", "url": "data:image/png;base64,Zm9v" }
            ])
        );
    }

    #[test]
    fn file_approval_uses_proposed_changes_from_started_item() {
        let mut item_tools = HashMap::new();
        item_tools.insert(
            "item-1".to_owned(),
            (
                "Edit".to_owned(),
                json!({ "changes": [{ "path": "src/main.rs", "kind": "update" }] }),
            ),
        );

        let (name, input) = approval_tool_context(
            "item/fileChange/requestApproval",
            &json!({
                "itemId": "item-1",
                "reason": "Apply the requested fix"
            }),
            &item_tools,
        );

        assert_eq!(name, "Edit");
        assert_eq!(input["changes"][0]["path"], "src/main.rs");
    }

    #[test]
    fn file_approval_fallback_preserves_request_context() {
        let (name, input) = approval_tool_context(
            "item/fileChange/requestApproval",
            &json!({
                "itemId": "item-1",
                "reason": "Write outside the current root",
                "grantRoot": "/other"
            }),
            &HashMap::new(),
        );

        assert_eq!(name, "Edit");
        assert_eq!(input["reason"], "Write outside the current root");
        assert_eq!(input["grantRoot"], "/other");
    }

    #[test]
    fn confirm_changes_parks_safe_codex_requests_for_the_user() {
        let decision = automatic_permission_decision(
            &PermissionPolicy::confirm_changes(),
            "Bash",
            &json!({ "command": "cargo test" }),
        );

        assert!(decision.is_none());
    }

    #[test]
    fn autonomous_mode_auto_allows_safe_codex_requests() {
        let policy = PermissionPolicy::with_mode(crate::permission::PermissionMode::Autonomous);
        let decision =
            automatic_permission_decision(&policy, "Bash", &json!({ "command": "cargo test" }));

        assert_eq!(decision, Some(Decision::Allow));
    }

    #[test]
    fn autonomous_mode_cannot_bypass_destructive_floor() {
        let policy = PermissionPolicy::with_mode(crate::permission::PermissionMode::Autonomous);
        let decision =
            automatic_permission_decision(&policy, "Bash", &json!({ "command": "rm -rf /" }));

        assert!(matches!(decision, Some(Decision::Deny(_))));
    }
}
