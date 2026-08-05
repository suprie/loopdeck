use crate::agents::{
    apply_agent_config, parse_ask_user_questions, parse_exit_plan, parse_stream_line,
    AgentResponse, AskUserQuestionSpec, ClaudeEvent, ContentBlock, ControlRequestBody,
    ControlResponsePayload, ResponseAccumulator, StreamEvent, TokenBudget,
};
use crate::config::AgentConfig;
use crate::conversation::Attachment;
use crate::error::AppError;
use crate::harness::HarnessAdapter;
use crate::permission::{Decision, PermissionPolicy};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::{Arc, Mutex as StdMutex};
use std::time::{Duration, Instant};
use tauri::ipc::Channel;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin, ChildStdout, Command};
use tokio::sync::oneshot;
use tokio::task::JoinHandle;

/// Per-`read_line` idle timeout. Bounds how long we'll wait for the next line
/// of stdout from claude with no activity — a stuck peer (no output for this
/// long) fails loudly instead of hanging the caller.
///
/// Bounds *active reading* only, not the parked phase: when we're parked on
/// `rx` awaiting an approval/answer we're not awaiting `read_line`, so this
/// timeout never fires mid-park. The parked phase is bounded separately by
/// [`TURN_DEADLINE`].
const READ_LINE_TIMEOUT: Duration = Duration::from_secs(180);

/// How long `write_set_permission_mode` waits for the matching
/// `control_response` before treating the mode switch as failed.
///
/// Much shorter than [`READ_LINE_TIMEOUT`]: this is a lightweight control
/// message exchanged between turns (no model generation involved), sent and
/// answered synchronously by the CLI, so a real reply arrives near-instantly.
/// A long wait here would just delay surfacing a genuinely stuck process.
const SET_PERMISSION_MODE_TIMEOUT: Duration = Duration::from_secs(10);

/// Absolute (turn-start-relative) deadline bounding how long a single turn
/// may stay **parked** on a manual approval / `AskUserQuestion`.
///
/// This is the backstop the 2026-07-23 "approvals/questions wait indefinitely"
/// decision explicitly deferred. That decision removed the old 10-minute
/// `PARKED_SLOT_TIMEOUT`: the visibility layer (toast / `ProjectCard` pill /
/// detail-view callout, reconciled on app launch + window focus +
/// visibilitychange) makes a *briefly*-unattended prompt unlikely to be
/// missed, so the 10-minute auto-deny was a footgun (it silently denied
/// prompts the user never saw). But "forgotten is unlikely" is not "forgotten
/// is impossible": a prompt parked while the laptop is closed overnight, the
/// app is backgrounded, or the user simply stepped away for hours still holds
/// the per-project turn lock, blocking every later turn on that project (and
/// stalling an unattended autonomous loop). This deadline releases it.
///
/// **Why 30 minutes** (3× the removed 10-minute footgun): long enough that
/// stepping away for a meeting or lunch never trips it, short enough that a
/// genuinely-abandoned turn frees the lock in bounded time. It is a single
/// constant — the contract, not a runtime knob (cf. `logging::MAX_LOG_FILES`)
/// — so it is trivial to tune if the backstop proves too tight or too loose.
///
/// **Scope — parked phase only.** A turn that is actively producing output is
/// bounded by [`READ_LINE_TIMEOUT`] (per-read inactivity) and the
/// [`ResponseAccumulator`] byte/block caps, not by this deadline. The read
/// loop is suspended while parked, so this deadline is the only thing
/// bounding that gap. A turn that streamed actively for longer than
/// `TURN_DEADLINE` and *then* parked will expire on that first park — correct,
/// since nobody is watching a turn that long.
///
/// On expiry the park site writes the graceful `interrupt` control_request
/// (the live process survives — the next send resumes the conversation, just
/// as a manual Stop does) and returns `Err`, releasing the lock. See
/// [`park_until`].
const TURN_DEADLINE: Duration = Duration::from_secs(30 * 60);

/// The tool name Claude uses when it wants to ask the user a clarifying
/// question. We intercept this *before* the auto-permission policy so the
/// question can be surfaced to the human instead of auto-allowed with empty
/// answers.
const ASK_USER_QUESTION_TOOL: &str = "AskUserQuestion";

/// The tool name Claude uses to leave `plan` permission mode once it has
/// finished planning. Intercepted with the same precedence as
/// `ASK_USER_QUESTION_TOOL` — before the destructive floor / manual-approval /
/// auto-policy arms — since it's neither a mutating tool call nor a floor
/// candidate, just a request for the human to review the plan.
const EXIT_PLAN_MODE_TOOL: &str = "ExitPlanMode";

/// Outcome of racing a stdout read against an interrupt in the streaming loop.
///
/// Defined so the `select!` arms can be matched cleanly without re-running the
/// receiver branch on every iteration.
enum ReadOutcome {
    /// A line was read (n bytes, 0 = EOF).
    ReadResult(usize),
    /// The next read should proceed (interrupt sender dropped w/o firing).
    Read,
    /// The user interrupted the turn — end it gracefully.
    Interrupted,
}

/// Outcome of a single bounded line read — see [`read_bounded_line`].
///
/// Replaces the `usize` returned by `read_line` with three explicit states so
/// the read loops can distinguish "got a line", "EOF", and "discarded an
/// oversized line" without overloading a byte count.
enum BoundedRead {
    /// A complete line (including its trailing newline) is in the buffer.
    Line,
    /// EOF was reached before any bytes were read for this line.
    Eof,
    /// The line exceeded the byte cap and was discarded in full.
    Oversized,
}

/// Outcome of parking a turn on a oneshot receiver (a pending manual approval
/// or `AskUserQuestion`) bounded by an absolute turn deadline.
///
/// Separates the *decision* (this pure enum, returned by [`park_until`]) from
/// the *action* the park site takes on each variant (write a control_response,
/// return a cancellation error, or write an interrupt + return an expiry
/// error) — so the deadline logic is unit-testable without a live session.
#[derive(Debug)]
enum ParkOutcome<T> {
    /// The receiver resolved with an answer/verdict — write the control_response.
    Answered(T),
    /// The sender was dropped without sending — the turn was cancelled (the
    /// session is being dropped / a Stop landed elsewhere). The park site
    /// returns an error so the read loop stops.
    Cancelled,
    /// The absolute turn deadline elapsed with no answer — the prompt was
    /// abandoned. The park site writes the graceful `interrupt` control_request
    /// and returns an error, releasing the per-project lock.
    Expired,
}

/// Race a parked oneshot receiver against the turn's absolute deadline.
///
/// Returns [`ParkOutcome::Answered`] when the receiver resolves,
/// [`ParkOutcome::Cancelled`] when the sender is dropped, or
/// [`ParkOutcome::Expired`] when `deadline` elapses first. `deadline` is
/// absolute (captured at turn start as a `tokio::time::Instant`), so the same
/// deadline bounds every park within a turn — the total parked time across
/// multiple parks is bounded by [`TURN_DEADLINE`], not re-reset per park.
///
/// Cancel-safe: dropping the future mid-await only drops the
/// `oneshot::Receiver`.
async fn park_until<T>(deadline: tokio::time::Instant, rx: oneshot::Receiver<T>) -> ParkOutcome<T> {
    match tokio::time::timeout_at(deadline, rx).await {
        Ok(Ok(value)) => ParkOutcome::Answered(value),
        Ok(Err(_)) => ParkOutcome::Cancelled,
        Err(_) => ParkOutcome::Expired,
    }
}

/// The error surfaced when a parked prompt exceeds [`TURN_DEADLINE`].
///
/// Shared by the AskUserQuestion, manual-approval, and plan-approval park
/// sites so the "abandoned, lock released" message stays consistent. The live
/// session survives (the park site already wrote the `interrupt`
/// control_request), so the caller can re-send to resume the conversation.
///
/// `AppError::TurnParked` (not `Agent`) — an unattended caller (the
/// `prd-run-queue` executor) needs to tell this apart from a genuine turn
/// failure without parsing the message text. `pending_detail` names what was
/// left unanswered (the question text, the tool + input, or the plan) so an
/// unattended caller has something concrete to show in a morning report;
/// capped at a few hundred chars since a `Write`/`Edit` input or a long plan
/// could otherwise blow up the message.
/// Build the `stream-json` user-turn object written to the CLI's stdin.
///
/// The CLI accepts the Anthropic Messages API content-block shape verbatim, so
/// an attached image is a plain `image` block with an inline base64 source —
/// no CLI-specific wrapper, and no writing the file to disk for the agent to
/// `Read` back.
///
/// Images are emitted **before** the text block. That ordering is Anthropic's
/// documented recommendation for image+text prompts, and it matches how the
/// user composes: they paste a screenshot, then type the question about it.
///
/// Factored out (rather than inlined at each `write_all`) because the
/// streaming and non-streaming send paths each build this independently, and
/// a divergence between them would be invisible until one path silently
/// dropped attachments.
fn user_turn_json(text: &str, attachments: &[Attachment]) -> serde_json::Value {
    let mut content: Vec<serde_json::Value> = attachments
        .iter()
        .map(|a| {
            serde_json::json!({
                "type": "image",
                "source": {
                    "type": "base64",
                    "media_type": a.media_type,
                    "data": a.data,
                }
            })
        })
        .collect();
    // "Here, look at this" with no words is a legitimate message, and the API
    // rejects an empty text block — so drop the block entirely rather than
    // sending a blank one. Only when there is nothing else to say: a turn with
    // neither text nor images keeps the empty text block, preserving the
    // pre-attachment behaviour for that (composer-blocked) case.
    if !text.is_empty() || content.is_empty() {
        content.push(serde_json::json!({ "type": "text", "text": text }));
    }

    serde_json::json!({
        "type": "user",
        "message": {
            "role": "user",
            "content": content,
        }
    })
}

fn turn_deadline_expired_error(pending_detail: &str, questions_json: Option<String>) -> AppError {
    const MAX_DETAIL_CHARS: usize = 200;
    let detail: String = if pending_detail.chars().count() > MAX_DETAIL_CHARS {
        format!(
            "{}…",
            pending_detail
                .chars()
                .take(MAX_DETAIL_CHARS)
                .collect::<String>()
        )
    } else {
        pending_detail.to_string()
    };
    AppError::TurnParked {
        detail: format!(
            "turn deadline ({} minutes) elapsed while parked for a decision — the \
             project lock was released; the live session survived, so re-send to \
             resume. Pending: {detail}",
            TURN_DEADLINE.as_secs() / 60
        ),
        parked_questions_json: questions_json,
    }
}

/// Read one newline-terminated line from `reader` into `buf`, bounding peak
/// memory to `max_bytes` (PRD FR4).
///
/// Drop-in for `AsyncBufReadExt::read_line` that never buffers more than
/// `max_bytes` for a single line: once a line exceeds the cap we drain it
/// (discarding bytes up to and including its newline) and report
/// [`BoundedRead::Oversized`], so a pathologically large NDJSON line from the
/// `claude` CLI (a tool result embedding a huge file, a runaway `result`
/// event) can't exhaust memory. On a normal line the full line — newline
/// included — lands in `buf`.
///
/// **Cancel-safe:** the only `.await` is `fill_buf`, which leaves the reader
/// consistent if the future is dropped mid-call (the internal buffer is
/// untouched until the synchronous `consume`). Safe to race in a `select!`
/// against an interrupt / a timeout, as the streaming loop does — a dropped
/// partial line is simply re-read on the next turn.
///
/// Generic over `AsyncBufRead + Unpin` so it's unit-testable with a `Cursor`.
async fn read_bounded_line<R>(
    reader: &mut R,
    buf: &mut Vec<u8>,
    max_bytes: usize,
) -> Result<BoundedRead, std::io::Error>
where
    R: tokio::io::AsyncBufRead + Unpin,
{
    buf.clear();
    let mut oversized = false;
    loop {
        // `fill_buf` borrows `reader` for the lifetime of `data`; we lift
        // everything we need (lengths, whether to append) into locals below so
        // the borrow ends before the `&mut self` `consume`.
        let data = reader.fill_buf().await?;
        if data.is_empty() {
            // EOF. A pending oversized line with no terminating newline still
            // counts as oversized; otherwise an empty buffer is a clean EOF.
            return Ok(if oversized {
                BoundedRead::Oversized
            } else if buf.is_empty() {
                BoundedRead::Eof
            } else {
                BoundedRead::Line
            });
        }
        match data.iter().position(|&b| b == b'\n') {
            Some(nl) => {
                // The line ends within this chunk.
                let consume = nl + 1;
                if oversized || buf.len() + consume > max_bytes {
                    // Whole line is oversized — discard it (consume through the
                    // newline) and report. `consume` is a copy, so `data`'s
                    // borrow has ended by the time we mutably borrow `reader`.
                    reader.consume(consume);
                    buf.clear();
                    return Ok(BoundedRead::Oversized);
                }
                // Last use of `data` — the borrow ends here, so `consume` below
                // can take `&mut reader`.
                buf.extend_from_slice(&data[..consume]);
                reader.consume(consume);
                return Ok(BoundedRead::Line);
            }
            None => {
                // No newline yet. Capture the length (a copy) so the `data`
                // borrow ends before `consume`.
                let len = data.len();
                let over = oversized || buf.len() + len > max_bytes;
                if over {
                    oversized = true;
                } else {
                    buf.extend_from_slice(data);
                }
                reader.consume(len);
            }
        }
    }
}

/// The user's answer to a single `AskUserQuestion` question.
///
/// The user selects one or more canned option labels (radio/checkbox) and may
/// supply free-text when the "Other…" affordance is used. The backend doesn't
/// interpret these — it just forwards them to Claude as the tool's
/// `updatedInput.answers` value.
#[derive(Debug, Clone)]
pub struct QuestionAnswer {
    /// The selected option label(s). One entry for single-select (radio),
    /// zero or more for multi-select (checkbox). Empty if the user typed a
    /// free-text "Other…" answer only.
    pub labels: Vec<String>,
    /// Free-text from the "Other…" input, when used. Empty otherwise.
    pub other_text: Option<String>,
}

impl QuestionAnswer {
    /// Collapse the answer into the single string Claude expects for a
    /// single-select question (`answers[q] = "<label>"`). Falls back to the
    /// first label, or the other-text, when the user only typed free text.
    pub fn as_single(&self) -> String {
        if let Some(label) = self.labels.first() {
            return label.clone();
        }
        self.other_text.clone().unwrap_or_default()
    }

    /// Collapse the answer into the value shape for a multi-select question:
    /// `answers[q] = ["label1", "label2"]`, with any free-text appended.
    pub fn as_multi(&self) -> Vec<String> {
        let mut all = self.labels.clone();
        if let Some(other) = self.other_text.clone().filter(|t| !t.trim().is_empty()) {
            all.push(other);
        }
        all
    }
}

/// All answers to one `AskUserQuestion` call, keyed by question text.
///
/// The key is the question text because that's what Claude's protocol uses to
/// correlate answers back to questions (`answers: { "<question>": ... }`).
pub type QuestionAnswers = HashMap<String, QuestionAnswer>;

/// A pending `AskUserQuestion` request parked in the read loop, awaiting the
/// user's answers.
///
/// Carries both the oneshot `Sender` (which `agent_answer_question` pops to
/// deliver the answers and wake the parked turn) AND the request payload
/// (`request_id`, the parsed `questions`). The payload is stored here — not
/// just emitted as a `ClaudeEvent` on the streaming channel — so that a freshly
/// mounted frontend (after the user navigated away and back mid-question) can
/// reconcile the card via `agent_pending_question` even though the original
/// channel event was lost when its AgentPanel unmounted.
pub struct PendingQuestion {
    /// Correlates the answer with the originating control_request.
    pub request_id: String,
    /// The structured questions to surface to the user.
    pub questions: Vec<AskUserQuestionSpec>,
    /// Resolves the parked turn. `None` after `agent_answer_question` consumed it.
    pub sender: Option<oneshot::Sender<QuestionAnswers>>,
}

/// Shared wrapper around `Option<PendingQuestion>` — see `PendingQuestion` for
/// why this carries the payload alongside the sender. Same `Arc<StdMutex<…>>`
/// discipline as the other slots: short critical sections, never held across
/// an `.await`.
pub type QuestionSlot = Arc<StdMutex<Option<PendingQuestion>>>;

/// A pending manual tool-approval request parked in the read loop, awaiting
/// the user's Allow / Deny verdict. The manual-approval counterpart of
/// `PendingQuestion`.
///
/// Carries the oneshot `Sender` (which `agent_answer_permission` pops to
/// deliver the verdict) AND the request payload (`request_id`, `tool_name`,
/// `input`) so a freshly-mounted frontend can reconcile the card via
/// `agent_pending_permission` after navigation.
pub struct PendingPermission {
    /// Correlates the verdict with the originating control_request.
    pub request_id: String,
    /// Tool name, e.g. "Bash", "Edit".
    pub tool_name: String,
    /// Raw tool input as a JSON string.
    pub input: String,
    /// Resolves the parked turn. `None` after `agent_answer_permission` consumed it.
    pub sender: Option<oneshot::Sender<Decision>>,
}

/// Shared wrapper around `Option<PendingPermission>`. See `PendingPermission`.
pub type PermissionSlot = Arc<StdMutex<Option<PendingPermission>>>;

/// A pending `ExitPlanMode` request parked in the read loop, awaiting the
/// user's Approve / Reject verdict. The plan-approval counterpart of
/// `PendingPermission` — kept as its own slot (rather than reusing
/// `PermissionSlot`) so a plan approval and an unrelated tool approval can
/// never collide, even though both carry the same `Decision` payload.
///
/// Carries the oneshot `Sender` (which `agent_answer_plan` pops to deliver the
/// verdict) AND the request payload (`request_id`, `plan`) so a
/// freshly-mounted frontend can reconcile the card via `agent_pending_plan`
/// after navigation.
pub struct PendingPlan {
    /// Correlates the verdict with the originating control_request.
    pub request_id: String,
    /// The plan text Claude wrote, as passed to `ExitPlanMode`.
    pub plan: String,
    /// Resolves the parked turn. `None` after `agent_answer_plan` consumed it.
    pub sender: Option<oneshot::Sender<Decision>>,
}

/// Shared wrapper around `Option<PendingPlan>`. See `PendingPlan`.
pub type PlanSlot = Arc<StdMutex<Option<PendingPlan>>>;

/// The three per-project "answer this control_request" oneshot slots that
/// `answer_control_request` may park a turn on, bundled into one struct so it
/// and `send_message[_streaming]` don't each carry three separate slot
/// parameters (clippy's `too_many_arguments`). Distinct from `InterruptSlot`,
/// which serves an unrelated purpose (the Stop button) and is threaded
/// separately.
pub struct ParkSlots<'a> {
    pub question: &'a QuestionSlot,
    pub permission: &'a PermissionSlot,
    pub plan: &'a PlanSlot,
}

/// Shared, single-slot bridge between the read loop and the
/// `agent_interrupt` IPC command.
///
/// When a turn starts, the read loop stores a fresh oneshot `Sender` here and
/// `select!`s between the next stdout line and the receiver. `agent_interrupt`
/// pops the sender and fires it — the loop wakes, writes the graceful
/// `interrupt` control_request to stdin, and ends the turn with a sentinel
/// error. The live process and its context survive (unlike `agent_reset`,
/// which kills both), so the next send resumes the same conversation.
///
/// Same `Arc<StdMutex<…>>` shape as `QuestionSlot`/`PermissionSlot` for
/// consistency: trivially short critical sections, no `.await` held under the
/// lock.
pub type InterruptSlot = Arc<StdMutex<Option<oneshot::Sender<()>>>>;

/// A long-lived `claude --input-format stream-json` process.
///
/// Unlike `agents::call_agents` (which spawns a fresh process per prompt and
/// relies on `--resume`), a `ClaudeSession` spawns once and keeps stdin open.
/// Conversation context lives inside the process itself, so follow-up turns
/// are just another line written to stdin — no `--resume`, no respawn.
pub struct ClaudeSession {
    // `Option` so `Drop` can `take()` the child and hand ownership to a
    // fire-and-forget `spawn_blocking` reap task — it must not touch `self`
    // (which is being torn down) or block the dropping thread.
    child: Option<Child>,
    // `Option` so `Drop` can close stdin explicitly before reaping the child;
    // also doubles as a "still usable" flag.
    stdin: Option<ChildStdin>,
    stdout: BufReader<ChildStdout>,
    // Background task that drains claude's stderr so a verbose child can't
    // fill its OS pipe buffer and deadlock. `Option` so `Drop` can abort it.
    // Ends naturally on stderr EOF when the child exits.
    stderr_drain: Option<JoinHandle<()>>,
    // The permission policy consulted for each `control_request`. Pure logic
    // (see `permission.rs`) — the decision is written back to stdin as a
    // `control_response` and surfaced via `ClaudeEvent::PermissionRequest`.
    policy: PermissionPolicy,
    // Whether we last **confirmed** the live process is running in `plan`
    // permission mode (via a `set_permission_mode` control_request — see
    // `ensure_permission_mode`). Tracked here, not just inferred from the
    // per-call `plan_mode` flag, so a turn that leaves plan mode without ever
    // approving `ExitPlanMode` (Stop, error, or the model just answering
    // directly) is still defensively reset to `default` on the next send.
    //
    // `None` means "unknown, not merely unchanged" — set whenever a mode
    // switch is attempted but its outcome couldn't be confirmed (the write
    // failed, or the CLI's `control_response` never arrived / errored). The
    // write may still have reached and been applied by the CLI even though
    // we couldn't observe that; treating it as "still whatever it was
    // before" would let a later call take the `Some(x) == plan_mode`
    // shortcut against state that no longer matches reality, sending the
    // user's message under the wrong permission mode without even trying to
    // fix it. `None` forces the next call to always re-send and revalidate,
    // regardless of which mode it requests.
    plan_mode_active: Option<bool>,
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
        policy: PermissionPolicy,
    ) -> Result<Self, AppError> {
        // Resolve `claude` to an absolute, vetted path ourselves rather than
        // handing a bare name to the OS PATH search — defeats the cwd-hijack
        // vector (a `.`/empty PATH entry resolving against `project_path`, which
        // is a user-selected, untrusted directory) and pins the binary for the
        // process lifetime. See `binary`.
        let claude_path = crate::binary::claude()
            .map_err(|e| AppError::Agent(format!("failed to resolve claude binary: {e}")))?;
        let mut cmd = Command::new(claude_path);
        cmd.args([
            "--input-format",
            "stream-json",
            "--output-format",
            "stream-json",
            "--verbose",
            // Route permission prompts over stdio as `control_request` lines
            // so Selasar can answer them via `control_response`. Without this,
            // un-approved tools would prompt the (absent) TTY and stall.
            "--permission-prompt-tool",
            "stdio",
        ]);
        if let Some(id) = resume_session_id {
            cmd.args(["--resume", id]);
        }
        // `default` permission mode: every tool call that doesn't match an
        // allow rule in `.claude/settings.json` emits a `control_request`
        // that Selasar decides (floor → manual approval → auto-policy).
        // This is the honest single-source-of-truth: nothing is silently
        // auto-approved by Claude itself. Earlier iterations used
        // `acceptEdits`, which auto-approves Edit/Write/NotebookEdit inside
        // Claude and made the `MANUAL_APPROVAL_TOOLS` entries for those tools
        // dead code. See docs/PRD-trust-boundary-hardening.md FR1.
        cmd.args(["--permission-mode", "default"]);

        if let Some(model) = agent_config.model.clone() {
            cmd.args(["--model", &model]);
        }

        // Load user + project + local settings so the curated
        // `permissions.allow` list written by `skills::setup_hooks` (and any
        // user allow rules) short-circuit the control protocol for known-safe
        // commands, while everything else still routes through `policy`.
        cmd.args(["--setting-sources", "user,project,local"]);
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

        // Log the resolved spawn config at INFO so a gateway error (e.g. 529
        // overloaded, 401 bad token, wrong base URL) is diagnosable without
        // guessing what model/endpoint the CLI actually used. The
        // AgentConfig Debug impl redacts the auth token. The binary path is the
        // vetted, absolute resolution from `binary::claude` — logging it makes a
        // hostile `$PATH` (a different claude than the user expects) visible.
        tracing::info!(
            "spawning claude in {} | binary={} | model={} | base_url={} | auth_token={} | effort={} | resume={}",
            project_path.display(),
            claude_path.display(),
            agent_config.model.as_deref().unwrap_or("<cli default>"),
            agent_config.base_url.as_deref().unwrap_or("<cli default>"),
            if agent_config.auth_token.is_some() { "set" } else { "<none>" },
            agent_config.effort.as_deref().unwrap_or("<none>"),
            resume_session_id.unwrap_or("none"),
        );

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
            child: Some(child),
            stdin: Some(stdin),
            stdout: BufReader::new(stdout),
            stderr_drain: Some(stderr_drain),
            policy,
            // A freshly-spawned process starts in `default` permission mode
            // (the CLI's own default, never overridden by our spawn args) —
            // a real confirmed fact, not an assumption, so `Some(false)` is
            // correct here rather than `None`.
            plan_mode_active: Some(false),
        })
    }

    /// Decide and answer a single `control_request` from Claude.
    ///
    /// Called inline from both read loops whenever a `control_request` event is
    /// parsed. This is what unblocks the agent: Claude blocks on each
    /// permission request until we write the matching `control_response`, and
    /// it can't run the tool (or emit the next event) until then — so requests
    /// arrive serially. Answering in-arrival-order, keyed by `request_id`, is
    /// therefore inherently race-free: there is never more than one outstanding
    /// request at a time, so no queue or request map is needed (PRD R3/R4).
    ///
    /// **Five arms, evaluated in order:**
    /// 0. `ExitPlanMode` is intercepted first — Claude wants to leave `plan`
    ///    permission mode. The read loop parks on `plan_slot`'s oneshot
    ///    receiver until the frontend resolves it via `agent_answer_plan`,
    ///    then writes a plain `allow`/`deny` (no `updatedInput`). Autonomous
    ///    projects skip the park (see `answer_plan_approval`).
    /// 1. `AskUserQuestion` is intercepted next. The read loop parks on
    ///    `question_slot`'s oneshot receiver until the frontend resolves it via
    ///    `agent_answer_question`, then writes back an `allow` with
    ///    `updatedInput` carrying the user's answers.
    /// 2. The destructive floor (`policy.decide`) is consulted next. A floor
    ///    match (`rm -rf`, `sudo`, …) is denied immediately, no prompt — the
    ///    floor is a hard rule, not a preference.
    /// 3. If the floor clears but the tool is in `MANUAL_APPROVAL_TOOLS`
    ///    (Bash/Edit/Write/NotebookEdit/WebFetch), the read loop parks on
    ///    `permission_slot`'s oneshot until the user picks Allow/Deny via
    ///    `agent_answer_permission`, then writes the matching response.
    /// 4. Otherwise (read-only tools, unknown tools) the synchronous
    ///    allow-by-default policy answers immediately.
    ///
    /// `channel` is `Some` only on the streaming path, where each decision is
    /// also surfaced to the UI as a `ClaudeEvent` (PermissionRequest for
    /// permissions — `decision: "pending"` while parked, then the resolved
    /// allow/deny; AskUserQuestion for questions; PlanApproval for
    /// ExitPlanMode). The non-streaming path logs the decision only.
    ///
    /// Every permission decision is logged at `info` (tool, input, behavior,
    /// reason) per PRD R6 — both for debugging and so the demo can narrate it.
    async fn answer_control_request(
        &mut self,
        request_id: &str,
        request: &ControlRequestBody,
        channel: Option<&Channel<ClaudeEvent>>,
        slots: &ParkSlots<'_>,
        turn_deadline: tokio::time::Instant,
    ) -> Result<(), AppError> {
        // ── Arm 0: ExitPlanMode — surface the plan for human approval. ──
        // Checked first, same precedence rationale as AskUserQuestion below:
        // it's neither a mutating tool nor a floor candidate, just a request
        // to leave plan mode.
        if request.tool_name == EXIT_PLAN_MODE_TOOL {
            return self
                .answer_plan_approval(request_id, request, channel, slots.plan, turn_deadline)
                .await;
        }

        // ── Arm 1: AskUserQuestion — defer to the human via its dedicated path. ──
        if request.tool_name == ASK_USER_QUESTION_TOOL {
            return self
                .answer_ask_user_question(
                    request_id,
                    request,
                    channel,
                    slots.question,
                    turn_deadline,
                )
                .await;
        }

        // ── Arm 2: destructive floor — hard-deny, no prompt. ──
        // `policy.decide` runs the floor first; if it returns Deny, the request
        // matched a destructive pattern and there's nothing to ask the user —
        // we surface the floor reason and move on. The manual-approval arm
        // below only runs when the floor *cleared*.
        let input_str = request.input.to_string();
        let policy_decision = self.policy.decide(&request.tool_name, &request.input);

        // ── Arm 3: manual approval — park until the user decides. ──
        // Only for mutating/executing tools (see MANUAL_APPROVAL_TOOLS) AND
        // only when the floor didn't already deny. We check `policy_decision`
        // rather than calling `check_destructive_floor` directly so any future
        // hard-deny added to the policy is also exempt from prompting.
        //
        // **Autonomous mode skips this arm** (`is_autonomous()`): the project
        // is opted into unattended operation, so floor-clearing mutating tools
        // fall through to arm 4 and are auto-allowed. The floor in arm 2 still
        // ran first, so `rm -rf` / force-push / `curl|sh` / `sudo` are denied
        // regardless — this arm only governs whether *safe* mutations park.
        if policy_decision == Decision::Allow
            && !self.policy.is_autonomous()
            && crate::permission::requires_manual_approval(&request.tool_name)
        {
            return self
                .answer_manual_permission(
                    request_id,
                    request,
                    &input_str,
                    channel,
                    slots.permission,
                    turn_deadline,
                )
                .await;
        }

        // ── Arm 4: synchronous auto-policy (read-only tools, floor denies,
        //         autonomous-mode auto-allow for mutating tools). ──
        let decision = policy_decision;
        // Under autonomous mode, distinguish "the policy auto-allowed this"
        // from a read-only auto-allow or a user's manual Allow. The UI uses
        // this discriminator to tag auto-approved calls in the transcript so
        // the morning review can see what the agent did without asking.
        let autonomous_auto_allow = self.policy.is_autonomous() && decision == Decision::Allow;
        tracing::info!(
            tool = %request.tool_name,
            input = %input_str,
            behavior = decision.behavior(),
            reason = decision.reason(),
            autonomous = autonomous_auto_allow,
            "permission decision",
        );

        // Write the control_response back to stdin, matched by request_id.
        let stdin = self
            .stdin
            .as_mut()
            .ok_or_else(|| AppError::Agent("session stdin already closed".into()))?;
        let payload = match &decision {
            Decision::Allow => ControlResponsePayload::allow(request_id),
            Decision::Deny(reason) => ControlResponsePayload::deny(request_id, reason),
        };
        let mut line = payload.to_json().to_string();
        line.push('\n');
        tracing::debug!(target: "loopdeck::claude_wire", "→ control_response: {}", line.trim());
        stdin
            .write_all(line.as_bytes())
            .await
            .map_err(|e| AppError::Agent(format!("control_response write failed: {}", e)))?;
        stdin
            .flush()
            .await
            .map_err(|e| AppError::Agent(format!("control_response flush failed: {}", e)))?;

        // Streaming-only: narrate the decision to the UI.
        if let Some(channel) = channel {
            let _ = channel.send(ClaudeEvent::PermissionRequest {
                request_id: request_id.to_string(),
                tool_name: request.tool_name.clone(),
                input: input_str,
                // `"allow"` for read-only / user-confirmed; `"auto-allow"` for
                // autonomous-mode auto-approvals; `"deny"` for floor denies.
                decision: if autonomous_auto_allow {
                    String::from("auto-allow")
                } else {
                    decision.behavior().to_string()
                },
                reason: decision.reason().to_string(),
            });
        }

        Ok(())
    }

    /// Handle an `ExitPlanMode` control request by parking the read loop until
    /// the user approves or rejects the plan via `agent_answer_plan`.
    ///
    /// Mirrors `answer_manual_permission`'s park-then-write shape (same
    /// `Decision` payload, plain allow/deny — no `updatedInput`), but keyed on
    /// the dedicated `plan_slot` so a plan approval can never collide with an
    /// unrelated tool approval, and with its own autonomous short-circuit.
    ///
    /// **Autonomous projects skip the park.** The entire point of autonomous
    /// mode is unattended operation (see `PermissionPolicy::is_autonomous`);
    /// stopping for a plan review would just reintroduce the manual gate that
    /// mode exists to remove. The plan is still narrated via
    /// `ClaudeEvent::PlanApproval { decision: "auto-allow" }` so the morning
    /// review can see what the agent decided to do.
    ///
    /// On `Decision::Allow`, the CLI's own `ExitPlanMode` handler reverts the
    /// process's permission mode back to whatever preceded `plan` — we only
    /// need to clear `plan_mode_active` so `ensure_permission_mode` doesn't
    /// defensively re-send a mode that already reverted. On `Decision::Deny`,
    /// the model is expected to revise the plan and call `ExitPlanMode` again
    /// within the same turn — since the slot is single-use (overwritten on
    /// each call), that second call parks here again exactly like the first.
    ///
    /// On the non-streaming path (no channel) there's no UI surface to answer
    /// from, so we reclaim the sender and deny instead of parking forever —
    /// same stance as `answer_ask_user_question` / `answer_manual_permission`.
    async fn answer_plan_approval(
        &mut self,
        request_id: &str,
        request: &ControlRequestBody,
        channel: Option<&Channel<ClaudeEvent>>,
        plan_slot: &PlanSlot,
        turn_deadline: tokio::time::Instant,
    ) -> Result<(), AppError> {
        let plan = parse_exit_plan(&request.input).unwrap_or_default();

        if self.policy.is_autonomous() {
            // Mark unknown until `write_allow` actually succeeds — the CLI's
            // ExitPlanMode handler only reverts out of plan mode once it
            // *receives* our allow, so a write/flush failure here means the
            // approval was never delivered and the live process may still be
            // in plan mode. Recording `Some(false)` before that would claim
            // a confirmed state we don't actually have.
            self.plan_mode_active = None;
            tracing::info!(
                request_id = %request_id,
                "ExitPlanMode auto-allowed (autonomous mode)",
            );
            self.write_allow(request_id).await?;
            // Delivered — the CLI's own ExitPlanMode handler reverts out of
            // plan mode internally now that it has received the allow.
            self.plan_mode_active = Some(false);
            if let Some(channel) = channel {
                let _ = channel.send(ClaudeEvent::PlanApproval {
                    request_id: request_id.to_string(),
                    plan,
                    decision: String::from("auto-allow"),
                    reason: String::new(),
                });
            }
            return Ok(());
        }

        // Surface the pending plan to the UI BEFORE parking, mirroring the
        // manual-approval arm's "pending" narration.
        if let Some(channel) = channel {
            let _ = channel.send(ClaudeEvent::PlanApproval {
                request_id: request_id.to_string(),
                plan: plan.clone(),
                decision: String::from("pending"),
                reason: String::new(),
            });
        }

        let (tx, rx) = oneshot::channel::<Decision>();
        {
            let mut guard = plan_slot.lock().map_err(|_| AppError::LockError)?;
            *guard = Some(PendingPlan {
                request_id: request_id.to_string(),
                plan: plan.clone(),
                sender: Some(tx),
            });
        }

        tracing::info!(
            request_id = %request_id,
            "ExitPlanMode received — parking for user approval",
        );

        if channel.is_none() {
            let _ = plan_slot.lock().ok().and_then(|mut g| g.take());
            tracing::warn!("ExitPlanMode on non-streaming path — no way to surface; denying");
            return self
                .write_deny(request_id, "plan approval is not supported on this path")
                .await;
        }

        // Park until the user decides, bounded by the absolute turn deadline —
        // same backstop and rationale as the question/permission arms.
        let decision = match park_until(turn_deadline, rx).await {
            ParkOutcome::Answered(d) => d,
            ParkOutcome::Cancelled => {
                return Err(AppError::Agent(
                    "plan approval cancelled — answer channel closed".into(),
                ));
            }
            ParkOutcome::Expired => {
                self.write_interrupt_control_request().await;
                return Err(turn_deadline_expired_error(
                    &format!("plan approval — {plan}"),
                    None,
                ));
            }
        };

        tracing::info!(
            request_id = %request_id,
            behavior = decision.behavior(),
            reason = decision.reason(),
            "plan approval decided",
        );

        match &decision {
            Decision::Allow => {
                // Mark unknown until the write actually succeeds — same
                // reasoning as the autonomous-allow arm above: the CLI only
                // reverts out of plan mode once it receives this allow, so a
                // write/flush failure means the approval was never delivered.
                self.plan_mode_active = None;
                self.write_allow(request_id).await?;
                // Delivered — confirmed.
                self.plan_mode_active = Some(false);
            }
            Decision::Deny(reason) => {
                self.write_deny(request_id, reason).await?;
            }
        }

        if let Some(channel) = channel {
            let _ = channel.send(ClaudeEvent::PlanApproval {
                request_id: request_id.to_string(),
                plan,
                decision: decision.behavior().to_string(),
                reason: decision.reason().to_string(),
            });
        }

        Ok(())
    }

    /// Handle an `AskUserQuestion` control request by parking the read loop
    /// until the user answers via `agent_answer_question`.
    ///
    /// Flow:
    /// 1. Parse the structured questions from `request.input`.
    /// 2. Create a oneshot channel; store its `Sender` in `question_slot`
    ///    (shared with `AppState.pending_answers`), where the
    ///    `agent_answer_question` command will find and resolve it.
    /// 3. Emit `ClaudeEvent::AskUserQuestion` so the UI can render the prompt.
    /// 4. `.await` the receiver, bounded by the absolute `turn_deadline`
    ///    ([`TURN_DEADLINE`]). A user who answers resolves it; a user who never
    ///    answers eventually expires the deadline, which writes the graceful
    ///    `interrupt` control_request and aborts the turn (releasing the lock).
    /// 5. On answer, build `updatedInput = original input + { answers }` and
    ///    write back an `allow`-with-`updatedInput` control_response.
    ///
    /// If the questions array is malformed (zero questions parse), we deny
    /// rather than silently allow with empty input — the model should retry
    /// with a well-formed question.
    async fn answer_ask_user_question(
        &mut self,
        request_id: &str,
        request: &ControlRequestBody,
        channel: Option<&Channel<ClaudeEvent>>,
        question_slot: &QuestionSlot,
        turn_deadline: tokio::time::Instant,
    ) -> Result<(), AppError> {
        let questions = parse_ask_user_questions(&request.input);
        if questions.is_empty() {
            // Malformed request — don't allow with empty answers; deny so the
            // model can retry with a well-formed AskUserQuestion call.
            tracing::warn!(
                request_id = %request_id,
                "AskUserQuestion arrived with no parseable questions — denying"
            );
            return self
                .write_deny(
                    request_id,
                    "AskUserQuestion request had no parseable questions",
                )
                .await;
        }

        // Create the oneshot and stash the sender AND the payload in the
        // shared slot. `agent_answer_question` pops the sender to deliver
        // answers; `agent_pending_question` reads the payload (without
        // consuming the sender) so a freshly-mounted frontend can reconcile
        // the card after navigation.
        let (tx, rx) = oneshot::channel::<QuestionAnswers>();
        {
            let mut guard = question_slot.lock().map_err(|_| AppError::LockError)?;
            // There is at most one outstanding request at a time (Claude
            // blocks on each), so any pre-existing entry here would be a
            // dropped/leaked one — overwrite it.
            *guard = Some(PendingQuestion {
                request_id: request_id.to_string(),
                questions: questions.clone(),
                sender: Some(tx),
            });
        }

        tracing::info!(
            request_id = %request_id,
            questions = questions.len(),
            "AskUserQuestion received — parking for user answer"
        );

        // Surface the question to the UI. On the non-streaming path there's no
        // channel, so the question can't be answered — log and deny.
        if let Some(channel) = channel {
            let _ = channel.send(ClaudeEvent::AskUserQuestion {
                request_id: request_id.to_string(),
                tool_name: request.tool_name.clone(),
                questions: questions.clone(),
            });
        } else {
            // Non-streaming path has no UI surface for an answer. Reclaim the
            // sender and deny so we don't park forever.
            let _ = question_slot.lock().ok().and_then(|mut g| g.take());
            tracing::warn!("AskUserQuestion on non-streaming path — no way to surface; denying");
            return self
                .write_deny(request_id, "AskUserQuestion is not supported on this path")
                .await;
        }

        // Park until the user answers, bounded by the absolute turn deadline.
        // The visibility layer (toast / ProjectCard pill / detail-view
        // callout, reconciled on app launch + window focus + visibilitychange)
        // surfaces the pending question so it isn't missed, and the normal
        // escapes are to answer it or hit Stop (which cancels the turn and
        // drops the sender). The deadline is the last-resort backstop for a
        // genuinely-abandoned prompt (laptop closed overnight, app
        // backgrounded) — see `TURN_DEADLINE`. `rx` is a `oneshot::Receiver`,
        // so `park_until` is cancel-safe.
        let answers = match park_until(turn_deadline, rx).await {
            ParkOutcome::Answered(answers) => answers,
            ParkOutcome::Cancelled => {
                // Sender dropped without sending — the turn was cancelled
                // (e.g. session dropped or Stop was hit). Return an error so
                // the caller stops reading.
                return Err(AppError::Agent(
                    "AskUserQuestion cancelled — answer channel closed".into(),
                ));
            }
            ParkOutcome::Expired => {
                // Deadline elapsed with no answer — the prompt was abandoned.
                // Write the graceful interrupt so the live claude process
                // aborts cleanly (it survives — the next send resumes the
                // conversation), then surface an error to release the
                // per-project lock. See `TURN_DEADLINE`.
                self.write_interrupt_control_request().await;
                let detail = format!(
                    "AskUserQuestion — {}",
                    questions
                        .iter()
                        .map(|q| q.question.as_str())
                        .collect::<Vec<_>>()
                        .join("; ")
                );
                let questions_json = serde_json::to_string(&questions).ok();
                return Err(turn_deadline_expired_error(&detail, questions_json));
            }
        };

        tracing::info!(
            request_id = %request_id,
            answered = answers.len(),
            "AskUserQuestion answered — writing control_response"
        );

        // Build updatedInput: clone the original input object, then attach the
        // `answers` map. Single-select → string; multi-select → array. Keys are
        // the question text. This is the shape Claude's protocol expects.
        let mut updated_input = request.input.clone();
        let answers_json: HashMap<String, serde_json::Value> = questions
            .iter()
            .filter_map(|q| {
                answers.get(&q.question).map(|a| {
                    let value = if q.multi_select {
                        serde_json::Value::Array(
                            a.as_multi()
                                .into_iter()
                                .map(serde_json::Value::String)
                                .collect(),
                        )
                    } else {
                        serde_json::Value::String(a.as_single())
                    };
                    (q.question.clone(), value)
                })
            })
            .collect();
        if let Some(obj) = updated_input.as_object_mut() {
            obj.insert(
                "answers".to_string(),
                serde_json::to_value(&answers_json).unwrap_or(serde_json::Value::Null),
            );
        }

        self.write_allow_with_updated_input(request_id, updated_input)
            .await
    }

    /// Handle a `can_use_tool` request for a mutating/executing tool by parking
    /// the read loop until the user picks Allow or Deny.
    ///
    /// Mirrors `answer_ask_user_question`'s structure — same single-slot
    /// oneshot bridge, same park-then-write flow — but carries a `Decision`
    /// instead of structured answers, and writes a plain allow/deny
    /// `control_response` (no `updatedInput`).
    ///
    /// Flow:
    /// 1. Emit `ClaudeEvent::PermissionRequest { decision: "pending" }` so the
    ///    UI renders the Allow/Deny card (and a pending marker in the streaming
    ///    bubble) — this happens BEFORE the control_response is written, unlike
    ///    the auto-allow/deny narration.
    /// 2. Create a oneshot; stash the `Sender` in `permission_slot` (shared
    ///    with `AppState.pending_permissions`), where the
    ///    `agent_answer_permission` command will pop it to deliver the verdict.
    /// 3. `.await` the receiver, bounded by the absolute `turn_deadline`
    ///    ([`TURN_DEADLINE`]). A user who decides resolves it; a user who
    ///    never decides eventually expires the deadline, which writes the
    ///    graceful `interrupt` control_request and aborts the turn (releasing
    ///    the lock).
    /// 4. On `Decision::Allow`, write `allow`; on `Decision::Deny(r)`, write
    ///    `deny(r)`. Then emit a second `PermissionRequest` carrying the
    ///    resolved decision so the UI's pending marker becomes ✓/✗.
    ///
    /// On the non-streaming path (no channel) the user has no UI surface to
    /// answer from, so we reclaim the sender and deny instead of parking
    /// forever — same stance as `answer_ask_user_question`.
    async fn answer_manual_permission(
        &mut self,
        request_id: &str,
        request: &ControlRequestBody,
        input_str: &str,
        channel: Option<&Channel<ClaudeEvent>>,
        permission_slot: &PermissionSlot,
        turn_deadline: tokio::time::Instant,
    ) -> Result<(), AppError> {
        // Surface the pending request to the UI BEFORE parking. The streaming
        // bubble renders a pending marker so the user sees what's being asked.
        if let Some(channel) = channel {
            let _ = channel.send(ClaudeEvent::PermissionRequest {
                request_id: request_id.to_string(),
                tool_name: request.tool_name.clone(),
                input: input_str.to_string(),
                decision: String::from("pending"),
                reason: String::new(),
            });
        }

        // Stash the sender AND the payload for `agent_answer_permission` to
        // resolve (and for `agent_pending_permission` to surface to a
        // freshly-mounted frontend after navigation). Same single-slot
        // invariant as AskUserQuestion: at most one outstanding request at a
        // time (Claude blocks on each).
        let (tx, rx) = oneshot::channel::<Decision>();
        {
            let mut guard = permission_slot.lock().map_err(|_| AppError::LockError)?;
            *guard = Some(PendingPermission {
                request_id: request_id.to_string(),
                tool_name: request.tool_name.clone(),
                input: input_str.to_string(),
                sender: Some(tx),
            });
        }

        tracing::info!(
            request_id = %request_id,
            tool = %request.tool_name,
            input = %input_str,
            "manual approval required — parking for user decision",
        );

        // Non-streaming path can't surface the prompt — reclaim + deny rather
        // than park forever (matches the question arm).
        if channel.is_none() {
            let _ = permission_slot.lock().ok().and_then(|mut g| g.take());
            tracing::warn!(
                "manual-approval tool {tool} on non-streaming path — no way to surface; denying",
                tool = request.tool_name,
            );
            let reason = "this tool requires approval, but no UI is available on this path";
            self.write_deny(request_id, reason).await?;
            return Ok(());
        }

        // Park until the user decides, bounded by the absolute turn deadline.
        // The visibility layer surfaces pending approvals (toast / ProjectCard
        // pill / detail-view callout, reconciled on app launch + window focus +
        // visibilitychange) so the user can't miss one, and the normal escapes
        // are Allow/Deny or Stop (which cancels the turn and drops the sender).
        // The deadline is the last-resort backstop for a genuinely-abandoned
        // approval — see `TURN_DEADLINE`. Borrow of `self` is released at the
        // `.await`, so the writes below can re-take `&mut self`.
        let decision = match park_until(turn_deadline, rx).await {
            ParkOutcome::Answered(d) => d,
            ParkOutcome::Cancelled => {
                // Sender dropped without sending — the turn was cancelled.
                // Return an error so the caller stops reading (mirrors the
                // AskUserQuestion cancellation path).
                return Err(AppError::Agent(
                    "manual approval cancelled — answer channel closed".into(),
                ));
            }
            ParkOutcome::Expired => {
                // Deadline elapsed with no decision — the approval was
                // abandoned. Write the graceful interrupt so the live claude
                // process aborts cleanly (it survives — the next send resumes
                // the conversation), then surface an error to release the
                // per-project lock. See `TURN_DEADLINE`.
                self.write_interrupt_control_request().await;
                let detail = format!("{} — {input_str}", request.tool_name);
                return Err(turn_deadline_expired_error(&detail, None));
            }
        };

        tracing::info!(
            request_id = %request_id,
            tool = %request.tool_name,
            behavior = decision.behavior(),
            reason = decision.reason(),
            "manual approval decided",
        );

        // Write the control_response back to stdin.
        let stdin = self
            .stdin
            .as_mut()
            .ok_or_else(|| AppError::Agent("session stdin already closed".into()))?;
        let payload = match &decision {
            Decision::Allow => ControlResponsePayload::allow(request_id),
            Decision::Deny(reason) => ControlResponsePayload::deny(request_id, reason),
        };
        let mut line = payload.to_json().to_string();
        line.push('\n');
        tracing::debug!(target: "loopdeck::claude_wire", "→ control_response: {}", line.trim());
        stdin
            .write_all(line.as_bytes())
            .await
            .map_err(|e| AppError::Agent(format!("control_response write failed: {}", e)))?;
        stdin
            .flush()
            .await
            .map_err(|e| AppError::Agent(format!("control_response flush failed: {}", e)))?;

        // Emit the resolved decision so the UI's pending marker becomes ✓/✗.
        if let Some(channel) = channel {
            let _ = channel.send(ClaudeEvent::PermissionRequest {
                request_id: request_id.to_string(),
                tool_name: request.tool_name.clone(),
                input: input_str.to_string(),
                decision: decision.behavior().to_string(),
                reason: decision.reason().to_string(),
            });
        }

        Ok(())
    }

    /// Write an allow-with-updated-input control_response to stdin. Factored
    /// out so the question arm stays readable; mirrors the inline writes used
    /// by the permission arm.
    async fn write_allow_with_updated_input(
        &mut self,
        request_id: &str,
        updated_input: serde_json::Value,
    ) -> Result<(), AppError> {
        let stdin = self
            .stdin
            .as_mut()
            .ok_or_else(|| AppError::Agent("session stdin already closed".into()))?;
        let payload = ControlResponsePayload::allow_with_updated_input(request_id, updated_input);
        let mut line = payload.to_json().to_string();
        line.push('\n');
        tracing::debug!(target: "loopdeck::claude_wire", "→ control_response: {}", line.trim());
        stdin
            .write_all(line.as_bytes())
            .await
            .map_err(|e| AppError::Agent(format!("control_response write failed: {}", e)))?;
        stdin
            .flush()
            .await
            .map_err(|e| AppError::Agent(format!("control_response flush failed: {}", e)))?;
        Ok(())
    }

    /// Write a deny control_response to stdin. Factored out for the question
    /// arm's error paths (malformed request / unsupported path).
    async fn write_deny(&mut self, request_id: &str, reason: &str) -> Result<(), AppError> {
        let stdin = self
            .stdin
            .as_mut()
            .ok_or_else(|| AppError::Agent("session stdin already closed".into()))?;
        let payload = ControlResponsePayload::deny(request_id, reason);
        let mut line = payload.to_json().to_string();
        line.push('\n');
        tracing::debug!(target: "loopdeck::claude_wire", "→ control_response: {}", line.trim());
        stdin
            .write_all(line.as_bytes())
            .await
            .map_err(|e| AppError::Agent(format!("control_response write failed: {}", e)))?;
        stdin
            .flush()
            .await
            .map_err(|e| AppError::Agent(format!("control_response flush failed: {}", e)))?;
        Ok(())
    }

    /// Write a plain allow control_response to stdin. Factored out for the
    /// plan-approval arm, which (unlike the question/permission arms) never
    /// needs `updatedInput` — mirrors `write_deny`.
    async fn write_allow(&mut self, request_id: &str) -> Result<(), AppError> {
        let stdin = self
            .stdin
            .as_mut()
            .ok_or_else(|| AppError::Agent("session stdin already closed".into()))?;
        let payload = ControlResponsePayload::allow(request_id);
        let mut line = payload.to_json().to_string();
        line.push('\n');
        tracing::debug!(target: "loopdeck::claude_wire", "→ control_response: {}", line.trim());
        stdin
            .write_all(line.as_bytes())
            .await
            .map_err(|e| AppError::Agent(format!("control_response write failed: {}", e)))?;
        stdin
            .flush()
            .await
            .map_err(|e| AppError::Agent(format!("control_response flush failed: {}", e)))?;
        Ok(())
    }

    /// Write a graceful `interrupt` control_request to the live process.
    ///
    /// Called from the streaming read loop when it observes the interrupt
    /// oneshot fire. This asks the CLI to stop the current turn gracefully
    /// (the agent finishes any in-flight tool and returns a result) rather
    /// than us killing the process — so the live session and its context
    /// survive, and the next send resumes the same conversation. The CLI
    /// emits a normal `result` event in response; we don't wait for it here
    /// (the loop returns immediately after, surfacing the interrupt as a
    /// terminal `Result` + sentinel error).
    ///
    /// Best-effort: if stdin is closed (process already dying), the write is
    /// logged but not fatal — the turn ends either way.
    async fn write_interrupt_control_request(&mut self) {
        let Some(stdin) = self.stdin.as_mut() else {
            tracing::debug!("interrupt: stdin already closed — nothing to write");
            return;
        };
        // The interrupt control_request has no request body beyond the subtype.
        // No request_id is needed because the CLI doesn't send a matching
        // control_response for interrupts — it just stops the turn.
        let line = r#"{"type":"control_request","request":{"subtype":"interrupt"}}"#;
        let mut full = String::from(line);
        full.push('\n');
        tracing::info!(target: "loopdeck::claude_wire", "→ interrupt control_request: {line}");
        if let Err(e) = stdin.write_all(full.as_bytes()).await {
            tracing::warn!("interrupt control_request write failed: {e}");
            return;
        }
        if let Err(e) = stdin.flush().await {
            tracing::warn!("interrupt control_request flush failed: {e}");
        }
    }

    /// Enter or leave the CLI's `plan` permission mode for the upcoming turn.
    ///
    /// `plan_mode` mirrors Claude Code's own shift-tab toggle: when true, the
    /// CLI restricts the model to read-only tools plus `ExitPlanMode` (which
    /// `answer_plan_approval` intercepts) for the duration of plan mode —
    /// entirely client-enforced, not something Selasar's permission layer has
    /// to police.
    ///
    /// Tracks the last mode we asked for in `plan_mode_active` so this is a
    /// no-op when nothing changed, and — importantly — so a turn that leaves
    /// plan mode without ever calling `ExitPlanMode` (Stop, an error, or the
    /// model just answering directly without proposing anything) is
    /// defensively reset to `default` the next time a normal (non-plan) send
    /// goes out. Without this, a stuck `plan_mode_active` would silently keep
    /// every later turn read-only-restricted with no visible cause. An
    /// approved `ExitPlanMode` clears the flag itself (the CLI reverts
    /// internally), so the common path never re-sends here.
    ///
    /// **Failure is fatal, by design.** If the write, flush, or response
    /// validation fails we return `Err` — the caller propagates this as a
    /// turn failure rather than sending the user's message anyway. The
    /// alternative (log-and-continue, updating the flag regardless) would let
    /// a turn silently run under `default` permissions — full edit access —
    /// right after the user asked for a read-only plan-first review, which is
    /// exactly the kind of unattended mutation this app's whole
    /// confirm-before-mutate posture exists to prevent.
    ///
    /// **On failure, the cached state is marked unknown (`None`), not left
    /// unchanged.** `write_set_permission_mode`'s write can succeed and the
    /// CLI can apply the new mode even when we then fail to *confirm* it (a
    /// lost/errored/timed-out `control_response`) — so "unchanged" would
    /// mean "possibly wrong". Marking it unknown forces the *next* call,
    /// whichever mode it requests, to skip the equality shortcut and always
    /// re-send + revalidate, rather than trusting stale cached state that may
    /// no longer match what the live process is actually doing.
    async fn ensure_permission_mode(&mut self, plan_mode: bool) -> Result<(), AppError> {
        if self.plan_mode_active == Some(plan_mode) {
            return Ok(());
        }
        self.plan_mode_active = None;
        self.write_set_permission_mode(if plan_mode { "plan" } else { "default" })
            .await?;
        self.plan_mode_active = Some(plan_mode);
        Ok(())
    }

    /// Write a `set_permission_mode` control_request to the live process and
    /// wait for the matching `control_response`.
    ///
    /// A successful *write* is not the same as a successful mode switch: the
    /// installed CLI has explicit rejection paths for `set_permission_mode`
    /// (e.g. "onSetPermissionMode callback not registered"), so we read
    /// stdout until we see the `control_response` carrying our `request_id`
    /// and only return `Ok` for `subtype: "success"` — a `subtype: "error"`
    /// (or a timeout) is surfaced as a real error, which `ensure_permission_mode`
    /// propagates instead of flipping `plan_mode_active` on an unconfirmed
    /// change.
    ///
    /// This is safe to read here (outside the normal per-turn read loop)
    /// because no turn is in flight at this call site — it always runs
    /// before the user-turn line is written (see `ensure_permission_mode`'s
    /// caller), so the CLI cannot be emitting assistant/result events in this
    /// window. Any unrelated line encountered while waiting (the one-time
    /// `system` init line on a fresh process, or a stray non-matching
    /// response) is skipped rather than treated as the answer, bounded overall
    /// by [`SET_PERMISSION_MODE_TIMEOUT`].
    async fn write_set_permission_mode(&mut self, mode: &str) -> Result<(), AppError> {
        let request_id = format!("set-mode-{}", uuid::Uuid::new_v4());
        {
            let stdin = self
                .stdin
                .as_mut()
                .ok_or_else(|| AppError::Agent("session stdin already closed".into()))?;
            let line = serde_json::json!({
                "type": "control_request",
                "request_id": request_id,
                "request": { "subtype": "set_permission_mode", "mode": mode },
            });
            let mut full = line.to_string();
            full.push('\n');
            tracing::info!(target: "loopdeck::claude_wire", "→ set_permission_mode control_request: {full}");
            stdin
                .write_all(full.as_bytes())
                .await
                .map_err(|e| AppError::Agent(format!("set_permission_mode write failed: {e}")))?;
            stdin
                .flush()
                .await
                .map_err(|e| AppError::Agent(format!("set_permission_mode flush failed: {e}")))?;
        }

        let deadline = tokio::time::Instant::now() + SET_PERMISSION_MODE_TIMEOUT;
        let mut line_bytes: Vec<u8> = Vec::new();
        loop {
            let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
            if remaining.is_zero() {
                return Err(AppError::Agent(format!(
                    "set_permission_mode({mode}): no control_response from claude within {}s",
                    SET_PERMISSION_MODE_TIMEOUT.as_secs()
                )));
            }
            let read = tokio::time::timeout(
                remaining,
                read_bounded_line(
                    &mut self.stdout,
                    &mut line_bytes,
                    crate::limits::STREAM_LINE_MAX_BYTES,
                ),
            )
            .await
            .map_err(|_| {
                AppError::Agent(format!(
                    "set_permission_mode({mode}): no control_response from claude within {}s",
                    SET_PERMISSION_MODE_TIMEOUT.as_secs()
                ))
            })?
            .map_err(|e| {
                AppError::Agent(format!("set_permission_mode({mode}): read failed: {e}"))
            })?;

            match read {
                BoundedRead::Eof => {
                    return Err(AppError::Agent(format!(
                        "claude closed stdout while waiting for the set_permission_mode({mode}) response"
                    )));
                }
                // Discard and keep waiting — an oversized line can't be our
                // small control_response.
                BoundedRead::Oversized => continue,
                BoundedRead::Line => {}
            }

            let line = String::from_utf8_lossy(&line_bytes);
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }
            tracing::debug!(target: "loopdeck::claude_wire", "← claude: {trimmed}");
            match parse_stream_line(trimmed) {
                Some(StreamEvent::ControlResponse { response })
                    if response.request_id == request_id =>
                {
                    return if response.subtype == "success" {
                        Ok(())
                    } else {
                        Err(AppError::Agent(format!(
                            "claude rejected set_permission_mode({mode}): {}",
                            response.error.unwrap_or_else(|| "no reason given".into())
                        )))
                    };
                }
                // Not our response (the one-time `system` init line on a
                // fresh process, or an unrelated echo) — keep waiting,
                // bounded by the deadline above.
                _ => continue,
            }
        }
    }

    /// Send one user turn and block until claude emits its `result` event.
    ///
    /// Returns the aggregated `AgentResponse` for the whole turn. Output is
    /// buffered until the turn completes — this is the batch (non-streaming)
    /// variant; a future version can emit each `StreamEvent` to the frontend
    /// as it arrives.
    ///
    /// `question_slot` is the shared bridge for `AskUserQuestion`: if Claude
    /// asks a question mid-turn, the read loop parks on it until the
    /// `agent_answer_question` command resolves it. (On this non-streaming
    /// path there's no channel to surface the question, so a question is
    /// denied — but the slot is still threaded for consistency.)
    pub async fn send_message(
        &mut self,
        text: &str,
        attachments: &[Attachment],
        slots: &ParkSlots<'_>,
        _interrupt_slot: &InterruptSlot,
    ) -> Result<AgentResponse, AppError> {
        // Active reading is bounded per-`read_line` by READ_LINE_TIMEOUT (a
        // stuck peer fails loudly). The parked phase (awaiting an
        // approval/answer, which excludes the read timeout) is bounded by an
        // absolute, turn-start-relative backstop — TURN_DEADLINE — so a
        // genuinely-abandoned prompt releases the per-project lock instead of
        // holding it forever. The deadline is captured here once and threaded
        // into `answer_control_request` below.
        let turn_deadline = tokio::time::Instant::now() + TURN_DEADLINE;
        let result = async {
            // ---- write the user turn to stdin ----
            let stdin = self
                .stdin
                .as_mut()
                .ok_or_else(|| AppError::Agent("session stdin already closed".into()))?;

            let input = user_turn_json(text, attachments);
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
            let mut line_bytes: Vec<u8> = Vec::new();
            loop {
                // Bounded line read (PRD FR4): a single NDJSON line is capped
                // at STREAM_LINE_MAX_BYTES so a pathologically large line (a
                // tool result embedding a huge file) can't exhaust memory. The
                // per-read timeout catches a stuck peer (no stdout for
                // READ_LINE_TIMEOUT seconds) without bounding the parked phase.
                let read = match tokio::time::timeout(
                    READ_LINE_TIMEOUT,
                    read_bounded_line(
                        &mut self.stdout,
                        &mut line_bytes,
                        crate::limits::STREAM_LINE_MAX_BYTES,
                    ),
                )
                .await
                {
                    Ok(Ok(read)) => read,
                    Ok(Err(e)) => {
                        return Err(AppError::Agent(format!("read failed: {}", e)));
                    }
                    Err(_) => {
                        return Err(AppError::Agent(format!(
                            "claude produced no stdout for {}s — assuming stuck",
                            READ_LINE_TIMEOUT.as_secs()
                        )));
                    }
                };

                match read {
                    BoundedRead::Eof => {
                        return Err(AppError::Agent(
                            "claude closed stdout before sending a result event".into(),
                        ));
                    }
                    BoundedRead::Oversized => {
                        return Err(AppError::Limit(format!(
                            "claude stream line exceeded {} bytes — discarded",
                            crate::limits::STREAM_LINE_MAX_BYTES
                        )));
                    }
                    BoundedRead::Line => {}
                }

                let line = String::from_utf8_lossy(&line_bytes);
                let trimmed = line.trim();
                if trimmed.is_empty() {
                    continue;
                }
                tracing::debug!(target: "loopdeck::claude_wire", "← claude: {trimmed}");
                let is_result = match parse_stream_line(trimmed) {
                    // Intercept permission requests and answer them inline —
                    // never let them reach the accumulator (which would no-op
                    // them) without first writing the control_response, or
                    // Claude blocks forever. See `answer_control_request`.
                    Some(StreamEvent::ControlRequest {
                        request_id,
                        request,
                    }) => {
                        self.answer_control_request(
                            &request_id,
                            &request,
                            None,
                            slots,
                            turn_deadline,
                        )
                        .await?;
                        false
                    }
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
        .await;

        // Defensive: clear any stale parking entries left by an errored /
        // timed-out turn. Best-effort; locks can't deadlock (no await).
        // Done unconditionally on every exit (success or error) so a phantom
        // prompt can't survive into the next turn.
        let _ = slots.question.lock().ok().and_then(|mut g| g.take());
        let _ = slots.permission.lock().ok().and_then(|mut g| g.take());
        let _ = slots.plan.lock().ok().and_then(|mut g| g.take());

        result
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
    #[allow(clippy::too_many_arguments)]
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
        // Active reading is bounded per-`read_line` by READ_LINE_TIMEOUT (in
        // the select below); the parked phase — which that select excludes —
        // is bounded by the absolute TURN_DEADLINE backstop captured here and
        // threaded into `answer_control_request`. See `send_message` for the
        // full rationale.
        let turn_deadline = tokio::time::Instant::now() + TURN_DEADLINE;
        let result = async {
            // Enter/leave `plan` permission mode BEFORE writing the user turn
            // line below — the CLI processes stdin strictly in order, so the
            // mode is guaranteed to apply to this turn's generation. A
            // failure here fails the whole turn (via `?`) rather than
            // sending the user's message under an unconfirmed permission
            // mode — see `ensure_permission_mode`.
            self.ensure_permission_mode(plan_mode).await?;

            // ---- write the user turn to stdin ----
            let stdin = self
                .stdin
                .as_mut()
                .ok_or_else(|| AppError::Agent("session stdin already closed".into()))?;

            let input = user_turn_json(text, attachments);
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
            // Install the interrupt sender for this turn: the read loop
            // `select!`s between the next stdout line and the receiver, so
            // `agent_interrupt` can end the turn gracefully (writing the
            // `interrupt` control_request) without taking the per-project turn
            // lock (which this loop holds). Stashed into the shared slot —
            // same pattern as the question/permission slots.
            let (interrupt_tx, mut interrupt_rx) = oneshot::channel::<()>();
            {
                let mut guard = interrupt_slot.lock().map_err(|_| AppError::LockError)?;
                // Overwrite any sender left by a prior turn that ended without
                // clearing (defensive — the turn-end paths clear it below).
                *guard = Some(interrupt_tx);
            }

            let mut acc = ResponseAccumulator::new();
            let mut line_bytes: Vec<u8> = Vec::new();
            loop {
                // Race the next stdout line against an interrupt. On
                // interrupt we write the graceful control_request and end the
                // turn — the live process survives, so the next send resumes
                // the same conversation. (During a parked approval/question
                // the loop is off the read, so an interrupt there won't be
                // observed this turn; the user should deny the card instead.)
                //
                // The read is bounded (PRD FR4): `read_bounded_line` caps a
                // single NDJSON line at STREAM_LINE_MAX_BYTES. It is cancel-safe
                // (only `fill_buf` is awaited), so dropping it mid-line on
                // interrupt/timeout leaves the reader consistent for the next
                // turn — no data lost or duplicated.
                let read_or_interrupt = tokio::select! {
                    biased; // interrupt wins the race when both are ready
                    res = &mut interrupt_rx => match res {
                        Ok(()) => ReadOutcome::Interrupted,
                        Err(_) => ReadOutcome::Read, // sender dropped w/o firing — keep reading
                    },
                    // Per-read idle timeout: catches a stuck peer (no stdout for
                    // READ_LINE_TIMEOUT seconds) without bounding the parked
                    // phase — when parked on a control_request we're awaiting
                    // the oneshot in answer_control_request, not this select.
                    _ = tokio::time::sleep(READ_LINE_TIMEOUT) => {
                        return Err(AppError::Agent(format!(
                            "claude produced no stdout for {}s — assuming stuck",
                            READ_LINE_TIMEOUT.as_secs()
                        )));
                    },
                    read = read_bounded_line(
                        &mut self.stdout,
                        &mut line_bytes,
                        crate::limits::STREAM_LINE_MAX_BYTES,
                    ) => match read {
                        Ok(BoundedRead::Line) => ReadOutcome::ReadResult(1),
                        Ok(BoundedRead::Eof) => ReadOutcome::ReadResult(0),
                        Ok(BoundedRead::Oversized) => {
                            return Err(AppError::Limit(format!(
                                "claude stream line exceeded {} bytes — discarded",
                                crate::limits::STREAM_LINE_MAX_BYTES
                            )))
                        }
                        Err(e) => return Err(AppError::Agent(format!("read failed: {}", e))),
                    },
                };
                // Reclaim the interrupt sender when we leave the loop so a
                // stale turn-end doesn't leave a dangling sender for the next
                // turn. (Best-effort: the lock can't deadlock here — no await.)
                if matches!(read_or_interrupt, ReadOutcome::Interrupted) {
                    self.write_interrupt_control_request().await;
                    // Clear the slot so the next turn starts fresh.
                    let _ = interrupt_slot.lock().ok().and_then(|mut g| g.take());
                    // Best-effort: flush whatever the turn accumulated so the
                    // partial transcript is recorded. The terminal result
                    // event never arrives on interrupt, so we synthesize a
                    // minimal response + emit Result so the frontend
                    // reconciles (clears streaming bubble, reloads transcript).
                    let partial = acc.clone_partial();
                    let _ = channel.send(ClaudeEvent::Result {
                        text: partial.text.clone(),
                        thinking: partial.thinking.clone(),
                        result: format!("(interrupted) {}", partial.result),
                        usage: partial.usage.clone(),
                        is_error: false,
                        duration_ms: partial.duration_ms,
                        session_id: partial.session_id.clone(),
                    });
                    return Err(AppError::Agent("turn interrupted by user".into()));
                }
                let n = match read_or_interrupt {
                    ReadOutcome::ReadResult(n) => n,
                    _ => 0, // unreachable — handled above
                };

                if n == 0 {
                    return Err(AppError::Agent(
                        "claude closed stdout before sending a result event".into(),
                    ));
                }

                let line = String::from_utf8_lossy(&line_bytes);
                let trimmed = line.trim();
                if trimmed.is_empty() {
                    continue;
                }
                tracing::debug!(target: "loopdeck::claude_wire", "← claude: {trimmed}");
                let stream_event = match parse_stream_line(trimmed) {
                    Some(ev) => ev,
                    None => continue, // unrecognized line — skip
                };

                // Claude includes usage on each assistant message, before the
                // terminal result event. Count those model-call deltas against
                // the shared phase budget so an agentic turn is interrupted
                // between tool calls instead of only being labelled after it
                // has already finished overspending.
                if let (
                    Some(budget),
                    StreamEvent::Assistant {
                        message,
                        session_id: _,
                    },
                ) = (token_budget, &stream_event)
                {
                    if let Some(usage) = &message.usage {
                        let tokens = usage.input_tokens.saturating_add(usage.output_tokens);
                        if budget.add(tokens) {
                            self.write_interrupt_control_request().await;
                            return Err(AppError::Limit(budget.exceeded_message()));
                        }
                    }
                }

                // Intercept permission requests and answer them inline (with UI
                // narration) before any other handling — Claude blocks until it
                // gets the control_response, and the accumulator can't send one.
                if let StreamEvent::ControlRequest {
                    request_id,
                    request,
                } = &stream_event
                {
                    self.answer_control_request(
                        request_id,
                        request,
                        Some(channel),
                        slots,
                        turn_deadline,
                    )
                    .await?;
                    continue; // control events are not turn content — keep reading
                }

                // Emit per-content-block events for assistant messages.
                // The accumulator also processes the message below (capturing
                // session_id, text, and thinking for the terminal Result event).
                if let StreamEvent::Assistant { message, .. } = &stream_event {
                    for block in &message.content {
                        match block {
                            ContentBlock::Text { text: t } => {
                                let _ = channel.send(ClaudeEvent::TextDelta { text: t.clone() });
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
                                // `AskUserQuestion` is surfaced via the
                                // dedicated `ask_user_question` channel event
                                // (which shows the pinned question card), so
                                // emitting its raw tool_use block here would
                                // only produce a redundant activity row in the
                                // transcript. Skip it on the wire entirely —
                                // the frontend filters it too, but stopping it
                                // at the source keeps the persisted blocks and
                                // the live view consistent.
                                if name == "AskUserQuestion" {
                                    continue;
                                }

                                // `TaskUpdate` carries its state change in the
                                // call input, not the result. Emit a live
                                // `task_update` from here (the dedicated
                                // TaskPanel is the surface for it) and skip the
                                // generic activity row — a `› TaskUpdate ·
                                // {json}` line would be pure noise above the
                                // panel, same reasoning as AskUserQuestion.
                                // (Persisted copies land in the accumulator's
                                // `tasks` via the same helper.)
                                if name == "TaskUpdate" {
                                    if let Some(task) =
                                        crate::agents::extract_task_from_tool_use(name, input)
                                    {
                                        let _ = channel.send(ClaudeEvent::TaskUpdate { task });
                                    }
                                    continue;
                                }

                                let _ = channel.send(ClaudeEvent::ToolUse {
                                    name: name.clone(),
                                    input: input.to_string(),
                                });
                            }
                        }
                    }
                }

                // Surface task lifecycle events (Task/TodoWrite creates /
                // updates) as live activity — same rationale as tool_use: a
                // long agentic turn otherwise shows only a spinner, and task
                // progress is exactly the kind of signal the user wants to see.
                // Mirrors the persisted `tasks` field; uses the shared
                // extractor so live + persisted stay in sync.
                if let Some(task) = crate::agents::extract_task_from_tool_result(&stream_event) {
                    let _ = channel.send(ClaudeEvent::TaskUpdate { task });
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

            // Turn ended normally — clear the interrupt sender so a stale
            // interrupt from the next turn (or a no-op) can't fire into a
            // dropped receiver. Best-effort; the lock can't deadlock (no await).
            let _ = interrupt_slot.lock().ok().and_then(|mut g| g.take());

            Ok(response)
        }
        .await;

        // Defensive: clear any stale parking entries. A normal turn end already
        // cleared them via the result/answer paths, but an errored / timed-out /
        // interrupted turn may have left a pending approval or question whose
        // oneshot will never resolve. Without this, agent_pending_* would keep
        // reporting a phantom prompt. Runs on every exit (Ok and Err).
        let _ = interrupt_slot.lock().ok().and_then(|mut g| g.take());
        let _ = slots.question.lock().ok().and_then(|mut g| g.take());
        let _ = slots.permission.lock().ok().and_then(|mut g| g.take());
        let _ = slots.plan.lock().ok().and_then(|mut g| g.take());

        result
    }
}

impl HarnessAdapter for ClaudeSession {
    fn spawn(
        cwd: &Path,
        config: &AgentConfig,
        resume_session_id: Option<&str>,
        policy: PermissionPolicy,
    ) -> Result<Self, AppError> {
        Self::spawn(&cwd.to_path_buf(), config, resume_session_id, policy)
    }

    fn is_usable(&mut self) -> bool {
        // Claude owns no explicit wedge marker: an existing cached process is
        // reused just as it was before the adapter extraction.
        true
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

impl Drop for ClaudeSession {
    fn drop(&mut self) {
        // Close stdin FIRST so claude sees EOF and exits on its own. We are
        // inside drop(), so struct fields have NOT dropped yet — without this
        // explicit close, the reap below would wait on a process that's itself
        // blocked reading more stdin.
        self.stdin.take();

        // Move the child + stderr-drain handle out so the reap task can own
        // them without touching `self` (being torn down). Both are `Option`
        // precisely so Drop can take them here.
        let child = self.child.take();
        let stderr_drain = self.stderr_drain.take();

        let Some(child) = child else {
            return; // already taken — nothing to reap
        };

        // Reap the child OFF the tokio worker thread. `poll_reap` sleeps up to
        // 7s (5s graceful EOF window + 2s after SIGKILL) using `thread::sleep`;
        // running that on an async worker stalls every other task sharing it
        // (and `ClaudeSession` is dropped from inside async Tauri commands). We
        // fire-and-forget a `spawn_blocking` task that owns the child + drain,
        // reaps the child, then aborts the drain. `tokio::spawn` + `child
        // .wait().await` would also avoid blocking, but `spawn_blocking` lets
        // us keep the bounded graceful-then-forceful reap (claude gets a chance
        // to flush its `--resume` session state before SIGKILL).
        match tokio::runtime::Handle::try_current() {
            Ok(handle) => {
                // Detach: `spawn_blocking` has already queued the closure on the
                // blocking pool, so dropping the JoinHandle does NOT cancel it —
                // it runs to completion and reaps the child. (Distinct from
                // `Child::kill()`, whose future must be *awaited* to send the
                // signal — dropping that one unawaited was the checkpoint-4 bug.)
                drop(handle.spawn_blocking(move || reap_child(child, stderr_drain)));
            }
            Err(_) => {
                // No runtime context (e.g. process teardown, or dropped from a
                // non-runtime thread). Block here — there's no async worker to
                // stall — so we still reap instead of leaking a zombie.
                reap_child(child, stderr_drain);
            }
        }
    }
}

/// Graceful-then-forceful reap of a claude child, then abort the stderr drain.
///
/// Phase 1 gives the child a bounded window to exit on EOF (stdin was closed
/// before this is called). Phase 2 SIGKILLs and reaps again. The stderr-drain
/// task is kept alive *throughout* the reap so a verbose child can't fill its
/// stderr pipe buffer and block on exit; it's aborted only after the child is
/// gone (its stderr then EOFs and the drain would end on its own anyway — the
/// abort is a defensive backstop for a stuck read). `start_kill()` is
/// synchronous — the correct tokio API from a sync context; its `kill()`
/// counterpart returns a future that can't be awaited here, and `let _ =
/// kill()` would silently leak the child instead of killing it.
fn reap_child(mut child: Child, stderr_drain: Option<JoinHandle<()>>) {
    let reaped = poll_reap(&mut child, Duration::from_secs(5));

    if !reaped {
        tracing::debug!("claude session didn't exit on EOF; force-killing");
        let _ = child.start_kill();
        poll_reap(&mut child, Duration::from_secs(2));
    }

    if let Some(handle) = stderr_drain {
        handle.abort();
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

    fn png(data: &str) -> Attachment {
        Attachment {
            media_type: String::from("image/png"),
            data: String::from(data),
        }
    }

    #[test]
    fn text_only_turn_keeps_the_single_text_block_shape() {
        let turn = user_turn_json("hello", &[]);

        assert_eq!(turn["type"], "user");
        assert_eq!(turn["message"]["role"], "user");
        let content = turn["message"]["content"].as_array().expect("content");
        assert_eq!(content.len(), 1);
        assert_eq!(content[0]["type"], "text");
        assert_eq!(content[0]["text"], "hello");
    }

    #[test]
    fn attached_image_becomes_an_inline_base64_source_block() {
        let turn = user_turn_json("what colour?", &[png("aGVsbG8=")]);

        let content = turn["message"]["content"].as_array().expect("content");
        assert_eq!(content.len(), 2);
        assert_eq!(content[0]["type"], "image");
        assert_eq!(content[0]["source"]["type"], "base64");
        assert_eq!(content[0]["source"]["media_type"], "image/png");
        assert_eq!(content[0]["source"]["data"], "aGVsbG8=");
    }

    #[test]
    fn images_precede_the_text_block_and_keep_composer_order() {
        let turn = user_turn_json("compare these", &[png("Zmlyc3Q="), png("c2Vjb25k")]);

        let content = turn["message"]["content"].as_array().expect("content");
        assert_eq!(content.len(), 3);
        assert_eq!(content[0]["source"]["data"], "Zmlyc3Q=");
        assert_eq!(content[1]["source"]["data"], "c2Vjb25k");
        // Text last — see `user_turn_json` for why.
        assert_eq!(content[2]["type"], "text");
        assert_eq!(content[2]["text"], "compare these");
    }

    #[test]
    fn image_with_no_text_omits_the_empty_text_block() {
        let turn = user_turn_json("", &[png("YWJj")]);

        let content = turn["message"]["content"].as_array().expect("content");
        assert_eq!(content.len(), 1);
        assert_eq!(content[0]["type"], "image");
    }

    #[test]
    fn empty_turn_with_no_images_keeps_a_text_block() {
        let turn = user_turn_json("", &[]);

        let content = turn["message"]["content"].as_array().expect("content");
        assert_eq!(content.len(), 1);
        assert_eq!(content[0]["type"], "text");
    }

    /// The turn is written as one NDJSON line, so an embedded newline in the
    /// base64 (or the text) would split it into two malformed lines.
    #[test]
    fn serialized_turn_is_a_single_line() {
        let turn = user_turn_json("line one\nline two", &[png("YWJj")]);
        let encoded = turn.to_string();

        assert!(!encoded.contains('\n'), "turn must serialise to one line");
    }

    /// Test agent config.
    ///
    /// Centralised here so both integration tests share one source of truth.
    /// Lower `effort` (e.g. `"low"`) if the tests feel slow.
    ///
    /// The auth token is read from `LOOPDECK_TEST_AUTH_TOKEN` (never committed).
    /// If unset, returns `None` and the ignored integration tests will fail
    /// with a clear error rather than running with no credential — set the env
    /// var before invoking `cargo test -- --ignored`. The `base_url`/`model`
    /// defaults are non-secret and committed for convenience.
    fn test_config() -> AgentConfig {
        AgentConfig {
            base_url: std::env::var("LOOPDECK_TEST_BASE_URL")
                .ok()
                .or_else(|| Some(String::from("https://api.deepseek.com/anthropic"))),
            model: std::env::var("LOOPDECK_TEST_MODEL")
                .ok()
                .or_else(|| Some(String::from("deepseek-v4-pro[1m]"))),
            auth_token: std::env::var("LOOPDECK_TEST_AUTH_TOKEN").ok(),
            effort: Some(String::from("low")),
            ..Default::default()
        }
    }

    fn test_path() -> PathBuf {
        std::env::current_dir().expect("cannot found current directory")
    }

    /// A fresh empty question slot for tests. The ignored integration tests
    /// don't trigger AskUserQuestion, but `send_message[_streaming]` now
    /// require the parameter — an empty slot stands in for "no question".
    fn qslot() -> QuestionSlot {
        Arc::new(StdMutex::new(None))
    }

    /// A fresh empty permission slot for tests — the manual-approval
    /// counterpart of `qslot`. The ignored integration tests below predate the
    /// manual-approval arm; an empty slot stands in for "no pending approval".
    /// Note the two permission integration tests (handles_permission_request,
    /// deny_path_is_graceful) intentionally rely on the synchronous floor /
    /// policy path: they spawn allow-/deny-by-default and exercise commands
    /// not in `MANUAL_APPROVAL_TOOLS`… except `git status` IS Bash, so those
    /// would now park forever on this empty slot. The tests pass a slot whose
    /// sender is pre-disconnected so a pending approval immediately errors the
    /// turn rather than hanging — see `disconnected_pslot`.
    fn pslot() -> PermissionSlot {
        Arc::new(StdMutex::new(None))
    }

    /// A fresh empty plan-approval slot for tests — the `ExitPlanMode`
    /// counterpart of `pslot`. The ignored integration tests below never enter
    /// `plan` permission mode, so `ExitPlanMode` is never called and an empty
    /// slot stands in for "no pending plan".
    fn plan_slot() -> PlanSlot {
        Arc::new(StdMutex::new(None))
    }

    /// A fresh empty interrupt slot for tests — the graceful-stop counterpart
    /// of `qslot`/`pslot`. The ignored integration tests predate the interrupt
    /// path; an empty slot stands in for "no interrupt requested".
    fn islot() -> InterruptSlot {
        Arc::new(StdMutex::new(None))
    }

    // ── bounded line reader (PRD FR4) ───────────────────────────────────

    /// Wrap raw bytes in the same `BufReader<Cursor>` shape `read_bounded_line`
    /// is built for, so the unit tests exercise the real async path.
    fn buf_reader(bytes: &'static [u8]) -> tokio::io::BufReader<std::io::Cursor<&'static [u8]>> {
        tokio::io::BufReader::new(std::io::Cursor::new(bytes))
    }

    #[tokio::test]
    async fn read_bounded_line_returns_normal_line() {
        let mut reader = buf_reader(b"{\"type\":\"result\"}\nsecond\n");
        let mut buf = Vec::new();
        let read = read_bounded_line(&mut reader, &mut buf, 1024)
            .await
            .unwrap();
        assert!(matches!(read, BoundedRead::Line));
        assert_eq!(&buf, b"{\"type\":\"result\"}\n");
    }

    #[tokio::test]
    async fn read_bounded_line_eof_on_empty() {
        let mut reader = buf_reader(b"");
        let mut buf = Vec::new();
        let read = read_bounded_line(&mut reader, &mut buf, 1024)
            .await
            .unwrap();
        assert!(matches!(read, BoundedRead::Eof));
        assert!(buf.is_empty());
    }

    #[tokio::test]
    async fn read_bounded_line_discards_oversized_line() {
        // A 50-byte line (incl. newline) under a 10-byte cap → Oversized, the
        // whole line consumed, buf empty, and the NEXT read resumes at the
        // following line (no data lost across the discarded one).
        let mut payload: Vec<u8> = vec![b'x'; 49];
        payload.push(b'\n');
        let mut stream: Vec<u8> = payload.clone();
        stream.extend_from_slice(b"ok\n");
        let mut cursor = std::io::Cursor::new(stream);
        let mut reader = tokio::io::BufReader::new(&mut cursor);

        let mut buf = Vec::new();
        let read = read_bounded_line(&mut reader, &mut buf, 10).await.unwrap();
        assert!(matches!(read, BoundedRead::Oversized));
        assert!(buf.is_empty(), "oversized line must not be buffered");

        // Next read returns the following, normal line.
        let read = read_bounded_line(&mut reader, &mut buf, 10).await.unwrap();
        assert!(matches!(read, BoundedRead::Line));
        assert_eq!(&buf, b"ok\n");
    }

    #[tokio::test]
    async fn read_bounded_line_allows_line_at_exact_cap() {
        // A line exactly at the cap (inclusive) is allowed — the bound is `>`.
        let mut reader = buf_reader(b"abc\n");
        let mut buf = Vec::new();
        let read = read_bounded_line(&mut reader, &mut buf, 4).await.unwrap();
        assert!(matches!(read, BoundedRead::Line));
        assert_eq!(&buf, b"abc\n");
    }

    #[tokio::test]
    async fn read_bounded_line_returns_final_line_without_newline() {
        // A trailing line with no newline at EOF is still returned as a Line.
        let mut reader = buf_reader(b"tail-no-newline");
        let mut buf = Vec::new();
        let read = read_bounded_line(&mut reader, &mut buf, 1024)
            .await
            .unwrap();
        assert!(matches!(read, BoundedRead::Line));
        assert_eq!(&buf, b"tail-no-newline");
    }

    // ── park_until (absolute turn deadline) ────────────────────────────

    #[test]
    fn turn_deadline_is_a_finite_backstop() {
        // Lock the backstop value: 30 minutes, finite. A change here should be
        // deliberate (and reflected in the TURN_DEADLINE doc + decisions.md).
        assert_eq!(TURN_DEADLINE, Duration::from_secs(30 * 60));
    }

    #[test]
    fn turn_deadline_expired_error_describes_lock_release() {
        let msg = match turn_deadline_expired_error("AskUserQuestion — which port?", None) {
            AppError::TurnParked { detail, .. } => detail,
            _ => panic!("expected AppError::TurnParked variant"),
        };
        assert!(msg.contains("turn deadline"), "msg: {msg}");
        assert!(msg.contains("lock was released"), "msg: {msg}");
        assert!(msg.contains("which port?"), "msg: {msg}");
    }

    #[test]
    fn turn_deadline_expired_error_truncates_long_detail() {
        let long_input = "x".repeat(500);
        let msg = match turn_deadline_expired_error(&long_input, None) {
            AppError::TurnParked { detail, .. } => detail,
            _ => panic!("expected AppError::TurnParked variant"),
        };
        assert!(!msg.contains(&long_input), "detail should be truncated");
        assert!(msg.contains('…'), "msg: {msg}");
    }

    #[tokio::test]
    async fn park_until_answered_when_receiver_resolves_before_deadline() {
        let deadline = tokio::time::Instant::now() + Duration::from_secs(60);
        let (tx, rx) = oneshot::channel::<u32>();
        tx.send(42).unwrap();
        let outcome = park_until(deadline, rx).await;
        match outcome {
            ParkOutcome::Answered(v) => assert_eq!(v, 42),
            other => panic!("expected Answered, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn park_until_cancelled_when_sender_dropped() {
        let deadline = tokio::time::Instant::now() + Duration::from_secs(60);
        let (tx, rx) = oneshot::channel::<u32>();
        drop(tx); // sender dropped without sending — receiver resolves to a RecvError
        let outcome = park_until(deadline, rx).await;
        match outcome {
            ParkOutcome::Cancelled => {}
            other => panic!("expected Cancelled, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn park_until_expired_when_deadline_already_passed() {
        // A deadline in the past must expire immediately — never hang waiting
        // for the (never-coming) answer. Race it against a short wall-clock
        // guard so a regression (e.g. a per-park relative timeout instead of
        // the absolute `timeout_at`) fails loudly instead of stalling the
        // suite for TURN_DEADLINE.
        let past = tokio::time::Instant::now() - Duration::from_secs(60);
        let (_tx, rx) = oneshot::channel::<u32>(); // never answered
        let outcome = tokio::time::timeout(Duration::from_millis(500), park_until(past, rx))
            .await
            .expect("park_until with a past deadline must not hang");
        assert!(matches!(outcome, ParkOutcome::Expired), "got {outcome:?}");
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
            let mut session = ClaudeSession::spawn(
                &test_path(),
                &test_config(),
                None,
                PermissionPolicy::confirm_changes(),
            )
            .expect("failed to spawn claude session");

            let response = session
                .send_message(
                    "reply with exactly: hello",
                    &[],
                    &ParkSlots {
                        question: &qslot(),
                        permission: &pslot(),
                        plan: &plan_slot(),
                    },
                    &islot(),
                )
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

        let mut session = ClaudeSession::spawn(&test_path(), &test_config(), None, PermissionPolicy::confirm_changes())
            .expect("failed to spawn claude session");

        let response = session
            .send_message("is there a Cargo.toml in this directory, response with YES or NO only, No preamble", &[], &ParkSlots { question: &qslot(), permission: &pslot(), plan: &plan_slot() }, &islot())
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
            let mut session = ClaudeSession::spawn(&test_path(), &test_config(), None, PermissionPolicy::confirm_changes())
            .expect("failed to spawn claude session");

        // Turn 1 — plant a distinctive fact the model could only repeat by
        // remembering it (not by guessing from turn 2's question alone).
        let first = session
            .send_message("I'm telling you a secret codeword. The codeword is: ZUCCHINI. Reply with exactly: understood.", &[], &ParkSlots { question: &qslot(), permission: &pslot(), plan: &plan_slot() }, &islot())
            .await
            .expect("turn 1 (send_message) failed");
        assert!(!first.is_error, "turn 1 should not error, got: {:?}", first);

        // Turn 2 — recall it. Same process, same session, no --resume.
        let second = session
            .send_message("What is the secret codeword I just told you? Reply with only the codeword, nothing else.", &[], &ParkSlots { question: &qslot(), permission: &pslot(), plan: &plan_slot() }, &islot())
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
                ..Default::default()
            };
            let mut session = ClaudeSession::spawn(
                &test_path(),
                &no_auth,
                None,
                PermissionPolicy::confirm_changes(),
            )
            .expect("failed to spawn claude session");

            let response = session
                .send_message(
                    "reply with: hello",
                    &[],
                    &ParkSlots {
                        question: &qslot(),
                        permission: &pslot(),
                        plan: &plan_slot(),
                    },
                    &islot(),
                )
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
            let mut session = ClaudeSession::spawn(
                &test_path(),
                &test_config(),
                None,
                PermissionPolicy::confirm_changes(),
            )
            .expect("failed to spawn claude session");
            let first = session
                .send_message(
                    "I'm telling you a secret codeword. The codeword is: EGGPLANT. \
                     Reply with exactly: understood.",
                    &[],
                    &ParkSlots {
                        question: &qslot(),
                        permission: &pslot(),
                        plan: &plan_slot(),
                    },
                    &islot(),
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
            let mut resumed = ClaudeSession::spawn(
                &test_path(),
                &test_config(),
                Some(&session_id),
                PermissionPolicy::confirm_changes(),
            )
            .expect("failed to re-spawn claude session with --resume");

            let second = resumed
                .send_message(
                    "What is the secret codeword I told you earlier? \
                     Reply with only the codeword, nothing else.",
                    &[],
                    &ParkSlots {
                        question: &qslot(),
                        permission: &pslot(),
                        plan: &plan_slot(),
                    },
                    &islot(),
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

    /// PRD Test Plan #1 — control-protocol round-trip via a read-only tool.
    ///
    /// Forces a `Read` tool call (which, under `default` mode and not on any
    /// allow list, emits a `control_request`). `Read` is NOT in
    /// `MANUAL_APPROVAL_TOOLS`, so under allow-by-default the synchronous
    /// policy auto-allows it — proving the control-protocol round-trip works
    /// without parking on the manual-approval slot. Before the original fix,
    /// this would stall until READ_LINE_TIMEOUT (180s).
    ///
    /// (The earlier version of this test used `git status` via Bash, but Bash
    /// is now in the manual-approval set, so it would park on `permission_slot`
    /// with no UI to resolve it. The manual-approval path itself is exercised
    /// at the unit level in `permission.rs` and will get a dedicated
    /// integration test once a test harness can drive `agent_answer_permission`.)
    #[tokio::test]
    #[ignore = "calls a real provider; run with `cargo test -- --ignored`"]
    async fn test_session_handles_permission_request() {
        let result = tokio::time::timeout(std::time::Duration::from_secs(120), async {
            let mut session = ClaudeSession::spawn(
                &test_path(),
                &test_config(),
                None,
                PermissionPolicy::confirm_changes(),
            )
            .expect("failed to spawn claude session");

            // Read is a safe tool not in MANUAL_APPROVAL_TOOLS; under default
            // mode it routes through the control protocol and is auto-allowed
            // by the synchronous policy. If the response is written, claude
            // completes the turn normally.
            let response = session
                .send_message(
                    "Use the Read tool to read the file `Cargo.toml` in the current directory, \
                     then report the package name in one short sentence.",
                    &[],
                    &ParkSlots {
                        question: &qslot(),
                        permission: &pslot(),
                        plan: &plan_slot(),
                    },
                    &islot(),
                )
                .await
                .expect("send_message failed — did the permission request go unanswered?");

            assert!(
                !response.is_error,
                "turn should not error once the permission is answered, got: {:?}",
                response
            );
        })
        .await;

        assert!(
            result.is_ok(),
            "Test timed out — permission round-trip likely stalled"
        );
    }

    /// PRD Test Plan #2 — deny path is graceful.
    ///
    /// With a deny-by-default policy, every un-ruled tool request is denied
    /// immediately by the synchronous policy (no manual-approval parking —
    /// manual approval only runs when the floor AND the policy would allow).
    /// The CLI must recover from the deny and continue the turn (emit a result)
    /// rather than hanging or crashing. This is the key safety property: the
    /// agent can be told "no" and still finish gracefully.
    #[tokio::test]
    #[ignore = "calls a real provider; run with `cargo test -- --ignored`"]
    async fn test_session_deny_path_is_graceful() {
        use crate::permission::{PermissionMode, PermissionPolicy};

        let result = tokio::time::timeout(std::time::Duration::from_secs(120), async {
            let mut session = ClaudeSession::spawn(
                &test_path(),
                &test_config(),
                None,
                PermissionPolicy::with_mode(PermissionMode::Deny),
            )
            .expect("failed to spawn claude session");

            // Read is denied under deny-by-default; the model should recover
            // and report that it couldn't read the file.
            let response = session
                .send_message(
                    "Try to read the file `Cargo.toml` with the Read tool. \
                     If it's blocked, that's fine — just say so in one sentence.",
                    &[],
                    &ParkSlots {
                        question: &qslot(),
                        permission: &pslot(),
                        plan: &plan_slot(),
                    },
                    &islot(),
                )
                .await
                .expect("send_message failed — CLI should recover from a denied tool call");

            // The turn completes (no hang). It may or may not be an error
            // depending on how the model reacts to the denial — what matters
            // is that we GET a result event at all.
            assert!(
                !response.result.is_empty(),
                "deny path should still produce a result, got: {:?}",
                response
            );
        })
        .await;

        assert!(
            result.is_ok(),
            "Test timed out — deny path stalled instead of recovering"
        );
    }
}
