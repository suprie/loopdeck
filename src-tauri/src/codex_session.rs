//! Persistent Codex app-server adapter.
//!
//! The app-server protocol is newline-delimited JSON-RPC over stdio. One
//! process owns one LoopDeck project thread; turns reuse that thread, while a
//! persisted `codex:<thread-id>` resumes it after an app restart.

use crate::agents::{
    AgentResponse, AskUserQuestionOption, AskUserQuestionSpec, ClaudeEvent, ContentBlockRecord,
    UsageInfo,
};
use crate::claude_session::{
    InterruptSlot, PendingPermission, PendingQuestion, PermissionSlot, QuestionAnswers,
    QuestionSlot,
};
use crate::config::AgentConfig;
use crate::conversation::ToolCallRecord;
use crate::error::AppError;
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

const READ_TIMEOUT: Duration = Duration::from_secs(300);

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
    policy: PermissionPolicy,
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

        let mut child = command
            .spawn()
            .map_err(|e| AppError::Agent(format!("failed to start Codex app-server: {e}")))?;
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
            policy,
        })
    }

    pub async fn send_message(
        &mut self,
        text: &str,
        question_slot: &QuestionSlot,
        permission_slot: &PermissionSlot,
        interrupt_slot: &InterruptSlot,
    ) -> Result<AgentResponse, AppError> {
        self.send_turn(text, None, question_slot, permission_slot, interrupt_slot)
            .await
    }

    pub async fn send_message_streaming(
        &mut self,
        text: &str,
        channel: &Channel<ClaudeEvent>,
        question_slot: &QuestionSlot,
        permission_slot: &PermissionSlot,
        interrupt_slot: &InterruptSlot,
    ) -> Result<AgentResponse, AppError> {
        self.send_turn(
            text,
            Some(channel),
            question_slot,
            permission_slot,
            interrupt_slot,
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
            "params": {
                "clientInfo": {
                    "name": "loopdeck",
                    "title": "LoopDeck",
                    "version": env!("CARGO_PKG_VERSION")
                },
                "capabilities": { "experimentalApi": true }
            }
        }))
        .await?;
        self.write_message(json!({"method": "initialized", "params": {}}))
            .await?;
        self.wait_for_response(initialize_id).await?;

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

    async fn send_turn(
        &mut self,
        text: &str,
        channel: Option<&Channel<ClaudeEvent>>,
        question_slot: &QuestionSlot,
        permission_slot: &PermissionSlot,
        interrupt_slot: &InterruptSlot,
    ) -> Result<AgentResponse, AppError> {
        let result = self
            .send_turn_inner(
                text,
                channel,
                question_slot,
                permission_slot,
                interrupt_slot,
            )
            .await;
        let _ = interrupt_slot.lock().ok().and_then(|mut g| g.take());
        let _ = question_slot.lock().ok().and_then(|mut g| g.take());
        let _ = permission_slot.lock().ok().and_then(|mut g| g.take());
        result
    }

    async fn send_turn_inner(
        &mut self,
        text: &str,
        channel: Option<&Channel<ClaudeEvent>>,
        question_slot: &QuestionSlot,
        permission_slot: &PermissionSlot,
        interrupt_slot: &InterruptSlot,
    ) -> Result<AgentResponse, AppError> {
        self.ensure_initialized().await?;
        let started = Instant::now();
        let thread_id = self
            .thread_id
            .clone()
            .ok_or_else(|| AppError::Agent("Codex thread was not initialized".into()))?;
        let request_id = self.next_id();
        let mut params = json!({
            "threadId": thread_id,
            "input": [{ "type": "text", "text": text }],
            "cwd": self.cwd,
            "approvalPolicy": "on-request",
            "sandboxPolicy": {
                "type": "workspaceWrite",
                "writableRoots": [self.cwd],
                "networkAccess": false
            }
        });
        if let Some(model) = &self.model {
            params["model"] = Value::String(model.clone());
        }
        if let Some(effort) = &self.effort {
            params["effort"] = Value::String(effort.clone());
        }
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

        loop {
            let message = tokio::select! {
                biased;
                interrupt = &mut interrupt_rx, if !interrupt_requested => {
                    if interrupt.is_ok() {
                        interrupt_requested = true;
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
                message = self.read_message() => message?,
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
                        blocks.push(ContentBlockRecord::Text {
                            text: delta.to_owned(),
                        });
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
                        blocks.push(ContentBlockRecord::Thinking {
                            thinking: delta.to_owned(),
                        });
                        if let Some(channel) = channel {
                            let _ = channel.send(ClaudeEvent::ThinkingDelta {
                                thinking: delta.to_owned(),
                            });
                        }
                    }
                }
                "item/started" => {
                    if let Some(item) = params.get("item") {
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
                                    blocks.push(ContentBlockRecord::Text {
                                        text: final_text.to_owned(),
                                    });
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
                let item_id = params
                    .get("itemId")
                    .and_then(Value::as_str)
                    .unwrap_or_default();
                let fallback = if method.contains("commandExecution") {
                    (
                        "Bash".to_owned(),
                        json!({
                            "command": params.get("command").cloned().unwrap_or(Value::Null),
                            "cwd": params.get("cwd").cloned().unwrap_or(Value::Null),
                        }),
                    )
                } else {
                    ("Edit".to_owned(), json!({ "itemId": item_id }))
                };
                let (tool_name, input) = item_tools.get(item_id).cloned().unwrap_or(fallback);
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
                        "message": format!("LoopDeck does not implement {method}")
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
        let floor_decision = self.policy.decide(&tool_name, &input);
        if let Decision::Deny(reason) = floor_decision {
            emit_permission(channel, &request_id, &tool_name, &input, "deny", &reason);
            return Ok(Decision::Deny(reason));
        }
        if self.policy.is_autonomous() {
            emit_permission(channel, &request_id, &tool_name, &input, "auto-allow", "");
            return Ok(Decision::Allow);
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
        let mut line = String::new();
        let bytes = tokio::time::timeout(READ_TIMEOUT, self.stdout.read_line(&mut line))
            .await
            .map_err(|_| {
                AppError::Agent(format!(
                    "Codex produced no stdout for {}s — assuming stuck",
                    READ_TIMEOUT.as_secs()
                ))
            })?
            .map_err(|e| AppError::Agent(format!("read from Codex failed: {e}")))?;
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
            let message = self.read_message().await?;
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
}
