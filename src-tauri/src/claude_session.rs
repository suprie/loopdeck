use crate::agents::{
    apply_agent_config, parse_stream_line, AgentResponse, ClaudeEvent, ContentBlock,
    ResponseAccumulator, StreamEvent,
};
use crate::config::AgentConfig;
use crate::error::AppError;
use std::path::PathBuf;
use std::process::Stdio;
use std::time::{Duration, Instant};
use tauri::ipc::Channel;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin, ChildStdout, Command};
use tokio::task::JoinHandle;

/// Upper bound on a single `send_message` turn.
///
/// Generous enough to cover a real agent turn (incl. tool use / file reads),
/// but bounded so a stuck peer fails loudly instead of hanging the caller.
/// A timed-out session is left in an inconsistent state — the caller should
/// drop it (Drop cleans up) rather than send again.
const SEND_MESSAGE_TIMEOUT: Duration = Duration::from_secs(180);

/// A long-lived `claude --input-format stream-json` process.
///
/// Unlike `agents::call_agents` (which spawns a fresh process per prompt and
/// relies on `--resume`), a `ClaudeSession` spawns once and keeps stdin open.
/// Conversation context lives inside the process itself, so follow-up turns
/// are just another line written to stdin — no `--resume`, no respawn.
pub struct ClaudeSession {
    child: Child,
    // `Option` so `Drop` can close stdin explicitly before reaping the child;
    // also doubles as a "still usable" flag.
    stdin: Option<ChildStdin>,
    stdout: BufReader<ChildStdout>,
    // Background task that drains claude's stderr so a verbose child can't
    // fill its OS pipe buffer and deadlock. `Option` so `Drop` can abort it.
    // Ends naturally on stderr EOF when the child exits.
    stderr_drain: Option<JoinHandle<()>>,
}

impl ClaudeSession {
    /// Spawn a persistent `claude --input-format stream-json` process.
    ///
    /// `resume_session_id` is forwarded to claude as `--resume <id>` when set,
    /// so a process re-spawned after an app restart restores the model's
    /// conversation context. `None` starts a fresh conversation. Within a
    /// single live process, context is retained across turns automatically —
    /// resume is only needed for cross-restart continuity.
    pub fn spawn(
        project_path: &PathBuf,
        agent_config: &AgentConfig,
        resume_session_id: Option<&str>,
    ) -> Result<Self, AppError> {
        let mut cmd = Command::new("claude");
        cmd.args([
            "--input-format",
            "stream-json",
            "--output-format",
            "stream-json",
            "--verbose",
        ]);
        if let Some(id) = resume_session_id {
            cmd.args(["--resume", id]);
        }
        // Reuse the same env-var wiring as call_agents so the single-shot and
        // persistent paths can't drift out of sync.
        apply_agent_config(&mut cmd, agent_config);
        cmd.current_dir(project_path);
        cmd.stdin(Stdio::piped());
        cmd.stdout(Stdio::piped());
        // stderr must be piped (not null) so a verbose child can't fill its
        // OS pipe buffer and deadlock. A drain task below keeps it empty and
        // surfaces the output via tracing.
        cmd.stderr(Stdio::piped());

        let mut child = cmd
            .spawn()
            .map_err(|e| AppError::Agent(format!("failed to spawn claude: {}", e)))?;

        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| AppError::Agent("failed to get stdin".into()))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| AppError::Agent("failed to get stdout".into()))?;
        let stderr = child
            .stderr
            .take()
            .ok_or_else(|| AppError::Agent("failed to get stderr".into()))?;

        // Drain claude's stderr in the background so it can't deadlock the
        // stdout read. Verbose `--verbose` output lands here as line-delimited
        // diagnostics; we log them at debug so they're available when
        // troubleshooting but don't spam at the default level.
        let stderr_drain = tokio::spawn(async move {
            let mut reader = BufReader::new(stderr);
            let mut line = String::new();
            loop {
                line.clear();
                match reader.read_line(&mut line).await {
                    Ok(0) => return, // EOF — child closed stderr (usually exiting)
                    Ok(_) => {
                        let trimmed = line.trim();
                        if !trimmed.is_empty() {
                            tracing::debug!("[claude stderr] {trimmed}");
                        }
                    }
                    Err(e) => {
                        tracing::warn!("claude stderr read error: {e}");
                        return;
                    }
                }
            }
        });

        Ok(Self {
            child,
            stdin: Some(stdin),
            stdout: BufReader::new(stdout),
            stderr_drain: Some(stderr_drain),
        })
    }

    /// Send one user turn and block until claude emits its `result` event.
    ///
    /// Returns the aggregated `AgentResponse` for the whole turn. Output is
    /// buffered until the turn completes — this is the batch (non-streaming)
    /// variant; a future version can emit each `StreamEvent` to the frontend
    /// as it arrives.
    pub async fn send_message(&mut self, text: &str) -> Result<AgentResponse, AppError> {
        // Bound the whole turn so a stuck peer fails loudly instead of
        // hanging the caller. On timeout the session is left mid-turn (a late
        // `result` could still arrive) — the caller should drop it rather than
        // send again. See SEND_MESSAGE_TIMEOUT doc comment above.
        tokio::time::timeout(SEND_MESSAGE_TIMEOUT, async {
            // ---- write the user turn to stdin ----
            let stdin = self
                .stdin
                .as_mut()
                .ok_or_else(|| AppError::Agent("session stdin already closed".into()))?;

            let input = serde_json::json!({
                "type": "user",
                "message": {
                    "role": "user",
                    "content": [{"type": "text", "text": text}]
                }
            });
            let json_str = format!("{}\n", input);
            stdin
                .write_all(json_str.as_bytes())
                .await
                .map_err(|e| AppError::Agent(format!("write failed: {}", e)))?;

            stdin
                .flush()
                .await
                .map_err(|e| AppError::Agent(format!("flush failed: {}", e)))?;
            // ---- read stdout until the terminal `result` event ----
            let mut acc = ResponseAccumulator::new();
            let mut line = String::new();
            loop {
                line.clear();
                // read_line returns 0 only on EOF. With a persistent process that
                // means it died — treat as an error rather than a turn boundary.
                let n = self
                    .stdout
                    .read_line(&mut line)
                    .await
                    .map_err(|e| AppError::Agent(format!("read failed: {}", e)))?;

                if n == 0 {
                    return Err(AppError::Agent(
                        "claude closed stdout before sending a result event".into(),
                    ));
                }

                let trimmed = line.trim();
                if trimmed.is_empty() {
                    continue;
                }

                let is_result = match parse_stream_line(trimmed) {
                    Some(event) => acc.ingest_event(event),
                    None => continue, // not a recognized event line — skip silently
                };

                if is_result {
                    break;
                }
            }

            acc.finish()
                .ok_or_else(|| AppError::Agent("no result event from claude".into()))
        })
        .await
        .map_err(|_| {
            AppError::Agent(format!(
                "send_message timed out after {}s",
                SEND_MESSAGE_TIMEOUT.as_secs()
            ))
        })?
    }

    /// Send one user turn and stream events to the frontend as they arrive.
    ///
    /// Like `send_message`, but instead of buffering all output until the turn
    /// completes, each assistant content block is emitted immediately via
    /// `channel`. The frontend receives:
    ///
    /// - Zero or more `TextDelta` / `ThinkingDelta` events (one per content block).
    /// - Exactly one terminal `Result` event with the aggregated response.
    ///
    /// Channel sends are best-effort — if the frontend closes the channel
    /// (e.g. navigates away mid-turn), the send is silently dropped and the
    /// turn continues to completion (the transcript is still recorded).
    pub async fn send_message_streaming(
        &mut self,
        text: &str,
        channel: &Channel<ClaudeEvent>,
    ) -> Result<AgentResponse, AppError> {
        tokio::time::timeout(SEND_MESSAGE_TIMEOUT, async {
            // ---- write the user turn to stdin ----
            let stdin = self
                .stdin
                .as_mut()
                .ok_or_else(|| AppError::Agent("session stdin already closed".into()))?;

            let input = serde_json::json!({
                "type": "user",
                "message": {
                    "role": "user",
                    "content": [{"type": "text", "text": text}]
                }
            });
            let json_str = format!("{}\n", input);
            stdin
                .write_all(json_str.as_bytes())
                .await
                .map_err(|e| AppError::Agent(format!("write failed: {}", e)))?;

            stdin
                .flush()
                .await
                .map_err(|e| AppError::Agent(format!("flush failed: {}", e)))?;

            // ---- read stdout, emit per-block events as they arrive ----
            let mut acc = ResponseAccumulator::new();
            let mut line = String::new();
            loop {
                line.clear();
                let n = self
                    .stdout
                    .read_line(&mut line)
                    .await
                    .map_err(|e| AppError::Agent(format!("read failed: {}", e)))?;

                if n == 0 {
                    return Err(AppError::Agent(
                        "claude closed stdout before sending a result event".into(),
                    ));
                }

                let trimmed = line.trim();
                if trimmed.is_empty() {
                    continue;
                }

                let stream_event = match parse_stream_line(trimmed) {
                    Some(ev) => ev,
                    None => continue, // unrecognized line — skip
                };

                // Emit per-content-block events for assistant messages.
                // The accumulator also processes the message below (capturing
                // session_id, text, and thinking for the terminal Result event).
                if let StreamEvent::Assistant { message, .. } = &stream_event {
                    for block in &message.content {
                        match block {
                            ContentBlock::Text { text: t } => {
                                let _ = channel.send(ClaudeEvent::TextDelta {
                                    text: t.clone(),
                                });
                            }
                            ContentBlock::Thinking { thinking: th } => {
                                let _ = channel.send(ClaudeEvent::ThinkingDelta {
                                    thinking: th.clone(),
                                });
                            }
                            // Surface tool calls as live activity. During an
                            // agentic turn (reading files, editing, running
                            // commands) text deltas are sparse — without these
                            // events the UI would show only a spinner for the
                            // whole multi-minute turn.
                            ContentBlock::ToolUse { name, input } => {
                                let _ = channel.send(ClaudeEvent::ToolUse {
                                    name: name.clone(),
                                    input: input.to_string(),
                                });
                            }
                        }
                    }
                }

                let is_result = acc.ingest_event(stream_event);

                if is_result {
                    break;
                }
            }

            let response = acc
                .finish()
                .ok_or_else(|| AppError::Agent("no result event from claude".into()))?;

            // Emit the terminal result event so the frontend can reconcile
            // streamed deltas and display usage/duration in one payload.
            let _ = channel.send(ClaudeEvent::Result {
                text: response.text.clone(),
                thinking: response.thinking.clone(),
                result: response.result.clone(),
                usage: response.usage.clone(),
                is_error: response.is_error,
                duration_ms: response.duration_ms,
                session_id: response.session_id.clone(),
            });

            Ok(response)
        })
        .await
        .map_err(|_| {
            AppError::Agent(format!(
                "send_message_streaming timed out after {}s",
                SEND_MESSAGE_TIMEOUT.as_secs()
            ))
        })?
    }
}

impl Drop for ClaudeSession {
    fn drop(&mut self) {
        // Close stdin FIRST so claude sees EOF and exits on its own. We are
        // inside drop(), so struct fields have NOT dropped yet — without this
        // explicit close, the reap loop below would wait on a process that's
        // itself blocked reading more stdin.
        self.stdin.take();

        // Phase 1 — graceful: give claude a bounded window to exit on EOF.
        let reaped = poll_reap(&mut self.child, Duration::from_secs(5));

        // Phase 2 — forceful: if still alive, SIGKILL and reap again. Must use
        // `start_kill()` (synchronous — sends the signal now) rather than
        // `kill()`, whose returned future can't be awaited from Drop. Using
        // `let _ = kill()` here would silently leak the child instead of
        // killing it (the bug this restructure fixes).
        if !reaped {
            tracing::debug!("claude session didn't exit on EOF; force-killing");
            let _ = self.child.start_kill();
            poll_reap(&mut self.child, Duration::from_secs(2));
        }

        // Abort the stderr drain task. It usually ends on its own once the
        // child above is reaped/killed (stderr EOFs), but abort defensively
        // in case it's stuck on a read.
        if let Some(handle) = self.stderr_drain.take() {
            handle.abort();
        }
    }
}

/// Synchronously poll `try_wait()` in a tight loop until the child exits or
/// the deadline passes.
///
/// Drop can't `.await` tokio futures, so we use the non-blocking `try_wait()`
/// paced with short `thread::sleep`s — the pattern tokio itself recommends for
/// reaping from a sync context. Returns `true` if the child was reaped,
/// `false` on timeout or `try_wait` error.
fn poll_reap(child: &mut Child, window: Duration) -> bool {
    let deadline = Instant::now() + window;
    loop {
        match child.try_wait() {
            Ok(Some(_)) => return true, // reaped
            Ok(None) if Instant::now() < deadline => {
                std::thread::sleep(Duration::from_millis(50));
            }
            _ => return false, // wait errored or window expired
        }
    }
}

// ── Tests ──────────────────────────────────────────────────────────────────
//
// These are integration tests that spawn a real `claude` process and hit a
// live provider. They are `#[ignore]`d so `cargo test` stays offline; run them
// explicitly with:
//
//     cargo test --lib claude_session -- --ignored --nocapture
//
// The `--nocapture` lets you see the println/panic output live, which matters
// because each turn can take several seconds.

#[cfg(test)]
mod tests {
    use super::*;

    /// Test agent config — same as the commented-out block in `agents.rs`.
    ///
    /// Centralised here so both integration tests share one source of truth.
    /// Lower `effort` (e.g. `"low"`) if the tests feel slow.
    fn test_config() -> AgentConfig {
        AgentConfig {
            base_url: Some(String::from("https://api.deepseek.com/anthropic")),
            model: Some(String::from("deepseek-v4-pro[1m]")),
            auth_token: Some(String::from("sk-64a7220f24e241dc8139ba445cd634f0")),
            effort: Some(String::from("low")),
        }
    }

    fn test_path() -> PathBuf {
        std::env::current_dir().expect("cannot found current directory")
    }

    /// Smoke test: spawn one session, send one message, get a structured reply.
    ///
    /// Validates the full pipeline end-to-end: spawn (env vars, piped stdio)
    /// → write a user-turn JSON line to stdin → read the stream back → stop at
    /// the Result event → parse into AgentResponse.
    #[tokio::test]
    #[ignore = "calls a real provider; run with `cargo test -- --ignored`"]
    async fn test_session_single_turn() {
        let result = tokio::time::timeout(std::time::Duration::from_secs(120), async {
            let mut session = ClaudeSession::spawn(&test_path(), &test_config(), None)
                .expect("failed to spawn claude session");

            let response = session
                .send_message("reply with exactly: hello")
                .await
                .expect("send_message failed");

            assert!(!response.result.is_empty(), "result should not be empty");
            assert!(
                !response.is_error,
                "turn should not be an error, got: {:?}",
                response
            );
            assert!(
                response.result.to_lowercase().contains("hello"),
                "result should contain 'hello', got: {}",
                response.result
            );
        })
        .await;

        assert!(result.is_ok(), "Test timed out");

        // `session` drops here → Drop closes stdin → claude exits gracefully.
    }

    /// Smoke test: spawn one session, send one message, get a structured reply.
    ///
    /// Validates the full pipeline end-to-end: spawn (env vars, piped stdio)
    /// → write a user-turn JSON line to stdin → read the stream back → stop at
    /// the Result event → parse into AgentResponse.
    #[tokio::test]
    #[ignore = "calls a real provider; run with `cargo test -- --ignored`"]
    async fn test_session_current_directory() {
        let result = tokio::time::timeout(std::time::Duration::from_secs(120), async {

        let mut session = ClaudeSession::spawn(&test_path(), &test_config(), None)
            .expect("failed to spawn claude session");

        let response = session
            .send_message("is there a Cargo.toml in this directory, response with YES or NO only, No preamble")
            .await
            .expect("send_message failed");

        assert!(!response.result.is_empty(), "result should not be empty");
        assert!(
            !response.is_error,
            "turn should not be an error, got: {:?}",
            response
        );
        assert!(
            response.result.to_lowercase().contains("yes"),
            "result should contain 'YES', got: {}",
            response.result
        );
    }).await;
        assert!(result.is_ok(), "Test timed out");

        // `session` drops here → Drop closes stdin → claude exits gracefully.
    }

    /// The thesis test: prove the conversation context survives across turns
    /// on the SAME process, WITHOUT `--resume`.
    ///
    /// This is the whole reason `ClaudeSession` exists (see
    /// docs/researchs/agent-spawn.md). If this passes, the persistent-process
    /// approach is validated; if only single-turn passes but this fails,
    /// context is being lost somewhere.
    #[tokio::test]
    #[ignore = "calls a real provider; run with `cargo test -- --ignored`"]
    async fn test_session_retains_context_across_turns() {
        let result = tokio::time::timeout(std::time::Duration::from_secs(120), async {
            let mut session = ClaudeSession::spawn(&test_path(), &test_config(), None)
            .expect("failed to spawn claude session");

        // Turn 1 — plant a distinctive fact the model could only repeat by
        // remembering it (not by guessing from turn 2's question alone).
        let first = session
            .send_message("I'm telling you a secret codeword. The codeword is: ZUCCHINI. Reply with exactly: understood.")
            .await
            .expect("turn 1 (send_message) failed");
        assert!(!first.is_error, "turn 1 should not error, got: {:?}", first);

        // Turn 2 — recall it. Same process, same session, no --resume.
        let second = session
            .send_message("What is the secret codeword I just told you? Reply with only the codeword, nothing else.")
            .await
            .expect("turn 2 (send_message) failed");

        assert!(
            !second.is_error,
            "turn 2 should not error, got: {:?}",
            second
        );

        let lower = second.result.to_lowercase();
        assert!(
            lower.contains("zucchini"),
            "turn 2 should recall the codeword 'zucchini' — if this fails, \
             context is NOT surviving across turns. Got: {}",
            second.result
        );
}).await;

        assert!(result.is_ok(), "Test timed out");
        // `session` drops here → Drop closes stdin → claude exits gracefully.
    }

    /// Regression test for the silent-failure bug.
    ///
    /// When claude can't reach a provider (no auth / wrong base URL / etc.), it
    /// does NOT crash or hang — it completes the stream normally with a
    /// `result` event carrying `is_error: true` and a human-readable message
    /// (e.g. "Not logged in · Please run /login"). `send_message` must surface
    /// that faithfully as `Ok(AgentResponse { is_error: true, ... })` so the
    /// command layer can convert it to an `Err`. If this flag ever gets lost,
    /// auth failures look like "nothing happened" in the UI.
    ///
    /// Uses an empty `AgentConfig` (no token, no base_url) to force the auth
    /// failure without depending on the user's environment.
    #[tokio::test]
    #[ignore = "calls a real provider; run with `cargo test -- --ignored`"]
    async fn test_session_surfaces_provider_error() {
        let result = tokio::time::timeout(std::time::Duration::from_secs(60), async {
            let no_auth = AgentConfig {
                base_url: None,
                model: None,
                auth_token: None,
                effort: None,
            };
            let mut session = ClaudeSession::spawn(&test_path(), &no_auth, None)
                .expect("failed to spawn claude session");

            let response = session
                .send_message("reply with: hello")
                .await
                .expect("send_message should complete even on auth failure");

            // The turn must be flagged as an error — this is the contract the
            // command layer relies on to convert Ok → Err.
            assert!(
                response.is_error,
                "an unauthenticated turn must set is_error=true — \
                 if this fails, auth failures will be silently swallowed. \
                 Got: {:?}",
                response
            );
            assert!(
                !response.result.is_empty(),
                "the error turn should carry a human-readable result message"
            );
        })
        .await;

        assert!(result.is_ok(), "Test timed out");
    }

    /// The cross-restart thesis test (SPIKE for the resume architecture).
    ///
    /// Validates the ONE composition that was never proven in research:
    /// `--resume <id>` together with `--input-format stream-json`. If this
    /// passes, the whole resume-on-restart architecture is sound; if it fails,
    /// we ship persistence-for-display + in-process retention and drop resume.
    ///
    /// Flow: plant a codeword → capture `session_id` → DROP the session
    /// (process dies, simulating an app restart) → re-spawn with
    /// `--resume <session_id>` → ask for the codeword back → assert recall.
    #[tokio::test]
    #[ignore = "calls a real provider; run with `cargo test -- --ignored`"]
    async fn test_session_resume_after_restart() {
        let result = tokio::time::timeout(std::time::Duration::from_secs(180), async {
            // Phase 1 — plant a distinctive codeword in a fresh session.
            let mut session = ClaudeSession::spawn(&test_path(), &test_config(), None)
                .expect("failed to spawn claude session");
            let first = session
                .send_message(
                    "I'm telling you a secret codeword. The codeword is: EGGPLANT. \
                     Reply with exactly: understood.",
                )
                .await
                .expect("turn 1 (send_message) failed");
            assert!(!first.is_error, "turn 1 should not error, got: {:?}", first);

            let session_id = first.session_id.clone();
            assert!(
                !session_id.is_empty(),
                "session_id must be present to resume — got empty. \
                 If the provider doesn't emit a session_id, resume is impossible."
            );

            // Phase 2 — DROP the session. This kills the claude process and
            // simulates an app restart: no live process retains context now.
            drop(session);

            // Phase 3 — re-spawn a NEW process with --resume <session_id>.
            // If the composition works, claude restores the prior conversation
            // from its own session store and remembers the codeword.
            let mut resumed = ClaudeSession::spawn(&test_path(), &test_config(), Some(&session_id))
                .expect("failed to re-spawn claude session with --resume");

            let second = resumed
                .send_message(
                    "What is the secret codeword I told you earlier? \
                     Reply with only the codeword, nothing else.",
                )
                .await
                .expect("turn 2 (send_message on resumed session) failed");

            assert!(
                !second.is_error,
                "resumed turn should not error, got: {:?}",
                second
            );

            let lower = second.result.to_lowercase();
            assert!(
                lower.contains("eggplant"),
                "resumed session should recall the codeword 'eggplant' — if this fails, \
                 --resume + --input-format stream-json does NOT restore context together. \
                 Got: {}",
                second.result
            );
        })
        .await;

        assert!(result.is_ok(), "Test timed out");
    }
}
