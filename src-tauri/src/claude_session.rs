use crate::agents::{apply_agent_config, parse_stream_line, AgentResponse, ResponseAccumulator};
use crate::config::AgentConfig;
use crate::error::AppError;
use std::path::PathBuf;
use std::process::Stdio;
use std::time::{Duration, Instant};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin, ChildStdout, Command};

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
}

impl ClaudeSession {
    pub fn spawn(project_path: &PathBuf, agent_config: &AgentConfig) -> Result<Self, AppError> {
        let mut cmd = Command::new("claude");
        cmd.args([
            "--input-format",
            "stream-json",
            "--output-format",
            "stream-json",
            "--verbose",
        ]);
        // Reuse the same env-var wiring as call_agents so the single-shot and
        // persistent paths can't drift out of sync.
        apply_agent_config(&mut cmd, agent_config);
        cmd.current_dir(project_path);
        cmd.stdin(Stdio::piped());
        cmd.stdout(Stdio::piped());
        cmd.stderr(Stdio::null()); // don't inherit — keep stdout clean for parsing

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

        Ok(Self {
            child,
            stdin: Some(stdin),
            stdout: BufReader::new(stdout),
        })
    }

    /// Send one user turn and block until claude emits its `result` event.
    ///
    /// Returns the aggregated `AgentResponse` for the whole turn. Output is
    /// buffered until the turn completes — this is the batch (non-streaming)
    /// variant; a future version can emit each `StreamEvent` to the frontend
    /// as it arrives.
    pub async fn send_message(&mut self, text: &str) -> Result<AgentResponse, AppError> {
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
    }
}

impl Drop for ClaudeSession {
    fn drop(&mut self) {
        // Close stdin FIRST so claude sees EOF and exits on its own. We are
        // inside drop(), so struct fields have NOT dropped yet — without this
        // explicit close, wait() below would deadlock on a process that's
        // itself blocked reading more stdin.
        self.stdin.take();

        // Give claude a bounded window to exit gracefully on EOF, then
        // force-kill so a hung child can never wedge the host app on drop.
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            match self.child.try_wait() {
                Ok(Some(_)) => return, // reaped cleanly
                Ok(None) if Instant::now() < deadline => {
                    std::thread::sleep(Duration::from_millis(50));
                }
                _ => {
                    // Timed out or wait errored — escalate to kill, then reap.
                    let _ = self.child.kill();
                    let _ = self.child.wait();
                    return;
                }
            }
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
            let mut session = ClaudeSession::spawn(&test_path(), &test_config())
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

        let mut session = ClaudeSession::spawn(&test_path(), &test_config())
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
            let mut session = ClaudeSession::spawn(&test_path(), &test_config())
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
}
