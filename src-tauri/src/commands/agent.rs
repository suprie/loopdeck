//! Agent / Claude session commands — the 19 `agent_*` IPC handlers plus their
//! private helpers (retry wrappers, transcript-recording pipelines, the
//! fresh-start pipeline, and the loop-prompt builder).

use super::state::{
    fire_interrupt, interrupt_slot, permission_slot, plan_slot, project_busy, question_slot,
    resolve_agent_config, resolve_permission_policy, resolve_root, with_session, AppState,
};
use crate::agents::{AgentResponse, ClaudeEvent, TokenBudget};
use crate::claude_session::{ParkSlots, QuestionAnswers};
use crate::conversation::{self, Attachment, ConversationSummary, ConversationTurn};
use crate::error::AppError;
use crate::harness::HarnessSession;
use crate::limits;
use crate::permission::{Decision as PermissionDecision, PermissionMode, PermissionPolicy};
use crate::retry;
use crate::retry::MAX_ATTEMPTS;
use serde::Deserialize;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tauri::ipc::Channel;
use tauri::State;
use tracing::{debug, info, warn};

// ── Agent commands ─────────────────────────────────────────────────────────

/// Start the next development loop for a project.
///
/// Builds the next-loop prompt from `.loopdeck/loops.md` (first unchecked
/// step under `## Next Steps`, or a "propose the next loop" fallback) and
/// sends it through the **fresh-start** pipeline: any live session is dropped,
/// the transcript is archived, and a brand-new claude process is spawned
/// **without** `--resume` — Start always begins a new conversation.
///
/// Concurrency: uses `try_lock`. If a turn is already in flight on this
/// project, Start is rejected immediately with "agent is busy" rather than
/// queueing (only `agent_send_message` queues). The successful `try_lock` is
/// the proof that the prior session is idle and safe to replace.
#[tauri::command]
pub async fn agent_start_loop(
    path: String,
    state: State<'_, AppState>,
) -> Result<AgentResponse, AppError> {
    debug!("agent_start_loop called for path: {path}");
    // Resolve the canonical, registered root (PRD FR3): the agent process is
    // spawned with this as its cwd, so it must be a registered project.
    let root = resolve_root(&state, &path)?;

    let (prompt, title) = build_next_loop_prompt(&root);
    let response = start_fresh_and_record(&state, &root, &prompt, title).await?;
    info!("agent_start_loop complete for: {path}");
    Ok(response)
}

/// Send a free-form follow-up message to the project's agent session.
///
/// Continues the **existing** conversation: reuses the live process if present,
/// or (after an app restart, when no live process exists) re-spawns claude with
/// `--resume <last_session_id>` so the model's context is restored. Both turns
/// are recorded. Contrast with `agent_start_loop`, which always begins a fresh
/// conversation.
///
/// Concurrency: uses `lock().await`, so a follow-up sent while a turn is in
/// flight on this project queues behind it (different projects run in parallel).
#[tauri::command]
pub async fn agent_send_message(
    path: String,
    prompt: String,
    attachments: Option<Vec<Attachment>>,
    state: State<'_, AppState>,
) -> Result<AgentResponse, AppError> {
    debug!("agent_send_message called for path: {path}");
    let root = resolve_root(&state, &path)?;
    // `Option` so existing callers that omit the argument keep working —
    // Tauri deserializes a missing field to `None` rather than erroring.
    let attachments = validate_attachments(attachments.unwrap_or_default())?;

    let response = send_and_record(&state, &root, &prompt, &attachments).await?;
    info!("agent_send_message complete for: {path}");
    Ok(response)
}

/// Start the next development loop with streaming events via Tauri Channel.
///
/// Like `agent_start_loop`, but emits each assistant content block as a
/// `ClaudeEvent` on the `on_event` channel as it arrives. The transcript is
/// still recorded atomically after the turn completes.
///
/// Fresh-start semantics, identical to `agent_start_loop`: drops any live
/// session, archives the transcript, spawns without `--resume`, rejects with
/// "agent is busy" if a turn is in flight (`try_lock`).
///
/// Returns `()` rather than `AgentResponse` — the terminal result event
/// (`ClaudeEvent::Result`) carries the full aggregated response.
#[tauri::command]
pub async fn agent_start_loop_streaming(
    path: String,
    on_event: Channel<ClaudeEvent>,
    state: State<'_, AppState>,
) -> Result<(), AppError> {
    debug!("agent_start_loop_streaming called for path: {path}");
    let root = resolve_root(&state, &path)?;

    let (prompt, title) = build_next_loop_prompt(&root);
    let _ = start_fresh_and_record_streaming(&state, &root, &prompt, title, &on_event).await?;
    info!("agent_start_loop_streaming complete for: {path}");
    Ok(())
}

/// Send a free-form follow-up message with streaming events via Tauri Channel.
///
/// Like `agent_send_message`, but each assistant content block is emitted as
/// a `ClaudeEvent` on the `on_event` channel as it arrives, so the frontend
/// can render tokens immediately. The transcript is still recorded atomically
/// after the turn completes.
///
/// Returns `()` rather than `AgentResponse` — the terminal result event
/// (`ClaudeEvent::Result`) carries the full aggregated response, so the
/// frontend doesn't need to await the return value.
///
/// `plan_mode`: when true, the turn runs under the CLI's `plan` permission
/// mode (mirrors Claude Code's own shift-tab toggle) — the agent is
/// restricted to read-only tools plus `ExitPlanMode`, which surfaces a
/// `ClaudeEvent::PlanApproval` card for the user to approve/reject instead of
/// letting the agent edit anything.
#[tauri::command]
pub async fn agent_send_message_streaming(
    path: String,
    prompt: String,
    attachments: Option<Vec<Attachment>>,
    on_event: Channel<ClaudeEvent>,
    plan_mode: bool,
    state: State<'_, AppState>,
) -> Result<(), AppError> {
    debug!("agent_send_message_streaming called for path: {path}, plan_mode: {plan_mode}");
    let root = resolve_root(&state, &path)?;
    let attachments = validate_attachments(attachments.unwrap_or_default())?;

    send_and_record_streaming(&state, &root, &prompt, &attachments, &on_event, plan_mode).await?;
    info!("agent_send_message_streaming complete for: {path}");
    Ok(())
}

/// Load the persisted conversation transcript for the Agent tab.
///
/// Returns an empty vec when no conversation has been recorded yet. Lenient
/// about malformed lines (a corrupt append doesn't hide earlier turns).
#[tauri::command]
pub async fn agent_get_conversation(
    path: String,
    state: State<'_, AppState>,
) -> Result<Vec<ConversationTurn>, AppError> {
    debug!("agent_get_conversation called for path: {path}");
    let root = resolve_root(&state, &path)?;
    Ok(conversation::load_conversation(&root))
}

/// List all conversations (active + archived) for the history UI.
///
/// Returns one `ConversationSummary` per transcript file in the sessions dir,
/// sorted newest-first by last-turn timestamp. Each row carries an id
/// (`"active"` or an archive stem) the frontend passes back to
/// `agent_get_conversation_by_id` to load the turns.
#[tauri::command]
pub async fn agent_list_conversations(
    path: String,
    state: State<'_, AppState>,
) -> Result<Vec<ConversationSummary>, AppError> {
    debug!("agent_list_conversations called for path: {path}");
    let root = resolve_root(&state, &path)?;
    Ok(conversation::list_conversations(&root))
}

/// Load a specific conversation by id (`"active"` or an archive stem).
///
/// Used by the history viewer to open a past conversation read-only. Returns
/// an empty vec for an unknown id (e.g. an archive deleted out of band) — the
/// UI shows an empty state rather than erroring.
#[tauri::command]
pub async fn agent_get_conversation_by_id(
    path: String,
    id: String,
    state: State<'_, AppState>,
) -> Result<Vec<ConversationTurn>, AppError> {
    debug!("agent_get_conversation_by_id called for path: {path}, id: {id}");
    let root = resolve_root(&state, &path)?;
    Ok(conversation::load_conversation_by_id(&root, &id))
}

/// Promote an archived conversation to active, returning its `session_id`.
///
/// Called by the frontend when the user sends a follow-up while viewing an
/// archived conversation. The backend:
/// 1. `promote_to_active` — rotates the current `active.jsonl` aside (so it
///    survives in history) and seeds a fresh active transcript with the chosen
///    archive's turns.
/// 2. Extracts the most recent assistant `session_id` from that conversation
///    so the agent pipeline can `--resume` it, restoring the model's context.
///
/// Returns `None` when the source has no `session_id` (empty, or only user
/// turns) — in that case the frontend proceeds with a non-resume start. The
/// live session is also dropped (its context is now stale relative to the
/// promoted transcript); the next send re-spawns with the returned resume id.
#[tauri::command]
pub async fn agent_promote_to_active(
    path: String,
    id: String,
    state: State<'_, AppState>,
) -> Result<Option<String>, AppError> {
    debug!("agent_promote_to_active called for path: {path}, id: {id}");
    let root = resolve_root(&state, &path)?;

    // Extract the resume id BEFORE promoting (after promotion the turns live
    // in active.jsonl, but reading from the source id is unambiguous).
    let resume_id = conversation::session_id_for_conversation(&root, &id);

    // Promote: archive current active, seed new active from the source. No-op
    // for `id == "active"` or an unknown/empty source — both safe here.
    conversation::promote_to_active(&root, &id)?;

    // Drop any live session — its in-process context is now stale relative to
    // the promoted transcript. The next send re-spawns with `--resume <id>`
    // via `with_session` (which reads `last_session_id` off the new active).
    // The session map is keyed by the canonical root (installed by the agent
    // pipeline), so removing by `root` matches.
    let removed = state
        .claude_sessions
        .lock()
        .map_err(|_| AppError::LockError)?
        .remove(&root)
        .is_some();
    if removed {
        debug!("dropped live session for promote of {id} in: {path}");
    }

    info!("agent_promote_to_active complete for: {path}, id: {id}");
    Ok(resume_id)
}

/// Reset the project's agent session: drop the live process and archive the
/// transcript.
///
/// The next `agent_start_loop` starts a fresh conversation (no `--resume`).
/// The archived transcript is preserved as `archive-<ts>.jsonl` for history.
#[tauri::command]
pub async fn agent_reset_session(path: String, state: State<'_, AppState>) -> Result<(), AppError> {
    debug!("agent_reset_session called for path: {path}");
    let root = resolve_root(&state, &path)?;

    // Remove the live session from the map. The Arc's last reference drops
    // here → `ClaudeSession::Drop` closes stdin and reaps the child. The map
    // is keyed by the canonical root (installed by the agent pipeline).
    let removed = state
        .claude_sessions
        .lock()
        .map_err(|_| AppError::LockError)?
        .remove(&root)
        .is_some();
    if removed {
        debug!("dropped live claude session for: {path}");
    }

    // Archive the transcript regardless of whether a live session existed —
    // a reset should always mean "next Start is fresh".
    conversation::archive_conversation(&root)?;

    info!("agent_reset_session complete for: {path}");
    Ok(())
}

/// Wire shape of a single answer as sent by the frontend
/// (`agent_answer_question`'s `answers` map value).
///
/// `labels` carries the selected option label(s); `other_text` carries the
/// free-text "Other…" value when the user typed one instead of (or alongside)
/// picking a canned option. Both optional so the frontend can send whichever
/// applies.
#[derive(Debug, Deserialize)]
pub struct AnswerWire {
    #[serde(default)]
    pub labels: Vec<String>,
    #[serde(default)]
    pub other_text: Option<String>,
}

/// Answer a pending `AskUserQuestion` for the given project.
///
/// Called by the frontend when the user submits the question card. Pops the
/// oneshot sender from the per-project `pending_answers` slot and sends the
/// answers — this wakes the read loop (parked in
/// `ClaudeSession::answer_ask_user_question`), which writes the
/// `control_response` with `updatedInput.answers` and the turn resumes.
///
/// Returns an error if no question is pending for this project (the user
/// submitted without a prompt, or the turn already ended/timed out).
#[tauri::command]
pub async fn agent_answer_question(
    path: String,
    request_id: String,
    answers: HashMap<String, AnswerWire>,
    state: State<'_, AppState>,
) -> Result<(), AppError> {
    debug!(
        "agent_answer_question called for path: {path}, request_id: {request_id}, {} answers",
        answers.len()
    );
    let repo_path = PathBuf::from(&path);

    // Pop the sender for this project. There's at most one pending question at
    // a time (Claude blocks on each), so this is a single take of the sender
    // field — the slot entry is cleared entirely so `agent_pending_question`
    // stops reporting it as pending.
    let sender = {
        let guard = state
            .pending_answers
            .lock()
            .map_err(|_| AppError::LockError)?;
        guard
            .get(&repo_path)
            .and_then(|slot| {
                slot.lock()
                    .ok()
                    .and_then(|mut g| g.take())
            })
            .and_then(|pending| pending.sender)
            .ok_or_else(|| {
                AppError::Agent(
                    "no pending question for this project (it may have timed out or already been answered)".into(),
                )
            })?
    };

    // Convert the wire answers into the backend type and send. The sender
    // drops on send, so the slot is now empty for the next question.
    let mapped: QuestionAnswers = answers
        .into_iter()
        .map(|(q, a)| {
            (
                q,
                crate::claude_session::QuestionAnswer {
                    labels: a.labels,
                    other_text: a.other_text.filter(|t| !t.trim().is_empty()),
                },
            )
        })
        .collect();

    sender.send(mapped).map_err(|_| {
        AppError::Agent(
            "the pending question is no longer waiting for an answer (turn ended)".into(),
        )
    })?;

    info!("agent_answer_question delivered for: {path}");
    Ok(())
}

/// Wire shape of the user's manual-approval verdict, as sent by the frontend
/// (`agent_answer_permission`'s `decision` arg).
///
/// `allow: true` → run the tool; `allow: false` → deny it. `reason` is
/// optional and only meaningful on a deny (it's surfaced to the model as the
/// deny message). Mirrors `AnswerWire`'s role for `agent_answer_question`.
#[derive(Debug, Deserialize)]
pub struct ApprovalWire {
    pub allow: bool,
    #[serde(default)]
    pub reason: Option<String>,
}

/// Resolve a pending manual-approval request for the given project.
///
/// Called by the frontend when the user clicks Allow or Deny on the approval
/// card. Pops the oneshot sender from the per-project `pending_permissions`
/// slot and sends the `Decision` — this wakes the read loop (parked in
/// `ClaudeSession::answer_manual_permission`), which writes the matching
/// `control_response` and the turn resumes (or, on deny, recovers).
///
/// Returns an error if no approval is pending for this project (the user
/// clicked after the turn ended/timed out, or there was never a prompt).
#[tauri::command]
pub async fn agent_answer_permission(
    path: String,
    request_id: String,
    decision: ApprovalWire,
    state: State<'_, AppState>,
) -> Result<(), AppError> {
    debug!(
        "agent_answer_permission called for path: {path}, request_id: {request_id}, allow: {}",
        decision.allow
    );
    let repo_path = PathBuf::from(&path);

    // Pop the sender for this project. At most one pending approval at a time
    // (Claude blocks on each control_request), so this is a single take of the
    // sender field — the slot entry is cleared entirely so
    // `agent_pending_permission` stops reporting it as pending.
    let sender = {
        let guard = state
            .pending_permissions
            .lock()
            .map_err(|_| AppError::LockError)?;
        guard
            .get(&repo_path)
            .and_then(|slot| slot.lock().ok().and_then(|mut g| g.take()))
            .and_then(|pending| pending.sender)
            .ok_or_else(|| {
                AppError::Agent(
                    "no pending permission approval for this project (it may have timed out or already been answered)".into(),
                )
            })?
    };

    // Convert the wire verdict into the policy's `Decision` vocabulary — the
    // single source of truth the read loop writes back. A deny with no reason
    // gets a generic message so the model always sees *something*.
    let verdict = if decision.allow {
        PermissionDecision::Allow
    } else {
        PermissionDecision::Deny(
            decision
                .reason
                .filter(|r| !r.trim().is_empty())
                .unwrap_or_else(|| String::from("denied by user")),
        )
    };

    sender.send(verdict).map_err(|_| {
        AppError::Agent(
            "the pending approval is no longer waiting for an answer (turn ended)".into(),
        )
    })?;

    info!("agent_answer_permission delivered for: {path}");
    Ok(())
}

/// Wire shape of the user's plan-approval verdict, as sent by the frontend
/// (`agent_answer_plan`'s `decision` arg). Structurally identical to
/// `ApprovalWire` — `approve: true` lets the agent leave plan mode and start
/// executing; `approve: false` keeps it in plan mode, with `feedback`
/// surfaced to the model so it can revise. Kept as its own type (rather than
/// reusing `ApprovalWire`) so the plan-approval and tool-approval wire shapes
/// can evolve independently.
#[derive(Debug, Deserialize)]
pub struct PlanApprovalWire {
    pub approve: bool,
    #[serde(default)]
    pub feedback: Option<String>,
}

/// Resolve a pending `ExitPlanMode` request for the given project.
///
/// Called by the frontend when the user clicks Approve or Reject on the plan
/// card. Pops the oneshot sender from the per-project `pending_plans` slot and
/// sends the `Decision` — this wakes the read loop (parked in
/// `ClaudeSession::answer_plan_approval`), which writes the matching
/// `control_response`. On approve, the CLI's own `ExitPlanMode` handler
/// reverts the process out of plan mode and the agent starts executing; on
/// reject, the model is expected to revise the plan and call `ExitPlanMode`
/// again within the same turn.
///
/// Returns an error if no plan approval is pending for this project (the user
/// clicked after the turn ended/timed out, or there was never a prompt), OR if
/// `request_id` doesn't match the currently-parked plan. The mismatch case
/// matters because the model can revise and re-propose a plan mid-turn: if the
/// frontend's card is stale (a missed `plan_approval` channel event) and the
/// user clicks Approve/Reject on request A while the backend is now parked on
/// revised plan B, blindly taking the sender would apply the user's verdict —
/// which they gave after reading plan A — to plan B instead. Checking the ID
/// first, and leaving the slot untouched on a mismatch, means a stale click
/// errors instead of silently approving/rejecting a plan the user never saw.
#[tauri::command]
pub async fn agent_answer_plan(
    path: String,
    request_id: String,
    decision: PlanApprovalWire,
    state: State<'_, AppState>,
) -> Result<(), AppError> {
    debug!(
        "agent_answer_plan called for path: {path}, request_id: {request_id}, approve: {}",
        decision.approve
    );
    let repo_path = PathBuf::from(&path);

    // Validate the request_id BEFORE taking the sender — a mismatch leaves
    // the slot untouched (the still-pending, newer plan keeps waiting for its
    // own answer) rather than consuming it on behalf of a stale request.
    let sender = {
        let guard = state
            .pending_plans
            .lock()
            .map_err(|_| AppError::LockError)?;
        let slot = guard.get(&repo_path).ok_or_else(|| {
            AppError::Agent(
                "no pending plan approval for this project (it may have timed out or already been answered)".into(),
            )
        })?;
        let mut slot_guard = slot.lock().map_err(|_| AppError::LockError)?;
        match slot_guard.as_ref() {
            None => {
                return Err(AppError::Agent(
                    "no pending plan approval for this project (it may have timed out or already been answered)".into(),
                ));
            }
            Some(pending) if pending.request_id != request_id => {
                return Err(AppError::Agent(format!(
                    "this plan approval is stale — the agent is now waiting on a different plan (request_id {}); reload and answer the current one",
                    pending.request_id
                )));
            }
            Some(_) => {}
        }
        // IDs match — safe to take. The slot entry is cleared entirely so
        // `agent_pending_plan` stops reporting it as pending.
        slot_guard
            .take()
            .and_then(|pending| pending.sender)
            .ok_or_else(|| {
                AppError::Agent(
                    "the pending plan approval is no longer waiting for an answer (turn ended)"
                        .into(),
                )
            })?
    };

    let verdict = if decision.approve {
        PermissionDecision::Allow
    } else {
        PermissionDecision::Deny(
            decision
                .feedback
                .filter(|r| !r.trim().is_empty())
                .unwrap_or_else(|| String::from("the user rejected this plan")),
        )
    };

    sender.send(verdict).map_err(|_| {
        AppError::Agent(
            "the pending plan approval is no longer waiting for an answer (turn ended)".into(),
        )
    })?;

    info!("agent_answer_plan delivered for: {path}");
    Ok(())
}

/// Persist a permission allow-rule into the project's `.claude/settings.local.json`.
///
/// "Always allow" affordance for the manual-approval card: alongside the
/// per-call Allow/Deny verdict, the user can ask Selasar to remember the
/// decision for future calls of the same tool/command. This writes the rule
/// into `permissions.allow[]` of the **local** settings file (gitignored by
/// Claude Code convention — machine-specific, never shared), deduped against
/// any rules already present.
///
/// The rule string is built by the frontend (it has the parsed tool input and
/// mirrors the `describeTool` field extraction) in the canonical Claude Code
/// format, e.g. `Bash(docker:*)`, `Read(*)`, `mcp__server__tool`. The format is
/// the same one `skills::setup_hooks` seeds into `settings.json` (project
/// scope) at `CURATED_ALLOW` — local + project are merged by Claude Code's own
/// settings loader.
///
/// **Effect timing:** settings are loaded at `ClaudeSession::spawn`, so the
/// rule takes effect on the *next* spawned session (after a reset/restart, or
/// once the live process exits). It does NOT auto-allow the currently-parked
/// approval — that's resolved separately by `agent_answer_permission` writing
/// the `control_response`. The rule just prevents the prompt from reappearing
/// for future calls. Both calls are made by the frontend on "Always allow".
///
/// Idempotent: re-adding an existing rule is a no-op (the dedup preserves the
/// original write order). Creates `.claude/` and the file if absent.
#[tauri::command]
pub async fn agent_add_allow_rule(
    path: String,
    rule: String,
    state: State<'_, AppState>,
) -> Result<(), AppError> {
    debug!("agent_add_allow_rule called for path: {path}, rule: {rule}");
    // Resolve the canonical, registered root (PRD FR3) before writing the
    // project's `.claude/settings.local.json`. (Local var is `proj_root` to
    // avoid clashing with the JSON `root` value below.)
    let proj_root = resolve_root(&state, &path)?;
    let claude_dir = proj_root.join(".claude");
    let settings_path = claude_dir.join("settings.local.json");

    // Load existing settings (or start fresh). A missing file or unparseable
    // body is fine — we treat it as `{}` and (re)write a clean file. This
    // recovers gracefully from a hand-corrupted local settings file.
    let mut root: serde_json::Value = std::fs::read_to_string(&settings_path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_else(|| serde_json::json!({}));

    // Ensure `permissions` is an object and `permissions.allow` is an array,
    // mirroring the structure `skills::setup_hooks` writes into settings.json.
    if root.get("permissions").is_none() {
        root["permissions"] = serde_json::json!({});
    }
    let allow_arr = root["permissions"]["allow"]
        .as_array()
        .cloned()
        .unwrap_or_default();

    // Dedup: only push if the rule isn't already present. Preserves existing
    // order (curated entries stay first, user-added rules append).
    let mut existing: Vec<serde_json::Value> = allow_arr;
    let already_present = existing.iter().any(|v| v.as_str() == Some(rule.as_str()));
    if !already_present {
        existing.push(serde_json::Value::from(rule));
    }
    root["permissions"]["allow"] = serde_json::Value::Array(existing);

    // Write back, pretty-printed for readability / low-diff manual edits.
    std::fs::create_dir_all(&claude_dir)
        .map_err(|e| AppError::Config(format!("failed to create .claude dir: {e}")))?;
    let formatted = serde_json::to_string_pretty(&root)
        .map_err(|e| AppError::Config(format!("JSON serialization error: {e}")))?;
    std::fs::write(&settings_path, formatted)
        .map_err(|e| AppError::Config(format!("failed to write settings.local.json: {e}")))?;

    if already_present {
        info!("agent_add_allow_rule rule already present in {settings_path:?} — no-op");
    } else {
        info!("agent_add_allow_rule wrote rule to {settings_path:?}");
    }
    Ok(())
}

/// Gracefully interrupt the in-flight turn for a project.
///
/// Called by the frontend's Stop button. Pops the oneshot sender from the
/// per-project `interrupt_slots` and fires it; the streaming read loop (which
/// `select!`s on the receiver) wakes, writes the graceful `interrupt`
/// control_request to the live process, and ends the turn. The live process
/// and its conversation context survive (unlike `agent_reset_session`, which
/// kills both) — the next send resumes the same conversation.
///
/// Returns `Ok(())` whether or not a turn was in flight: no-op when idle is
/// the friendlier contract for a UI button (the user just sees "stopped").
/// Returns an error only on state corruption (lock poisoned).
///
/// **Limitation:** if the turn is currently parked on an AskUserQuestion or
/// manual-approval card (off `read_line`), the interrupt isn't observed this
/// turn — the user should dismiss the card instead. The next turn picks up
/// the interrupt slot fresh.
#[tauri::command]
pub async fn agent_interrupt(path: String, state: State<'_, AppState>) -> Result<(), AppError> {
    debug!("agent_interrupt called for path: {path}");
    let repo_path = PathBuf::from(&path);

    // `send` failing means the receiver was already dropped (turn ended
    // between the UI click and here) — treat as a no-op success so the
    // button feels responsive either way.
    if fire_interrupt(&state, &repo_path)? {
        info!("agent_interrupt fired for: {path}");
    } else {
        debug!("agent_interrupt: no in-flight turn for {path} (no-op)");
    }
    Ok(())
}

/// Report whether an agent turn is currently in flight for a project.
///
/// Used by the frontend to reconcile `busy` state after navigating away and
/// back to the Agent page mid-turn. Without it, the unmounted AgentPanel loses
/// the streaming thread entirely; this command lets a freshly-mounted panel
/// honestly report "still working" and poll the transcript until it lands.
#[tauri::command]
pub async fn agent_is_busy(path: String, state: State<'_, AppState>) -> Result<bool, AppError> {
    Ok(project_busy(&state, &PathBuf::from(&path)))
}

// ── Pending-interaction payloads (reconciliation after navigation) ───────────
//
// When an AskUserQuestion or manual-approval request parks the turn, the
// request payload now lives in `AppState.pending_*` alongside the oneshot
// sender (see `PendingQuestion` / `PendingPermission`). These commands expose
// the payload without consuming the sender, so a freshly-mounted AgentPanel —
// whose predecessor's Tauri Channel (and thus the original parking event) was
// lost on unmount — can re-materialize the card.

/// Serializable payload for a pending manual-approval request.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PendingPermissionInfo {
    pub request_id: String,
    pub tool_name: String,
    pub input: String,
}

/// Serializable payload for a pending AskUserQuestion request.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PendingQuestionInfo {
    pub request_id: String,
    pub questions: Vec<crate::agents::AskUserQuestionSpec>,
}

/// Serializable payload for a pending `ExitPlanMode` request.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PendingPlanInfo {
    pub request_id: String,
    pub plan: String,
}

/// One project's pending `AskUserQuestion`, surfaced across the whole registry
/// by `list_pending_questions`. Carries the project `path` (so the frontend can
/// route the answer) alongside the same payload as `PendingQuestionInfo`.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PendingQuestionEntry {
    /// Canonical registered project path — the key into `pending_answers` and
    /// the value the frontend echoes back to `agent_answer_question`.
    pub path: String,
    pub request_id: String,
    pub questions: Vec<crate::agents::AskUserQuestionSpec>,
}

/// One project's pending manual-approval request, surfaced across the whole
/// registry by `list_pending_permissions`. Carries the project `path` (so the
/// frontend can route the verdict) alongside the same payload as
/// `PendingPermissionInfo`.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PendingPermissionEntry {
    /// Canonical registered project path — the key into `pending_permissions`
    /// and the value the frontend echoes back to `agent_answer_permission`.
    pub path: String,
    pub request_id: String,
    pub tool_name: String,
    pub input: String,
}

/// One project's pending `ExitPlanMode` request, surfaced across the whole
/// registry by `list_pending_plans`. Carries the project `path` (so the
/// frontend can route the verdict) alongside the same payload as
/// `PendingPlanInfo`.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PendingPlanEntry {
    /// Canonical registered project path — the key into `pending_plans` and
    /// the value the frontend echoes back to `agent_answer_plan`.
    pub path: String,
    pub request_id: String,
    pub plan: String,
}

/// Collect every project that currently has a pending `AskUserQuestion`, across
/// the whole registry.
///
/// The read loop parks one oneshot per pending question in
/// `AppState.pending_answers`; this walks that map and snapshots every
/// non-empty slot. Used by the frontend's global "stuck prompt" reconciliation
/// (app launch + window focus + manual refresh) so a prompt parked while the
/// user was on another view — or with the Mac locked — is never silently
/// missed. `agent_pending_question` covers the single-project case; this is the
/// cross-project aggregate.
///
/// Fully synchronous and lock-bounded like `agent_pending_question`: each
/// inner slot is locked only long enough to clone the payload, and the outer
/// map guard is dropped before returning.
pub(crate) fn collect_pending_questions(
    pending: &std::sync::Mutex<HashMap<PathBuf, crate::claude_session::QuestionSlot>>,
) -> Result<Vec<PendingQuestionEntry>, AppError> {
    let guard = pending.lock().map_err(|_| AppError::LockError)?;
    let mut out = Vec::new();
    for (path, slot) in guard.iter() {
        // Snapshot the payload inside the slot guard's scope — cloning outside
        // would borrow the temporary guard (cf. `agent_pending_question`).
        let entry = slot.lock().ok().and_then(|g| {
            g.as_ref().map(|p| PendingQuestionEntry {
                path: path.to_string_lossy().into_owned(),
                request_id: p.request_id.clone(),
                questions: p.questions.clone(),
            })
        });
        if let Some(entry) = entry {
            out.push(entry);
        }
    }
    Ok(out)
}

/// Collect every project that currently has a pending manual-approval request,
/// across the whole registry. The permission-side mirror of
/// [`collect_pending_questions`].
///
/// The read loop parks one oneshot per pending approval in
/// `AppState.pending_permissions`; this walks that map and snapshots every
/// non-empty slot. Used by the frontend's global "stuck prompt" reconciliation
/// (app launch + window focus + manual refresh) so an approval parked while the
/// user was on another view — or with the Mac locked — is surfaced rather than
/// missed (parked approvals now wait indefinitely until answered or Stopped,
/// so this is no longer racing a timeout).
/// `agent_pending_permission` covers the single-project case; this is the
/// cross-project aggregate.
///
/// Fully synchronous and lock-bounded like `agent_pending_permission`: each
/// inner slot is locked only long enough to clone the payload, and the outer
/// map guard is dropped before returning.
pub(crate) fn collect_pending_permissions(
    pending: &std::sync::Mutex<HashMap<PathBuf, crate::claude_session::PermissionSlot>>,
) -> Result<Vec<PendingPermissionEntry>, AppError> {
    let guard = pending.lock().map_err(|_| AppError::LockError)?;
    let mut out = Vec::new();
    for (path, slot) in guard.iter() {
        // Snapshot the payload inside the slot guard's scope — cloning outside
        // would borrow the temporary guard (cf. `agent_pending_permission`).
        let entry = slot.lock().ok().and_then(|g| {
            g.as_ref().map(|p| PendingPermissionEntry {
                path: path.to_string_lossy().into_owned(),
                request_id: p.request_id.clone(),
                tool_name: p.tool_name.clone(),
                input: p.input.clone(),
            })
        });
        if let Some(entry) = entry {
            out.push(entry);
        }
    }
    Ok(out)
}

/// Collect every project that currently has a pending `ExitPlanMode` request,
/// across the whole registry. The plan-side mirror of
/// [`collect_pending_permissions`].
///
/// Fully synchronous and lock-bounded like `agent_pending_plan`: each inner
/// slot is locked only long enough to clone the payload, and the outer map
/// guard is dropped before returning.
pub(crate) fn collect_pending_plans(
    pending: &std::sync::Mutex<HashMap<PathBuf, crate::claude_session::PlanSlot>>,
) -> Result<Vec<PendingPlanEntry>, AppError> {
    let guard = pending.lock().map_err(|_| AppError::LockError)?;
    let mut out = Vec::new();
    for (path, slot) in guard.iter() {
        let entry = slot.lock().ok().and_then(|g| {
            g.as_ref().map(|p| PendingPlanEntry {
                path: path.to_string_lossy().into_owned(),
                request_id: p.request_id.clone(),
                plan: p.plan.clone(),
            })
        });
        if let Some(entry) = entry {
            out.push(entry);
        }
    }
    Ok(out)
}

/// Read the pending manual-approval payload for a project, if any.
///
/// Does NOT consume the sender — only the payload. The frontend uses this to
/// re-render the Allow/Deny card after navigating away and back. Returns `None`
/// when no approval is pending.
#[tauri::command]
pub async fn agent_pending_permission(
    path: String,
    state: State<'_, AppState>,
) -> Result<Option<PendingPermissionInfo>, AppError> {
    let repo_path = PathBuf::from(&path);
    let guard = state
        .pending_permissions
        .lock()
        .map_err(|_| AppError::LockError)?;
    Ok(guard.get(&repo_path).and_then(|slot| {
        slot.lock().ok().and_then(|g| {
            g.as_ref().map(|p| PendingPermissionInfo {
                request_id: p.request_id.clone(),
                tool_name: p.tool_name.clone(),
                input: p.input.clone(),
            })
        })
    }))
}

/// Read the pending AskUserQuestion payload for a project, if any.
///
/// Does NOT consume the sender — only the payload. The frontend uses this to
/// re-render the question card after navigating away and back. Returns `None`
/// when no question is pending.
#[tauri::command]
pub async fn agent_pending_question(
    path: String,
    state: State<'_, AppState>,
) -> Result<Option<PendingQuestionInfo>, AppError> {
    let repo_path = PathBuf::from(&path);
    let guard = state
        .pending_answers
        .lock()
        .map_err(|_| AppError::LockError)?;
    Ok(guard.get(&repo_path).and_then(|slot| {
        slot.lock().ok().and_then(|g| {
            g.as_ref().map(|p| PendingQuestionInfo {
                request_id: p.request_id.clone(),
                questions: p.questions.clone(),
            })
        })
    }))
}

/// List every project with a pending `AskUserQuestion`, across the whole
/// registry. The cross-project aggregate of `agent_pending_question`.
///
/// Returns one `PendingQuestionEntry` per parked prompt (path + request_id +
/// the structured questions). Empty when nothing is waiting anywhere. The
/// frontend calls this on app launch, on window focus, and on manual refresh
/// to surface "stuck" prompts the user would otherwise miss (e.g. the question
/// card never rendered because the Mac was locked).
#[tauri::command]
pub async fn list_pending_questions(
    state: State<'_, AppState>,
) -> Result<Vec<PendingQuestionEntry>, AppError> {
    collect_pending_questions(&state.pending_answers)
}

/// List every project with a pending manual-approval request, across the whole
/// registry. The cross-project aggregate of `agent_pending_permission`, and the
/// permission-side mirror of `list_pending_questions`.
///
/// Returns one `PendingPermissionEntry` per parked approval (path + request_id
/// + tool_name + input). Empty when nothing is waiting anywhere. The frontend
/// calls this on app launch, on window focus, and on manual refresh to surface
/// "stuck" approvals the user would otherwise miss — parked while on another
/// view or with the Mac locked, silently auto-denied 10 minutes later.
#[tauri::command]
pub async fn list_pending_permissions(
    state: State<'_, AppState>,
) -> Result<Vec<PendingPermissionEntry>, AppError> {
    collect_pending_permissions(&state.pending_permissions)
}

/// Read the pending `ExitPlanMode` payload for a project, if any.
///
/// Does NOT consume the sender — only the payload. The frontend uses this to
/// re-render the plan-approval card after navigating away and back. Returns
/// `None` when no plan approval is pending.
#[tauri::command]
pub async fn agent_pending_plan(
    path: String,
    state: State<'_, AppState>,
) -> Result<Option<PendingPlanInfo>, AppError> {
    let repo_path = PathBuf::from(&path);
    let guard = state
        .pending_plans
        .lock()
        .map_err(|_| AppError::LockError)?;
    Ok(guard.get(&repo_path).and_then(|slot| {
        slot.lock().ok().and_then(|g| {
            g.as_ref().map(|p| PendingPlanInfo {
                request_id: p.request_id.clone(),
                plan: p.plan.clone(),
            })
        })
    }))
}

/// List every project with a pending `ExitPlanMode` request, across the whole
/// registry. The cross-project aggregate of `agent_pending_plan`, and the
/// plan-side mirror of `list_pending_permissions`.
#[tauri::command]
pub async fn list_pending_plans(
    state: State<'_, AppState>,
) -> Result<Vec<PendingPlanEntry>, AppError> {
    collect_pending_plans(&state.pending_plans)
}

// ── Loop-prompt builder ─────────────────────────────────────────────────────

/// Truncate a prompt for logging — shows the first 200 chars so the log line
/// is useful (which step is being sent) without dumping the entire prompt.
fn truncate_prompt(s: &str) -> String {
    const MAX: usize = 200;
    if s.len() <= MAX {
        s.to_string()
    } else {
        format!("{}…", &s[..MAX])
    }
}

/// Build the prompt that kicks off the next development loop.
///
/// Scans `.loopdeck/loops.md` raw text for the first unchecked `- [ ]` under
/// `## Next Steps` (the structured `memory::parse_loops` flattens checked and
/// unchecked steps together, so we read the raw file here to preserve the
/// distinction). Falls back to a "propose the next loop" prompt when every
/// step is done or there is no `loops.md` yet.
/// Returns `(prompt, title)` — `title` is the raw step text (when one was
/// found) so the history list can show it as the conversation's display
/// name instead of the prompt's generic opening boilerplate.
pub(crate) fn build_next_loop_prompt(path: &Path) -> (String, Option<String>) {
    let next_step = next_unchecked_loop_step(path);

    match next_step {
        Some(step) => (
            format!(
                "You are working on this Selasar project. Use the `loopdeck-orchestrator` \
                 skill conventions. Read `.loopdeck/loops.md` for full context. The next \
                 unchecked step is: \"{step}\". Implement it. When done, update \
                 `.loopdeck/loops.md` (mark the step `[x]`, refresh `## Current`) and \
                 append any architectural decisions to `.loopdeck/decisions.md` per the \
                 memory convention."
            ),
            Some(step),
        ),
        None => (
            String::from(
                "You are working on this Selasar project. Use the `loopdeck-orchestrator` \
                 skill conventions. Review `.loopdeck/loops.md`, then propose and start \
                 the next loop. When done, update `.loopdeck/loops.md` (refresh \
                 `## Current`, add new steps under `## Next Steps`) and append any \
                 architectural decisions to `.loopdeck/decisions.md` per the memory \
                 convention.",
            ),
            None,
        ),
    }
}

/// Scan `.loopdeck/loops.md` for the first unchecked `- [ ]` step under
/// `## Next Steps`. Returns `None` if the file is missing, the section is
/// absent, or every remaining step is a `Review & merge:` bookkeeping
/// reminder (the `loopdeck-open-pr` skill appends those after opening a PR —
/// they're not implementable work, so "start next loop" must skip past them
/// rather than asking an agent to "implement" a merge link).
fn next_unchecked_loop_step(path: &Path) -> Option<String> {
    let content = limits::read_bounded_to_string(
        path.join(".loopdeck").join("loops.md"),
        limits::SPEC_MAX_BYTES,
    )
    .ok()?;

    // Walk to the `## Next Steps` section, then read lines until the next
    // `## ` heading. Return the first `- [ ]` item in that window.
    let mut in_next_steps = false;
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("## ") {
            in_next_steps = trimmed.eq_ignore_ascii_case("## Next Steps");
            continue;
        }
        if in_next_steps {
            if let Some(rest) = trimmed.strip_prefix("- [ ] ") {
                let rest = rest.trim();
                if rest.to_ascii_lowercase().starts_with("review & merge:") {
                    continue;
                }
                return Some(rest.to_string());
            }
        }
    }
    None
}

// ── Transient-error retry wrappers ─────────────────────────────────────────
//
// The `claude` CLI surfaces a gateway `529 overloaded` (or similar transient
// failure) as a normal `Ok(AgentResponse { is_error: true, result: "API Error:
// 529 ..." })` — it doesn't crash. These wrappers re-send the same prompt with
// exponential backoff until the turn succeeds, the error turns out to be
// non-transient (auth, bad request), or `retry::MAX_ATTEMPTS` is exhausted.
// Non-transient errors are returned immediately (retrying won't help) and left
// for the caller's `is_error` check to convert into an `Err`.
//
// Transcript recording stays OUT of these wrappers: the pipeline helpers record
// the user turn once before and the (final) assistant turn once after, so a
// retried turn appears as a single exchange, not N.

/// Send a prompt with retry on transient gateway overload.
///
/// Wraps [`ClaudeSession::send_message`]: loops until a non-retryable outcome
/// (success, non-overload error, or exhausted retries) and returns the final
/// `AgentResponse`. The response may still carry `is_error: true` if every
/// attempt overloaded or the failure was non-transient — the caller decides
/// whether to propagate that as an `Err`.
async fn send_with_retry(
    session: &mut HarnessSession,
    prompt: &str,
    attachments: &[Attachment],
    slots: &ParkSlots<'_>,
    interrupt_slot: &crate::claude_session::InterruptSlot,
) -> Result<AgentResponse, AppError> {
    // `attempt` is the 0-based index of the attempt that just ran. `elapsed_ms`
    // accumulates backoff sleeps so `retry::next_backoff` can enforce both the
    // count bound (MAX_ATTEMPTS) and the wall-clock budget
    // (BACKOFF_TOTAL_BUDGET_MS) in one place.
    let mut attempt: u32 = 0;
    let mut elapsed_ms: u64 = 0;
    loop {
        let response = session
            .send_message(prompt, attachments, slots, interrupt_slot)
            .await?;

        // Done unless this is a retryable transient overload.
        if !(response.is_error && retry::is_overloaded(&response.result)) {
            return Ok(response);
        }

        // Overloaded — back off and retry if attempts + budget remain.
        match retry::next_backoff(attempt, elapsed_ms) {
            Some(delay) => {
                warn!(
                    attempt = attempt + 1,
                    max_attempts = MAX_ATTEMPTS,
                    delay_ms = delay,
                    elapsed_ms,
                    "provider overloaded; retrying in {} ms: {}",
                    delay,
                    response.result.trim(),
                );
                tokio::time::sleep(std::time::Duration::from_millis(delay)).await;
                elapsed_ms = elapsed_ms.saturating_add(delay);
                attempt += 1;
            }
            None => {
                warn!(
                    max_attempts = MAX_ATTEMPTS,
                    elapsed_ms,
                    "provider overloaded; exhausted retries ({} attempts, ~{}ms backoff), surfacing error: {}",
                    attempt + 1,
                    elapsed_ms,
                    response.result.trim(),
                );
                return Ok(response);
            }
        }
    }
}

/// Streaming send with retry on transient gateway overload.
///
/// Wraps [`ClaudeSession::send_message_streaming`]. Between a failed attempt
/// and its retry, emits a [`ClaudeEvent::Retrying`] so the UI can show
/// "Retrying 2/4 in 4s…" — otherwise the frontend would see a terminal
/// `Result{is_error:true}` followed, silently, by a second `Result`. The final
/// `Result` event (success or terminal failure) remains authoritative.
#[allow(clippy::too_many_arguments)]
async fn send_streaming_with_retry(
    session: &mut HarnessSession,
    prompt: &str,
    attachments: &[Attachment],
    channel: &Channel<ClaudeEvent>,
    slots: &ParkSlots<'_>,
    interrupt_slot: &crate::claude_session::InterruptSlot,
    plan_mode: bool,
    token_budget: Option<&TokenBudget>,
) -> Result<AgentResponse, AppError> {
    let mut attempt: u32 = 0;
    let mut elapsed_ms: u64 = 0;
    loop {
        let response = session
            .send_message_streaming(
                prompt,
                attachments,
                channel,
                slots,
                interrupt_slot,
                plan_mode,
                token_budget,
            )
            .await?;

        if !(response.is_error && retry::is_overloaded(&response.result)) {
            return Ok(response);
        }

        match retry::next_backoff(attempt, elapsed_ms) {
            Some(delay) => {
                warn!(
                    attempt = attempt + 1,
                    max_attempts = MAX_ATTEMPTS,
                    delay_ms = delay,
                    elapsed_ms,
                    "provider overloaded [streaming]; retrying in {} ms: {}",
                    delay,
                    response.result.trim(),
                );
                // `attempt` is the 0-based index of the attempt that just
                // failed, so the upcoming retry is the 1-based `attempt + 2`.
                let _ = channel.send(ClaudeEvent::Retrying {
                    attempt: attempt + 2,
                    max_attempts: MAX_ATTEMPTS,
                    backoff_ms: delay,
                    error: response.result.clone(),
                });
                tokio::time::sleep(std::time::Duration::from_millis(delay)).await;
                elapsed_ms = elapsed_ms.saturating_add(delay);
                attempt += 1;
            }
            None => {
                warn!(
                    max_attempts = MAX_ATTEMPTS,
                    elapsed_ms,
                    "provider overloaded [streaming]; exhausted retries ({} attempts, ~{}ms backoff), surfacing error: {}",
                    attempt + 1,
                    elapsed_ms,
                    response.result.trim(),
                );
                return Ok(response);
            }
        }
    }
}

// ── Interruption recovery ──────────────────────────────────────────────────
//
// Each send pipeline records the user turn *before* sending (crash-safety),
// then the assistant turn *after*. A send that returns `Err` — a transport
// failure, child exit, or spawn failure before any `result` event — skips the
// assistant-turn recording, orphaning the user turn. (An `Ok(response)` with
// `is_error: true`, e.g. auth failure or exhausted 529 retries, is NOT an
// orphan: the assistant turn is recorded from it before the error propagates.)
// Reconciling the orphan to a persisted `interrupted` marker keeps the
// transcript truthful instead of leaving a hanging unanswered prompt.

/// On a mid-turn send failure, mark the just-orphaned user turn with a
/// reason-appropriate terminal marker.
///
/// All send failures now map to the generic process-exited marker: the
/// per-park timeout (`ApprovalTimeout` / `QuestionTimeout`) and its distinct
/// "timed out" markers were removed — parked approvals/questions now wait
/// indefinitely until answered or Stopped, so the only send failures that
/// reach here are transport/child/spawn failures (which are genuinely
/// process-exited-shaped). Old transcripts carrying `interrupt_kind:
/// "approval_timeout"` / `"question_timeout"` still render their truthful
/// "timed out" tag (the kinds are kept for backward compatibility).
///
/// Best-effort wrapper over [`conversation::append_terminal_if_orphan`]: a
/// write failure here is only logged, and the caller's original send error
/// still propagates unchanged. Safe to call from the send-failure path because
/// the failed send still holds the per-project lock — no concurrent send can
/// be appending, so the trailing user turn is a genuine orphan, not an
/// in-flight turn mid-window.
pub(crate) fn mark_turn_terminal(path: &Path, _err: &AppError) {
    // Transport failure, child exit, spawn failure — the historical
    // process-exited marker. Startup reconciliation uses the same kind via
    // `reconcile_interrupted`.
    let turn = ConversationTurn::interrupted();
    match conversation::append_terminal_if_orphan(path, &turn) {
        Ok(true) => info!(
            "marked terminal turn in transcript ({}): {}",
            turn.interrupt_kind.as_deref().unwrap_or("interrupted"),
            path.display()
        ),
        Ok(false) => {}
        Err(e) => warn!(
            "failed to reconcile terminal turn for {}: {e}",
            path.display()
        ),
    }
}

/// Shared send pipeline used by `agent_start_loop` and `agent_send_message`.
///
/// Records the user turn to the transcript *before* sending (so a crash
/// mid-turn still captures intent), sends it, records the assistant turn, and
/// returns the structured response. The transcript append is best-effort: a
/// write failure is logged but doesn't fail the turn — the live result still
/// reaches the UI.
/// Read an image file from disk into an inline-base64 [`Attachment`].
///
/// Exists for the composer's drag-and-drop path only. A drop is delivered by
/// Tauri as a *filesystem path*, not as bytes — unlike paste and the file
/// picker, which hand the webview a `File` directly — so something has to read
/// it. (Tauri's native drag-drop is what delivers folder drops to the import
/// screen; switching the window to webview-level HTML5 drops in order to get
/// bytes in the browser would break that, hence this command.)
///
/// Returns the raw file bytes base64-encoded at original size. The caller
/// re-runs the result through the same downscale pipeline that paste uses, so
/// the transcript-size guarantees hold no matter which affordance was used.
#[tauri::command]
pub async fn agent_read_image_attachment(path: String) -> Result<Attachment, AppError> {
    let path = PathBuf::from(&path);

    // Media type is derived from the extension rather than sniffed: the value
    // is echoed to the model as the content block's `media_type`, and the
    // frontend re-encodes anyway, so a mislabelled extension degrades to a
    // decode failure in the browser rather than anything dangerous.
    let media_type = match path
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_ascii_lowercase)
        .as_deref()
    {
        Some("png") => "image/png",
        Some("jpg" | "jpeg") => "image/jpeg",
        Some("gif") => "image/gif",
        Some("webp") => "image/webp",
        _ => {
            return Err(AppError::Agent(String::from(
                "not an image file — drop a PNG, JPEG, GIF, or WebP",
            )))
        }
    };

    // Check the size before reading so a multi-gigabyte file is refused rather
    // than pulled into memory first. The budget is on base64 length, which
    // inflates the raw bytes by 4/3.
    let raw_budget = limits::ATTACHMENT_MAX_BYTES / 4 * 3;
    let len = std::fs::metadata(&path)
        .map_err(|e| AppError::Agent(format!("could not read {}: {e}", path.display())))?
        .len();
    if len as usize > raw_budget {
        return Err(AppError::Limit(format!(
            "image is too large: {len} bytes (max {raw_budget})"
        )));
    }

    let bytes = std::fs::read(&path)
        .map_err(|e| AppError::Agent(format!("could not read {}: {e}", path.display())))?;

    Ok(Attachment {
        media_type: String::from(media_type),
        data: base64_encode(&bytes),
    })
}

/// Standard-alphabet base64 with padding.
///
/// Hand-rolled to avoid pulling in a dependency for ~15 lines used on exactly
/// one path; the `data` field's contract (unwrapped, standard alphabet, no
/// line breaks) is narrow enough that the generality of a crate buys nothing.
fn base64_encode(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b = [
            chunk[0],
            *chunk.get(1).unwrap_or(&0),
            *chunk.get(2).unwrap_or(&0),
        ];
        let n = u32::from(b[0]) << 16 | u32::from(b[1]) << 8 | u32::from(b[2]);
        out.push(ALPHABET[(n >> 18 & 63) as usize] as char);
        out.push(ALPHABET[(n >> 12 & 63) as usize] as char);
        out.push(if chunk.len() > 1 {
            ALPHABET[(n >> 6 & 63) as usize] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            ALPHABET[(n & 63) as usize] as char
        } else {
            '='
        });
    }
    out
}

/// Bound and sanity-check composer attachments at the IPC boundary.
///
/// The frontend already downscales and re-encodes before invoking, so in
/// normal operation nothing here ever trips. It exists because the IPC surface
/// is reachable by anything running in the webview, and everything downstream
/// treats these bytes as trusted: they go inline into the NDJSON line written
/// to the agent's stdin *and* into the on-disk transcript. An unbounded or
/// malformed value would corrupt one or both.
///
/// Only structural properties are checked — size, count, media type, and that
/// the payload is really standard base64 with no embedded newline (which would
/// split the single-line NDJSON turn into two malformed ones). Whether the
/// decoded bytes are a valid image is the model's problem, not ours.
fn validate_attachments(attachments: Vec<Attachment>) -> Result<Vec<Attachment>, AppError> {
    /// Media types the Anthropic content block accepts. A value outside this
    /// set would be rejected by the API mid-turn with a far less obvious
    /// error, so it fails here instead.
    const ALLOWED_MEDIA_TYPES: [&str; 4] = ["image/png", "image/jpeg", "image/gif", "image/webp"];

    if attachments.len() > limits::ATTACHMENTS_MAX_COUNT {
        return Err(AppError::Limit(format!(
            "too many attachments: {} (max {})",
            attachments.len(),
            limits::ATTACHMENTS_MAX_COUNT
        )));
    }

    let mut total = 0usize;
    for attachment in &attachments {
        if !ALLOWED_MEDIA_TYPES.contains(&attachment.media_type.as_str()) {
            return Err(AppError::Limit(format!(
                "unsupported attachment media type: {} (expected one of {})",
                attachment.media_type,
                ALLOWED_MEDIA_TYPES.join(", ")
            )));
        }
        if attachment.data.len() > limits::ATTACHMENT_MAX_BYTES {
            return Err(AppError::Limit(format!(
                "attachment is too large: {} bytes of base64 (max {})",
                attachment.data.len(),
                limits::ATTACHMENT_MAX_BYTES
            )));
        }
        // Rejects whitespace-wrapped base64 (some encoders line-wrap at 76
        // chars) as well as a `data:` URI prefix that was never stripped.
        if !attachment
            .data
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'+' || b == b'/' || b == b'=')
        {
            return Err(AppError::Limit(String::from(
                "attachment data is not unwrapped standard base64 (strip any data: URI prefix and line breaks)",
            )));
        }
        total = total.saturating_add(attachment.data.len());
    }

    if total > limits::ATTACHMENTS_MAX_TOTAL_BYTES {
        return Err(AppError::Limit(format!(
            "attachments total {} bytes of base64 (max {})",
            total,
            limits::ATTACHMENTS_MAX_TOTAL_BYTES
        )));
    }

    Ok(attachments)
}

async fn send_and_record(
    state: &AppState,
    path: &Path,
    prompt: &str,
    attachments: &[Attachment],
) -> Result<AgentResponse, AppError> {
    let session_arc = with_session(state, path).await?;
    let mut session = session_arc.lock().await;
    let qslot = question_slot(state, path)?;
    let pslot = permission_slot(state, path)?;
    let plnslot = plan_slot(state, path)?;
    let islot = interrupt_slot(state, path)?;
    let slots = ParkSlots {
        question: &qslot,
        permission: &pslot,
        plan: &plnslot,
    };

    // 1. Record the user turn first (crash-safety: intent survives).
    if let Err(e) = conversation::append_turn(
        path,
        &ConversationTurn::user(prompt).with_attachments(attachments.to_vec()),
    ) {
        tracing::warn!("failed to append user turn to transcript: {e}");
    }

    // 2. Send + receive (with retry on transient 529 overload). The `Err` arm
    //    here is a transport/child/spawn failure *before* any result event —
    //    distinct from an `Ok(is_error)` outcome (auth failure, exhausted 529
    //    retries), which records a real assistant turn at step 3 and so is not
    //    an orphan. The `Err` path skips that recording, leaving the user turn
    //    from step 1 dangling; reconcile it to a truthful `interrupted` state
    //    before re-propagating so the transcript never hangs unanswered.
    let response = match send_with_retry(&mut session, prompt, attachments, &slots, &islot).await {
        Ok(r) => r,
        Err(e) => {
            mark_turn_terminal(path, &e);
            return Err(e);
        }
    };

    // 3. Record the assistant turn (best-effort). Done BEFORE the error check
    //    below so a failed turn (e.g. "Not logged in") still lands in the
    //    transcript — the user sees the error bubble AND its session_id is
    //    captured for a potential resume after they fix auth. Includes the
    //    model's thinking chain and tool calls so the transcript records how
    //    the answer was reached, not just the final summary.
    let assistant_turn = ConversationTurn::assistant(
        response.result.clone(),
        response.session_id.clone(),
        response.is_error,
        response.usage.clone(),
        response.duration_ms,
        response.thinking.clone(),
        response.tool_calls.clone(),
        response.blocks.clone(),
        response.tasks.clone(),
    );
    if let Err(e) = conversation::append_turn(path, &assistant_turn) {
        tracing::warn!("failed to append assistant turn to transcript: {e}");
    }

    // 4. Propagate claude-level errors as a real Err. claude doesn't crash on
    //    auth/config failures — it completes the stream with `is_error: true`
    //    and a human-readable `result` (e.g. "Not logged in · Please run
    //    /login"). Converting that to an AppError here makes the failure
    //    un-ignorable at the IPC boundary: every caller surfaces it instead of
    //    silently treating the turn as a success.
    if response.is_error {
        return Err(AppError::Agent(response.result.clone().trim().to_string()));
    }

    Ok(response)
}

/// Streaming variant of `send_and_record`.
///
/// Records the user turn, sends via `send_message_streaming` (which emits
/// per-block `ClaudeEvent`s on the channel as they arrive), then records the
/// assistant turn from the returned `AgentResponse`. The channel carries the
/// terminal `ClaudeEvent::Result` so the frontend gets the full aggregated
/// response inline — the return value here is just for transcript recording.
async fn send_and_record_streaming(
    state: &AppState,
    path: &Path,
    prompt: &str,
    attachments: &[Attachment],
    channel: &Channel<ClaudeEvent>,
    plan_mode: bool,
) -> Result<(), AppError> {
    let session_arc = with_session(state, path).await?;
    let mut session = session_arc.lock().await;
    let qslot = question_slot(state, path)?;
    let pslot = permission_slot(state, path)?;
    let plnslot = plan_slot(state, path)?;
    let islot = interrupt_slot(state, path)?;
    let slots = ParkSlots {
        question: &qslot,
        permission: &pslot,
        plan: &plnslot,
    };

    // 1. Record the user turn first (crash-safety: intent survives).
    if let Err(e) = conversation::append_turn(
        path,
        &ConversationTurn::user(prompt).with_attachments(attachments.to_vec()),
    ) {
        tracing::warn!("failed to append user turn to transcript: {e}");
    }

    // 2. Send + stream (with retry on transient 529 overload). See the
    //    non-streaming pipelines for the interruption-recovery rationale: a
    //    transport/child failure (`Err`) orphans the user turn from step 1, so
    //    we reconcile it to `interrupted` before re-propagating the error.
    let response = match send_streaming_with_retry(
        &mut session,
        prompt,
        attachments,
        channel,
        &slots,
        &islot,
        plan_mode,
        None,
    )
    .await
    {
        Ok(r) => r,
        Err(e) => {
            mark_turn_terminal(path, &e);
            return Err(e);
        }
    };

    // 3. Record the assistant turn (best-effort). Includes thinking + tool
    //    calls so the persisted transcript captures the full reasoning trail,
    //    not just the final summary text.
    let assistant_turn = ConversationTurn::assistant(
        response.result.clone(),
        response.session_id.clone(),
        response.is_error,
        response.usage.clone(),
        response.duration_ms,
        response.thinking.clone(),
        response.tool_calls.clone(),
        response.blocks.clone(),
        response.tasks.clone(),
    );
    if let Err(e) = conversation::append_turn(path, &assistant_turn) {
        tracing::warn!("failed to append assistant turn to transcript: {e}");
    }

    Ok(())
}

// ── Fresh-start pipeline (agent_start_loop[_streaming]) ────────────────────
//
// Start always begins a NEW conversation: drop any live session, archive the
// transcript, spawn a fresh claude process WITHOUT `--resume`. This is the
// deliberate contrast with `with_session` (used by `agent_send_message`),
// which reuses the live process or `--resume`s after a restart.
//
// Concurrency: Start uses `try_lock` — if a turn is already in flight on this
// project, the start is rejected immediately ("agent is busy") instead of
// queueing. The successful `try_lock` is also the proof that the prior session
// is idle and therefore safe to drop and replace mid-map. Send, by contrast,
// uses `lock().await` and queues.

/// Force-spawn a fresh `ClaudeSession` for `path`, replacing any live one.
///
/// 1. `try_lock` the existing arc (if any). `Err` ⇒ a turn is in flight on the
///    current session → reject with "agent is busy" (Start must not interrupt a
///    running turn). `Ok` ⇒ the old session is provably idle.
/// 2. Drop the old arc from the map (last reference drops → `Drop` closes stdin
///    → claude exits → child reaped).
/// 3. `archive_conversation` — rotate `active.jsonl` aside so the new
///    conversation begins fresh.
/// 4. Spawn a NEW `ClaudeSession` with `resume_session_id = None` (never resume
///    on Start), insert its arc into the map, and return the arc.
///
/// Returns an owned `Arc` (mirroring `with_session`'s contract) so the caller
/// `.lock().await`s it. The busy-check `try_lock` in phase 1 only proves the
/// *old* session was idle; between then and the caller taking the new arc's
/// lock, a concurrent producer could in principle race — but the only producers
/// are Start/Send, and the frontend disables both while a turn is in flight, so
/// the race window is closed in practice for this single-user app.
async fn spawn_fresh(
    state: &AppState,
    path: &Path,
    policy_root: &Path,
    force_autonomous: bool,
) -> Result<Arc<tokio::sync::Mutex<HarnessSession>>, AppError> {
    // ── Phase 1: try_lock the existing arc to prove it's idle. ──
    // Scoped so the map's std Mutex guard is dropped before we await anything.
    {
        let map_guard = state
            .claude_sessions
            .lock()
            .map_err(|_| AppError::LockError)?;
        if let Some(arc) = map_guard.get(path) {
            // Try to acquire the per-project turn lock non-blockingly. Holding
            // the std Mutex here is fine — try_lock is synchronous, no .await.
            if arc.try_lock().is_err() {
                return Err(AppError::Agent(
                    "agent is busy — wait for the current turn to finish before starting a new conversation".into(),
                ));
            }
            // try_lock succeeded ⇒ idle. The guard drops here; we proceed to
            // replace the session below.
        }
    }

    // ── Phase 2: drop the old arc from the map (reaps the child via Drop). ──
    let dropped = state
        .claude_sessions
        .lock()
        .map_err(|_| AppError::LockError)?
        .remove(path);
    if dropped.is_some() {
        debug!(
            "dropped live claude session for fresh start: {}",
            path.display()
        );
    }

    // ── Phase 3: archive the transcript (rotate active.jsonl aside). ──
    // Surfaced as a real error: the user asked for a fresh conversation and
    // didn't get one.
    // A run may execute in an isolated worktree while its user-visible
    // transcript belongs to the registered project. Archive the latter so
    // the Agent surface can show the unattended turn instead of hiding it in
    // the worktree's private `.loopdeck` directory.
    conversation::archive_conversation(policy_root)?;

    let agent_config = resolve_agent_config(state)?;
    spawn_fresh_with_config(state, path, policy_root, &agent_config, force_autonomous)
}

/// The fresh-session primitive with an already resolved profile. Multi-agent
/// runs resolve every roster entry before worktrees are spawned, then use this
/// function so a later settings edit cannot change an in-flight sub-run.
pub(crate) fn spawn_fresh_with_config(
    state: &AppState,
    path: &Path,
    policy_root: &Path,
    agent_config: &crate::config::AgentConfig,
    force_autonomous: bool,
) -> Result<Arc<tokio::sync::Mutex<HarnessSession>>, AppError> {
    // The caller has already performed the busy check / archive steps when it
    // is the normal `spawn_fresh` path. Multi-agent worktrees are unique, so
    // their maps cannot collide with an existing session.
    let policy = if force_autonomous {
        PermissionPolicy::with_mode(PermissionMode::Autonomous)
    } else {
        resolve_permission_policy(state, policy_root)
    };

    let session = HarnessSession::spawn(path, agent_config, None, policy)?;
    let arc = Arc::new(tokio::sync::Mutex::new(session));
    state
        .claude_sessions
        .lock()
        .map_err(|_| AppError::LockError)?
        .insert(path.to_path_buf(), Arc::clone(&arc));

    Ok(arc)
}

/// Fresh-start send pipeline used by `agent_start_loop`. Mirrors
/// `send_and_record` but spawns a brand-new session (no `--resume`) and rejects
/// when busy instead of queueing. See `spawn_fresh` for the lifecycle.
///
/// `pub(crate)` so the `prd-run-queue` executor (`commands::run_queue`) can
/// drive one orchestrated turn per queued phase through the same
/// retry/transcript/park-slot pipeline a human-initiated "Start Loop" uses —
/// an unattended phase run and a manual one are the same primitive, run
/// without a human clicking the button each time.
pub(crate) async fn start_fresh_and_record(
    state: &AppState,
    path: &Path,
    prompt: &str,
    title: Option<String>,
) -> Result<AgentResponse, AppError> {
    let session_arc = spawn_fresh(state, path, path, false).await?;
    let mut session = session_arc.lock().await;
    let qslot = question_slot(state, path)?;
    let pslot = permission_slot(state, path)?;
    let plnslot = plan_slot(state, path)?;
    let islot = interrupt_slot(state, path)?;
    let slots = ParkSlots {
        question: &qslot,
        permission: &pslot,
        plan: &plnslot,
    };

    // 1. Record the user turn first (crash-safety: intent survives).
    //    Marked `user_loop` — this prompt was auto-built from
    //    `.loopdeck/loops.md` by `build_next_loop_prompt`, not typed by the
    //    human. The UI renders these as compact system rows instead of
    //    user chat bubbles so they don't drown out real messages.
    if let Err(e) = conversation::append_turn(path, &ConversationTurn::user_loop(prompt, title)) {
        tracing::warn!("failed to append user turn to transcript: {e}");
    }

    tracing::info!(
        "sending loop prompt ({} chars) to claude: {:?}",
        prompt.len(),
        truncate_prompt(prompt),
    );

    // 2. Send + receive (with retry on transient 529 overload). The `Err` arm
    //    here is a transport/child/spawn failure *before* any result event —
    //    distinct from an `Ok(is_error)` outcome (auth failure, exhausted 529
    //    retries), which records a real assistant turn at step 3 and so is not
    //    an orphan. The `Err` path skips that recording, leaving the user turn
    //    from step 1 dangling; reconcile it to a truthful `interrupted` state
    //    before re-propagating so the transcript never hangs unanswered.
    //
    //    No attachments: this is the auto-built loop prompt, which is text by
    //    construction (there is no composer in the loop path to attach from).
    let response = match send_with_retry(&mut session, prompt, &[], &slots, &islot).await {
        Ok(r) => r,
        Err(e) => {
            mark_turn_terminal(path, &e);
            return Err(e);
        }
    };

    // 3. Record the assistant turn (best-effort, includes thinking + tool calls).
    let assistant_turn = ConversationTurn::assistant(
        response.result.clone(),
        response.session_id.clone(),
        response.is_error,
        response.usage.clone(),
        response.duration_ms,
        response.thinking.clone(),
        response.tool_calls.clone(),
        response.blocks.clone(),
        response.tasks.clone(),
    );
    if let Err(e) = conversation::append_turn(path, &assistant_turn) {
        tracing::warn!("failed to append assistant turn to transcript: {e}");
    }

    // 4. Propagate claude-level errors (is_error: true) as a real Err so every
    //    caller surfaces them instead of silently treating the turn as success.
    if response.is_error {
        return Err(AppError::Agent(response.result.clone().trim().to_string()));
    }

    Ok(response)
}

/// Streaming variant of `start_fresh_and_record`, used by
/// `agent_start_loop_streaming`. Same fresh-start + reject-when-busy semantics.
/// `pub(crate)` (rather than private) because `commands::run_queue`'s
/// executor reuses this pipeline too (Phase 4) — a streaming send is what
/// lets a mid-run `AskUserQuestion`/permission/plan card actually park
/// instead of being auto-denied like the non-streaming `send_message` path
/// does when it has no channel to surface a card on.
pub(crate) async fn start_fresh_and_record_streaming(
    state: &AppState,
    path: &Path,
    prompt: &str,
    title: Option<String>,
    channel: &Channel<ClaudeEvent>,
) -> Result<AgentResponse, AppError> {
    start_fresh_and_record_streaming_in_root(
        state,
        path,
        path,
        prompt,
        title,
        channel,
        StreamingRunOptions::default(),
    )
    .await
}

#[derive(Default)]
pub(crate) struct StreamingRunOptions<'a> {
    pub(crate) token_budget: Option<&'a TokenBudget>,
    pub(crate) force_autonomous: bool,
}

/// Streaming fresh turn whose process/transcript live in `path`, but whose
/// project-facing state (permission tier, pending-card slots, and interrupts)
/// belongs to the registered project `policy_root`. Unattended runs use this
/// to execute inside an isolated worktree without silently falling back from
/// the configured policy or hiding parked cards under an unregistered path.
pub(crate) async fn start_fresh_and_record_streaming_in_root(
    state: &AppState,
    path: &Path,
    policy_root: &Path,
    prompt: &str,
    title: Option<String>,
    channel: &Channel<ClaudeEvent>,
    options: StreamingRunOptions<'_>,
) -> Result<AgentResponse, AppError> {
    start_fresh_and_record_streaming_in_root_with_config(
        state,
        path,
        policy_root,
        prompt,
        title,
        channel,
        options.token_budget,
        None,
        None,
        options.force_autonomous,
    )
    .await
}

/// Profile-pinned counterpart of [`start_fresh_and_record_streaming_in_root`].
/// `agent_config` is intentionally owned by the caller's run snapshot; it is
/// never re-read from global settings after a multi-agent loop has begun.
#[allow(clippy::too_many_arguments)]
pub(crate) async fn start_fresh_and_record_streaming_in_root_with_config(
    state: &AppState,
    path: &Path,
    policy_root: &Path,
    prompt: &str,
    title: Option<String>,
    channel: &Channel<ClaudeEvent>,
    token_budget: Option<&TokenBudget>,
    agent_config: Option<&crate::config::AgentConfig>,
    // The key used for cached sessions and interactive control slots. Legacy
    // worktree callers retain `policy_root`; multi-agent workers pass their
    // own linked worktree so sibling controls can never collide.
    control_key: Option<&Path>,
    force_autonomous: bool,
) -> Result<AgentResponse, AppError> {
    let session_arc = match agent_config {
        Some(config) => {
            spawn_fresh_with_config(state, path, policy_root, config, force_autonomous)?
        }
        None => spawn_fresh(state, path, policy_root, force_autonomous).await?,
    };
    let mut session = session_arc.lock().await;
    let control_key = control_key.unwrap_or(policy_root);
    let qslot = question_slot(state, control_key)?;
    let pslot = permission_slot(state, control_key)?;
    let plnslot = plan_slot(state, control_key)?;
    let islot = interrupt_slot(state, control_key)?;
    let slots = ParkSlots {
        question: &qslot,
        permission: &pslot,
        plan: &plnslot,
    };

    // 1. Record the user turn first (crash-safety: intent survives).
    //    Marked `user_loop` (see `start_fresh_and_record` for rationale).
    if let Err(e) =
        conversation::append_turn(policy_root, &ConversationTurn::user_loop(prompt, title))
    {
        tracing::warn!("failed to append user turn to transcript: {e}");
    }

    tracing::info!(
        "sending loop prompt ({} chars) to claude [streaming]: {:?}",
        prompt.len(),
        truncate_prompt(prompt),
    );

    // 2. Send + stream (with retry on transient 529 overload). See the
    //    non-streaming pipelines for the interruption-recovery rationale: a
    //    transport/child failure (`Err`) orphans the user turn from step 1, so
    //    we reconcile it to `interrupted` before re-propagating the error.
    // Start never runs under plan mode — it's the auto-built next-loop prompt,
    // not a human follow-up with the composer's Plan-mode toggle.
    // No attachments — the loop prompt is text by construction (see the
    // non-streaming fresh-start path).
    let response = match send_streaming_with_retry(
        &mut session,
        prompt,
        &[],
        channel,
        &slots,
        &islot,
        false,
        token_budget,
    )
    .await
    {
        Ok(r) => r,
        Err(e) => {
            mark_turn_terminal(policy_root, &e);
            return Err(e);
        }
    };

    // 3. Record the assistant turn (best-effort, includes thinking + tool calls).
    let assistant_turn = ConversationTurn::assistant(
        response.result.clone(),
        response.session_id.clone(),
        response.is_error,
        response.usage.clone(),
        response.duration_ms,
        response.thinking.clone(),
        response.tool_calls.clone(),
        response.blocks.clone(),
        response.tasks.clone(),
    );
    if let Err(e) = conversation::append_turn(policy_root, &assistant_turn) {
        tracing::warn!("failed to append assistant turn to transcript: {e}");
    }

    // 4. Propagate claude-level errors as a real Err, mirroring `send_and_record`
    //    (its step 4). claude completes the stream with `is_error: true` on
    //    API/auth failures (e.g. a 429 rate limit) instead of crashing; passing
    //    Ok through would let headless callers (multi-agent workers, night runs)
    //    persist the run as done despite the failure.
    if response.is_error {
        return Err(AppError::Agent(response.result.clone().trim().to_string()));
    }

    Ok(response)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn attachment(data: &str) -> Attachment {
        Attachment {
            media_type: String::from("image/png"),
            data: String::from(data),
        }
    }

    /// Vectors from RFC 4648 §10, which exercise all three padding cases.
    #[test]
    fn base64_encode_matches_rfc4648_vectors() {
        assert_eq!(base64_encode(b""), "");
        assert_eq!(base64_encode(b"f"), "Zg==");
        assert_eq!(base64_encode(b"fo"), "Zm8=");
        assert_eq!(base64_encode(b"foo"), "Zm9v");
        assert_eq!(base64_encode(b"foob"), "Zm9vYg==");
        assert_eq!(base64_encode(b"fooba"), "Zm9vYmE=");
        assert_eq!(base64_encode(b"foobar"), "Zm9vYmFy");
    }

    /// Exercises the `+` and `/` end of the alphabet, which the ASCII-only
    /// vectors above never reach.
    #[test]
    fn base64_encode_covers_the_high_alphabet() {
        assert_eq!(base64_encode(&[0xfb, 0xff, 0xfe]), "+//+");
        assert_eq!(base64_encode(&[0x00, 0x00, 0x00]), "AAAA");
    }

    /// The drag-and-drop path end to end on the backend side: a real file on
    /// disk in, a validated attachment out.
    #[tokio::test]
    async fn read_image_attachment_encodes_a_file_from_disk() {
        let dir = std::env::temp_dir().join(format!("loopdeck-attach-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("shot.png");
        std::fs::write(&file, b"foobar").unwrap();

        let attachment = agent_read_image_attachment(file.to_string_lossy().into_owned())
            .await
            .unwrap();

        assert_eq!(attachment.media_type, "image/png");
        assert_eq!(attachment.data, "Zm9vYmFy");
        // The result must survive the same boundary a pasted image crosses.
        validate_attachments(vec![attachment]).unwrap();
        std::fs::remove_dir_all(&dir).ok();
    }

    #[tokio::test]
    async fn read_image_attachment_rejects_a_non_image_extension() {
        let dir = std::env::temp_dir().join(format!("loopdeck-attach-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("notes.txt");
        std::fs::write(&file, b"hello").unwrap();

        let err = agent_read_image_attachment(file.to_string_lossy().into_owned())
            .await
            .unwrap_err();

        assert!(matches!(err, AppError::Agent(_)), "got {err:?}");
        std::fs::remove_dir_all(&dir).ok();
    }

    /// Extension casing comes from the OS, not from us.
    #[tokio::test]
    async fn read_image_attachment_accepts_uppercase_extensions() {
        let dir = std::env::temp_dir().join(format!("loopdeck-attach-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("PHOTO.JPG");
        std::fs::write(&file, b"foo").unwrap();

        let attachment = agent_read_image_attachment(file.to_string_lossy().into_owned())
            .await
            .unwrap();

        assert_eq!(attachment.media_type, "image/jpeg");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn validate_accepts_ordinary_attachments() {
        let ok = validate_attachments(vec![attachment("Zm9vYmFy")]).unwrap();
        assert_eq!(ok.len(), 1);
    }

    #[test]
    fn validate_rejects_unsupported_media_type() {
        let err = validate_attachments(vec![Attachment {
            media_type: String::from("image/svg+xml"),
            data: String::from("Zm9v"),
        }])
        .unwrap_err();
        assert!(matches!(err, AppError::Limit(_)), "got {err:?}");
    }

    /// A `data:` prefix or line-wrapped base64 would corrupt the single-line
    /// NDJSON turn, so it must not reach the session layer.
    #[test]
    fn validate_rejects_non_bare_base64() {
        let wrapped = validate_attachments(vec![attachment("Zm9v\nYmFy")]).unwrap_err();
        assert!(matches!(wrapped, AppError::Limit(_)), "got {wrapped:?}");

        let prefixed =
            validate_attachments(vec![attachment("data:image/png;base64,Zm9v")]).unwrap_err();
        assert!(matches!(prefixed, AppError::Limit(_)), "got {prefixed:?}");
    }

    #[test]
    fn validate_rejects_too_many_attachments() {
        let many = vec![attachment("Zm9v"); limits::ATTACHMENTS_MAX_COUNT + 1];
        let err = validate_attachments(many).unwrap_err();
        assert!(matches!(err, AppError::Limit(_)), "got {err:?}");
    }

    #[test]
    fn validate_rejects_an_oversized_single_attachment() {
        let huge = attachment(&"A".repeat(limits::ATTACHMENT_MAX_BYTES + 1));
        let err = validate_attachments(vec![huge]).unwrap_err();
        assert!(matches!(err, AppError::Limit(_)), "got {err:?}");
    }

    /// Each image can be under the per-image cap while the turn as a whole
    /// still writes an unreasonable NDJSON line.
    #[test]
    fn validate_rejects_an_oversized_total_across_attachments() {
        let each = attachment(&"A".repeat(limits::ATTACHMENT_MAX_BYTES));
        let err = validate_attachments(vec![each; 4]).unwrap_err();
        assert!(matches!(err, AppError::Limit(_)), "got {err:?}");
    }

    #[test]
    fn next_unchecked_step_finds_first_unchecked() {
        let dir = std::env::temp_dir().join(format!("loopdeck-prompt-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(dir.join(".loopdeck")).unwrap();
        std::fs::write(
            dir.join(".loopdeck").join("loops.md"),
            "# Loops\n\n## Current\n\n_No active loop._\n\n## Next Steps\n\
             - [x] Done thing\n\
             - [ ] First open step\n\
             - [ ] Second open step\n\n## History\n",
        )
        .unwrap();

        let step = next_unchecked_loop_step(&dir);
        assert_eq!(step.as_deref(), Some("First open step"));

        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn next_unchecked_step_none_when_all_done() {
        let dir = std::env::temp_dir().join(format!("loopdeck-prompt-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(dir.join(".loopdeck")).unwrap();
        std::fs::write(
            dir.join(".loopdeck").join("loops.md"),
            "# Loops\n\n## Next Steps\n- [x] one\n- [x] two\n",
        )
        .unwrap();

        assert!(next_unchecked_loop_step(&dir).is_none());

        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn next_unchecked_step_skips_review_and_merge_reminders() {
        let dir = std::env::temp_dir().join(format!("loopdeck-prompt-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(dir.join(".loopdeck")).unwrap();
        std::fs::write(
            dir.join(".loopdeck").join("loops.md"),
            "# Loops\n\n## Next Steps\n\
             - [ ] Review & merge: https://github.com/foo/bar/pull/1\n\
             - [ ] Actually implement the thing\n\n## History\n",
        )
        .unwrap();

        let step = next_unchecked_loop_step(&dir);
        assert_eq!(step.as_deref(), Some("Actually implement the thing"));

        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn next_unchecked_step_none_when_only_review_reminders_left() {
        let dir = std::env::temp_dir().join(format!("loopdeck-prompt-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(dir.join(".loopdeck")).unwrap();
        std::fs::write(
            dir.join(".loopdeck").join("loops.md"),
            "# Loops\n\n## Next Steps\n\
             - [ ] Review & merge: https://github.com/foo/bar/pull/1\n\
             - [ ] Review & merge: https://github.com/foo/bar/pull/2\n",
        )
        .unwrap();

        assert!(next_unchecked_loop_step(&dir).is_none());

        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn next_unchecked_step_none_when_no_file() {
        let dir = std::env::temp_dir().join(format!("loopdeck-prompt-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        assert!(next_unchecked_loop_step(&dir).is_none());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn build_prompt_uses_step_when_present() {
        let dir = std::env::temp_dir().join(format!("loopdeck-prompt-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(dir.join(".loopdeck")).unwrap();
        std::fs::write(
            dir.join(".loopdeck").join("loops.md"),
            "## Next Steps\n- [ ] Wire up the thing\n",
        )
        .unwrap();

        let (prompt, title) = build_next_loop_prompt(&dir);
        assert!(prompt.contains("Wire up the thing"));
        assert!(prompt.contains("loopdeck-orchestrator"));
        assert_eq!(title.as_deref(), Some("Wire up the thing"));

        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn build_prompt_falls_back_when_no_step() {
        let dir = std::env::temp_dir().join(format!("loopdeck-prompt-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();

        let (prompt, title) = build_next_loop_prompt(&dir);
        assert!(prompt.contains("propose and start"));
        assert!(!prompt.contains("next unchecked step is"));
        assert!(title.is_none());

        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// `collect_pending_questions` is the pure core of the
    /// `list_pending_questions` command. Plant a slot directly into a fresh
    /// map (no AppState needed) and assert it round-trips path + payload, and
    /// that an empty slot is skipped.
    #[test]
    fn collect_pending_questions_reports_planted_and_skips_empty() {
        use crate::agents::{AskUserQuestionOption, AskUserQuestionSpec};
        use crate::claude_session::{PendingQuestion, QuestionSlot};
        use std::sync::{Arc, Mutex};

        let path_a = PathBuf::from("/repo/a");
        let path_b = PathBuf::from("/repo/b");
        let slot_a: QuestionSlot = Arc::new(Mutex::new(Some(PendingQuestion {
            request_id: "req-a".into(),
            questions: vec![AskUserQuestionSpec {
                question: "Which frontend?".into(),
                header: "Frontend".into(),
                options: vec![AskUserQuestionOption {
                    label: "HTMX".into(),
                    description: "Go-native".into(),
                }],
                multi_select: false,
            }],
            sender: None,
        })));
        // path_b's slot exists but is empty (no question parked) — must be skipped.
        let slot_b: QuestionSlot = Arc::new(Mutex::new(None));

        let mut map: HashMap<PathBuf, QuestionSlot> = HashMap::new();
        map.insert(path_a.clone(), slot_a);
        map.insert(path_b, slot_b);
        let pending = Mutex::new(map);

        let mut entries = collect_pending_questions(&pending).unwrap();
        assert_eq!(entries.len(), 1, "only the planted slot should be reported");
        let entry = entries.remove(0);
        assert_eq!(entry.path, path_a.to_string_lossy());
        assert_eq!(entry.request_id, "req-a");
        assert_eq!(entry.questions.len(), 1);
        assert_eq!(entry.questions[0].header, "Frontend");

        // Clearing the planted slot yields an empty list.
        {
            let m = pending.lock().unwrap();
            m.get(&path_a).unwrap().lock().unwrap().take();
        }
        let entries = collect_pending_questions(&pending).unwrap();
        assert!(entries.is_empty(), "empty slots must not be reported");
    }
}
