//! Durable, worktree-isolated execution for one logical loop assigned to
//! several named agent profiles. The manifest is intentionally the source of
//! truth: process handles disappear on restart, while a run record remains
//! inspectable and can be reconciled without guessing what an old child did.

use crate::agents::ClaudeEvent;
use crate::commands::agent::{
    build_next_loop_prompt, start_fresh_and_record_streaming_in_root_with_config,
};
use crate::commands::state::{
    acquire_multi_agent_run, fire_interrupt, multi_agent_manifest_lock, release_multi_agent_run,
    resolve_agent_config_by_id, resolve_root, AppState,
};
use crate::config::{AgentConfig, AgentHarness};
use crate::error::AppError;
use crate::git;
use crate::paths;
use crate::persist;
use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tauri::ipc::{Channel, InvokeResponseBody};
use tauri::{AppHandle, Manager, State};
use tokio::sync::Mutex as AsyncMutex;
use tokio::task::JoinSet;
use uuid::Uuid;

const RUNS_DIR: &str = "agent-runs";
const MANIFEST_FILE: &str = "manifest.json";

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum MultiAgentRunStatus {
    Queued,
    Running,
    Waiting,
    Done,
    Failed,
    Cancelled,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MultiAgentSubRun {
    pub id: String,
    pub agent_id: String,
    pub agent_name: String,
    pub harness: AgentHarness,
    pub model: Option<String>,
    pub status: MultiAgentRunStatus,
    pub branch: Option<String>,
    pub worktree: Option<PathBuf>,
    pub result: Option<String>,
    pub error: Option<String>,
    pub started_at: Option<String>,
    pub completed_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MultiAgentRun {
    pub id: String,
    pub path: PathBuf,
    pub loop_id: Option<String>,
    pub status: MultiAgentRunStatus,
    pub started_at: String,
    pub completed_at: Option<String>,
    pub sub_runs: Vec<MultiAgentSubRun>,
}

/// A stream event attributed to an immutable config id and logical run. A
/// snapshot accompanies lifecycle changes so the renderer can recover after a
/// dropped event without inferring status from token output.
#[derive(Debug, Clone, Serialize)]
pub struct MultiAgentEvent {
    pub run_id: String,
    pub agent_id: String,
    /// The normalized Claude/Codex event payload. It remains JSON here because
    /// Tauri's relay `Channel::new` receives an already-serialized IPC body.
    /// Its wire shape is the existing `ClaudeEvent` union.
    pub event: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sub_run: Option<MultiAgentSubRun>,
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MultiAgentControlAction {
    Interrupt,
    Retry,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct RunManifest {
    #[serde(flatten)]
    run: MultiAgentRun,
    prompt: String,
    title: Option<String>,
}

#[derive(Clone)]
struct ResolvedAssignment {
    sub_run: MultiAgentSubRun,
    config: AgentConfig,
}

fn now() -> String {
    Utc::now().to_rfc3339()
}

fn validate_uuid(kind: &str, value: &str) -> Result<(), AppError> {
    Uuid::parse_str(value)
        .map(|_| ())
        .map_err(|_| AppError::InvalidPath(format!("{kind} is not a valid UUID")))
}

fn manifest_path(root: &Path, run_id: &str, must_exist: bool) -> Result<PathBuf, AppError> {
    validate_uuid("multi-agent run id", run_id)?;
    paths::resolve_within(
        root,
        &format!(".loopdeck/{RUNS_DIR}/{run_id}/{MANIFEST_FILE}"),
        must_exist,
    )
}

fn transcript_path(root: &Path, run_id: &str, agent_id: &str) -> Result<PathBuf, AppError> {
    validate_uuid("multi-agent run id", run_id)?;
    validate_uuid("agent config id", agent_id)?;
    paths::resolve_within(
        root,
        &format!(".loopdeck/{RUNS_DIR}/{run_id}/{agent_id}.jsonl"),
        false,
    )
}

fn save_manifest(root: &Path, manifest: &RunManifest) -> Result<(), AppError> {
    let content = serde_json::to_string_pretty(manifest).map_err(|error| {
        AppError::RunPlan(format!("could not encode multi-agent manifest: {error}"))
    })?;
    persist::atomic_write(&manifest_path(root, &manifest.run.id, false)?, &content)?;
    Ok(())
}

fn load_manifest(root: &Path, run_id: &str) -> Result<RunManifest, AppError> {
    let raw = fs::read_to_string(manifest_path(root, run_id, true)?).map_err(|error| {
        AppError::RunPlan(format!(
            "could not read multi-agent run '{run_id}': {error}"
        ))
    })?;
    serde_json::from_str(&raw).map_err(|error| {
        AppError::RunPlan(format!("multi-agent run '{run_id}' is corrupt: {error}"))
    })
}

fn append_transcript(root: &Path, run_id: &str, agent_id: &str, value: &impl Serialize) {
    let path = match transcript_path(root, run_id, agent_id) {
        Ok(path) => path,
        Err(error) => {
            tracing::warn!("refused unsafe multi-agent transcript path: {error}");
            return;
        }
    };
    let line = match serde_json::to_string(value) {
        Ok(line) => line,
        Err(error) => {
            tracing::warn!("could not serialize multi-agent transcript event: {error}");
            return;
        }
    };
    if let Some(parent) = path.parent() {
        if let Err(error) = fs::create_dir_all(parent) {
            tracing::warn!("could not create multi-agent transcript directory: {error}");
            return;
        }
    }
    use std::io::Write;
    match fs::OpenOptions::new().create(true).append(true).open(&path) {
        Ok(mut file) => {
            let _ = writeln!(file, "{line}");
        }
        Err(error) => tracing::warn!("could not append multi-agent transcript: {error}"),
    }
}

fn validate_agent_ids(agent_ids: &[String]) -> Result<(), AppError> {
    if agent_ids.is_empty() || agent_ids.len() > 8 {
        return Err(AppError::RunPlan(
            "select between 1 and 8 distinct agent profiles".into(),
        ));
    }
    let mut seen = std::collections::HashSet::new();
    if agent_ids.iter().any(|id| !seen.insert(id)) {
        return Err(AppError::RunPlan(
            "an agent profile may only be assigned once per run".into(),
        ));
    }
    Ok(())
}

fn aggregate_status(sub_runs: &[MultiAgentSubRun]) -> MultiAgentRunStatus {
    if sub_runs
        .iter()
        .any(|sub| sub.status == MultiAgentRunStatus::Running)
    {
        MultiAgentRunStatus::Running
    } else if sub_runs
        .iter()
        .any(|sub| sub.status == MultiAgentRunStatus::Queued)
    {
        MultiAgentRunStatus::Queued
    } else if sub_runs
        .iter()
        .any(|sub| sub.status == MultiAgentRunStatus::Waiting)
    {
        MultiAgentRunStatus::Waiting
    } else if sub_runs
        .iter()
        .any(|sub| sub.status == MultiAgentRunStatus::Failed)
    {
        MultiAgentRunStatus::Failed
    } else if sub_runs
        .iter()
        .any(|sub| sub.status == MultiAgentRunStatus::Cancelled)
    {
        MultiAgentRunStatus::Cancelled
    } else {
        MultiAgentRunStatus::Done
    }
}

fn update_subrun(
    root: &Path,
    run_id: &str,
    agent_id: &str,
    update: impl FnOnce(&mut MultiAgentSubRun),
) -> Result<(MultiAgentRun, MultiAgentSubRun), AppError> {
    let mut manifest = load_manifest(root, run_id)?;
    let sub_run = manifest
        .run
        .sub_runs
        .iter_mut()
        .find(|sub| sub.agent_id == agent_id)
        .ok_or_else(|| {
            AppError::RunPlan(format!(
                "agent '{agent_id}' is not assigned to run '{run_id}'"
            ))
        })?;
    update(sub_run);
    let snapshot = sub_run.clone();
    manifest.run.status = aggregate_status(&manifest.run.sub_runs);
    if !matches!(
        manifest.run.status,
        MultiAgentRunStatus::Queued | MultiAgentRunStatus::Running | MultiAgentRunStatus::Waiting
    ) {
        manifest.run.completed_at.get_or_insert_with(now);
    }
    let run = manifest.run.clone();
    save_manifest(root, &manifest)?;
    Ok((run, snapshot))
}

fn emit_snapshot(channel: &Channel<MultiAgentEvent>, run_id: &str, sub_run: MultiAgentSubRun) {
    let _ = channel.send(MultiAgentEvent {
        run_id: run_id.to_string(),
        agent_id: sub_run.agent_id.clone(),
        event: None,
        sub_run: Some(sub_run),
    });
}

fn worktree_path(root: &Path, run_id: &str, agent_id: &str) -> Result<PathBuf, AppError> {
    // Contained under the project like every other managed worktree
    // (`prd-verified-delivery-reconciliation` Phase 2) — the legacy
    // sibling-of-repo `.loopdeck-agent-worktrees/` location is only
    // recognized by the external-worktree detector now.
    Ok(root
        .join(".loopdeck")
        .join("runs")
        .join("multi")
        .join(run_id)
        .join(agent_id))
}

fn has_active_run(root: &Path) -> Result<bool, AppError> {
    let base = root.join(".loopdeck").join(RUNS_DIR);
    let entries = match fs::read_dir(base) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(error) => return Err(AppError::Io(error)),
    };
    Ok(entries.filter_map(Result::ok).any(|entry| {
        load_manifest(root, &entry.file_name().to_string_lossy()).is_ok_and(|manifest| {
            matches!(
                manifest.run.status,
                MultiAgentRunStatus::Queued
                    | MultiAgentRunStatus::Running
                    | MultiAgentRunStatus::Waiting
            )
        })
    }))
}

/// Start the logical run in the background. Configs and their secrets are
/// resolved synchronously before any child is spawned, giving each sub-run a
/// stable profile snapshot even if Settings changes during execution.
#[tauri::command]
pub async fn agent_start_multi_loop_streaming(
    path: String,
    agent_ids: Vec<String>,
    on_event: Channel<MultiAgentEvent>,
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<MultiAgentRun, AppError> {
    validate_agent_ids(&agent_ids)?;
    let root = resolve_root(&state, &path)?;
    if has_active_run(&root)? {
        return Err(AppError::RunPlan(
            "a multi-agent loop is already active for this project".into(),
        ));
    }

    let assignments: Vec<ResolvedAssignment> = agent_ids
        .iter()
        .enumerate()
        .map(|(index, agent_id)| {
            let config = resolve_agent_config_by_id(&state, agent_id)?;
            let named = state
                .config
                .lock()
                .map_err(|_| AppError::LockError)?
                .find_agent_config(agent_id)
                .cloned()
                .ok_or_else(|| {
                    AppError::Config(format!("agent config '{agent_id}' was not found"))
                })?;
            Ok(ResolvedAssignment {
                sub_run: MultiAgentSubRun {
                    id: format!("sub-{}", index + 1),
                    agent_id: agent_id.clone(),
                    agent_name: named.name,
                    harness: config.harness,
                    model: config.model.clone(),
                    status: MultiAgentRunStatus::Queued,
                    branch: None,
                    worktree: None,
                    result: None,
                    error: None,
                    started_at: None,
                    completed_at: None,
                },
                config,
            })
        })
        .collect::<Result<_, AppError>>()?;

    let (prompt, title) = build_next_loop_prompt(&root);
    let run_id = Uuid::new_v4().to_string();
    let manifest = RunManifest {
        run: MultiAgentRun {
            id: run_id.clone(),
            path: root.clone(),
            loop_id: None,
            status: MultiAgentRunStatus::Queued,
            started_at: now(),
            completed_at: None,
            sub_runs: assignments
                .iter()
                .map(|assignment| assignment.sub_run.clone())
                .collect(),
        },
        prompt,
        title,
    };
    // The in-memory admission lock closes the read-check-create TOCTOU window
    // between two simultaneous Start/Retry calls for the same project.
    acquire_multi_agent_run(&state, &root)?;
    let manifest_lock = match multi_agent_manifest_lock(&state, &root) {
        Ok(lock) => lock,
        Err(error) => {
            let _ = release_multi_agent_run(&state, &root);
            return Err(error);
        }
    };
    if let Err(error) = save_manifest(&root, &manifest) {
        let _ = release_multi_agent_run(&state, &root);
        return Err(error);
    }
    let response = manifest.run.clone();
    tauri::async_runtime::spawn(execute_run(
        app,
        root,
        manifest,
        assignments,
        on_event,
        manifest_lock,
    ));
    Ok(response)
}

async fn execute_run(
    app: AppHandle,
    root: PathBuf,
    manifest: RunManifest,
    assignments: Vec<ResolvedAssignment>,
    on_event: Channel<MultiAgentEvent>,
    lock: Arc<AsyncMutex<()>>,
) {
    let mut jobs = JoinSet::new();
    for assignment in assignments {
        let app = app.clone();
        let root = root.clone();
        let run_id = manifest.run.id.clone();
        let prompt = manifest.prompt.clone();
        let title = manifest.title.clone();
        let on_event = on_event.clone();
        let lock = Arc::clone(&lock);
        jobs.spawn(async move {
            execute_subrun(app, root, run_id, prompt, title, assignment, on_event, lock).await;
        });
    }
    while jobs.join_next().await.is_some() {}
    let state = app.state::<AppState>();
    if let Err(error) = release_multi_agent_run(&state, &root) {
        tracing::warn!("could not release multi-agent admission lock: {error}");
    }
}

#[allow(clippy::too_many_arguments)]
async fn execute_subrun(
    app: AppHandle,
    root: PathBuf,
    run_id: String,
    prompt: String,
    title: Option<String>,
    assignment: ResolvedAssignment,
    on_event: Channel<MultiAgentEvent>,
    lock: Arc<AsyncMutex<()>>,
) {
    let agent_id = assignment.sub_run.agent_id.clone();
    let planned_worktree = match worktree_path(&root, &run_id, &agent_id) {
        Ok(path) => path,
        Err(error) => {
            finish_failed(
                &root,
                &run_id,
                &agent_id,
                error.to_string(),
                &on_event,
                &lock,
            )
            .await;
            return;
        }
    };
    // A retained failed worktree must never be overwritten by Retry. Reuse the
    // logical sub-run record but create a distinct attempt directory/branch.
    let attempt = Uuid::new_v4().simple().to_string();
    let worktree = if planned_worktree.exists() {
        planned_worktree.with_file_name(format!("{agent_id}-{}", &attempt[..8]))
    } else {
        planned_worktree
    };
    let branch = format!(
        "loopdeck/multi/{}/{}-{}",
        &run_id[..8],
        &agent_id[..8],
        &attempt[..8]
    );
    // Git serializes parts of `worktree add` itself, but this project lock
    // also makes the manifest transition atomic with creation. An interrupt
    // that wins while a sub-run is queued remains cancelled instead of being
    // overwritten by a late queued → running write.
    let start_result = {
        let _guard = lock.lock().await;
        let status = load_manifest(&root, &run_id).ok().and_then(|manifest| {
            manifest
                .run
                .sub_runs
                .into_iter()
                .find(|sub| sub.agent_id == agent_id)
                .map(|sub| sub.status)
        });
        if status == Some(MultiAgentRunStatus::Cancelled) {
            Ok(None)
        } else {
            git::worktree_add(&root, &worktree, &branch)
                .map_err(AppError::RunPlan)
                .and_then(|linked| {
                    update_subrun(&root, &run_id, &agent_id, |sub| {
                        sub.status = MultiAgentRunStatus::Running;
                        sub.branch = Some(linked.branch.clone());
                        sub.worktree = Some(linked.path.clone());
                        sub.started_at = Some(now());
                    })
                    .map(|(_, sub)| Some((linked, sub)))
                })
        }
    };
    let Some((linked, snapshot)) = (match start_result {
        Ok(value) => value,
        Err(error) => {
            finish_failed(
                &root,
                &run_id,
                &agent_id,
                error.to_string(),
                &on_event,
                &lock,
            )
            .await;
            return;
        }
    }) else {
        return;
    };
    emit_snapshot(&on_event, &run_id, snapshot);

    let transcript_root = root.clone();
    let transcript_run = run_id.clone();
    let transcript_agent = agent_id.clone();
    let event_channel = on_event.clone();
    let callback: Channel<ClaudeEvent> = Channel::new(move |body| {
        let event = match body {
            InvokeResponseBody::Json(json) => serde_json::from_str(&json)
                .unwrap_or_else(|_| serde_json::json!({ "type": "decode_error" })),
            InvokeResponseBody::Raw(bytes) => serde_json::json!({
                "type": "raw_event",
                "bytes": bytes,
            }),
        };
        append_transcript(&transcript_root, &transcript_run, &transcript_agent, &event);
        event_channel.send(MultiAgentEvent {
            run_id: transcript_run.clone(),
            agent_id: transcript_agent.clone(),
            event: Some(event),
            sub_run: None,
        })
    });

    let state = app.state::<AppState>();
    let outcome = start_fresh_and_record_streaming_in_root_with_config(
        &state,
        &linked.path,
        &root,
        &prompt,
        title,
        &callback,
        None,
        Some(&assignment.config),
        Some(&linked.path),
        true,
        false,
    )
    .await;
    // The session owns its child process. Drop it before even considering
    // worktree removal so no process can retain the linked checkout as cwd.
    if let Ok(mut sessions) = state.claude_sessions.lock() {
        sessions.remove(&linked.path);
    } else {
        tracing::warn!("could not drop terminal multi-agent session");
    }
    let _guard = lock.lock().await;
    let result = update_subrun(&root, &run_id, &agent_id, |sub| match outcome {
        Ok(response) => {
            // Interrupt persists cancellation before signalling the child. A
            // provider may still produce a normal terminal response while the
            // signal is being observed; never let that late response erase the
            // user's cancellation decision.
            if sub.status != MultiAgentRunStatus::Cancelled {
                sub.status = MultiAgentRunStatus::Done;
                sub.result = Some(response.result);
                sub.error = None;
            }
            sub.completed_at = Some(now());
        }
        Err(error) => {
            // A control request may already have marked this sub-run cancelled.
            if sub.status != MultiAgentRunStatus::Cancelled {
                sub.status = MultiAgentRunStatus::Failed;
                sub.error = Some(error.to_string());
            }
            sub.completed_at = Some(now());
        }
    });
    match result {
        Ok((_, sub)) => {
            append_transcript(&root, &run_id, &agent_id, &sub);
            emit_snapshot(&on_event, &run_id, sub);
        }
        Err(error) => tracing::warn!("could not persist multi-agent completion: {error}"),
    }

    // Successful but modified trees are valuable result artifacts too. Only a
    // pristine terminal tree is automatically removed; failed/cancelled trees
    // are always retained for diagnosis.
    if git::worktree_is_pristine(&linked.path) {
        let terminal = load_manifest(&root, &run_id)
            .ok()
            .and_then(|manifest| {
                manifest
                    .run
                    .sub_runs
                    .into_iter()
                    .find(|sub| sub.agent_id == agent_id)
            })
            .map(|sub| sub.status == MultiAgentRunStatus::Done)
            .unwrap_or(false);
        if terminal {
            if let Err(error) = git::worktree_remove(&root, &linked.path) {
                tracing::warn!("could not remove pristine multi-agent worktree: {error}");
            } else if let Ok((_, sub)) =
                update_subrun(&root, &run_id, &agent_id, |sub| sub.worktree = None)
            {
                emit_snapshot(&on_event, &run_id, sub);
            }
        }
    }
}

async fn finish_failed(
    root: &Path,
    run_id: &str,
    agent_id: &str,
    error: String,
    channel: &Channel<MultiAgentEvent>,
    lock: &Arc<AsyncMutex<()>>,
) {
    let _guard = lock.lock().await;
    match update_subrun(root, run_id, agent_id, |sub| {
        sub.status = MultiAgentRunStatus::Failed;
        sub.error = Some(error);
        sub.completed_at = Some(now());
    }) {
        Ok((_, sub)) => emit_snapshot(channel, run_id, sub),
        Err(error) => tracing::warn!("could not persist multi-agent failure: {error}"),
    }
}

#[tauri::command]
pub async fn agent_get_multi_agent_run(
    path: String,
    run_id: String,
    state: State<'_, AppState>,
) -> Result<MultiAgentRun, AppError> {
    let root = resolve_root(&state, &path)?;
    Ok(load_manifest(&root, &run_id)?.run)
}

#[tauri::command]
pub async fn agent_list_multi_agent_runs(
    path: String,
    state: State<'_, AppState>,
) -> Result<Vec<MultiAgentRun>, AppError> {
    let root = resolve_root(&state, &path)?;
    let base = root.join(".loopdeck").join(RUNS_DIR);
    let mut runs = match fs::read_dir(base) {
        Ok(entries) => entries
            .filter_map(Result::ok)
            .filter_map(|entry| {
                load_manifest(&root, &entry.file_name().to_string_lossy())
                    .ok()
                    .map(|m| m.run)
            })
            .collect::<Vec<_>>(),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Vec::new(),
        Err(error) => return Err(AppError::Io(error)),
    };
    runs.sort_by(|a, b| b.started_at.cmp(&a.started_at));
    Ok(runs)
}

#[tauri::command]
pub async fn agent_control_multi_agent_run(
    path: String,
    run_id: String,
    agent_id: String,
    action: MultiAgentControlAction,
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<MultiAgentRun, AppError> {
    let root = resolve_root(&state, &path)?;
    let manifest_lock = multi_agent_manifest_lock(&state, &root)?;
    let guard = manifest_lock.lock().await;
    let manifest = load_manifest(&root, &run_id)?;
    let current = manifest
        .run
        .sub_runs
        .iter()
        .find(|sub| sub.agent_id == agent_id)
        .cloned()
        .ok_or_else(|| {
            AppError::RunPlan(format!(
                "agent '{agent_id}' is not assigned to run '{run_id}'"
            ))
        })?;
    match action {
        MultiAgentControlAction::Interrupt => {
            if !matches!(
                current.status,
                MultiAgentRunStatus::Queued
                    | MultiAgentRunStatus::Running
                    | MultiAgentRunStatus::Waiting
            ) {
                return Err(AppError::RunPlan(
                    "only a queued or active sub-run can be interrupted".into(),
                ));
            }
            // Persist cancellation before signalling the child. The worker uses
            // the same lock and will preserve this terminal state if its turn
            // returns concurrently.
            let (run, _) = update_subrun(&root, &run_id, &agent_id, |sub| {
                sub.status = MultiAgentRunStatus::Cancelled;
                sub.completed_at = Some(now());
                sub.error = Some("interrupted by user".into());
            })?;
            if let Some(worktree) = current.worktree.as_deref() {
                let _ = fire_interrupt(&state, worktree)?;
            }
            Ok(run)
        }
        MultiAgentControlAction::Retry => {
            if matches!(
                current.status,
                MultiAgentRunStatus::Running | MultiAgentRunStatus::Queued
            ) {
                return Err(AppError::RunPlan(
                    "only a terminal sub-run can be retried".into(),
                ));
            }
            acquire_multi_agent_run(&state, &root)?;
            let config = match resolve_agent_config_by_id(&state, &agent_id) {
                Ok(config) => config,
                Err(error) => {
                    let _ = release_multi_agent_run(&state, &root);
                    return Err(error);
                }
            };
            let retry = ResolvedAssignment {
                sub_run: current.clone(),
                config,
            };
            let mut retry_manifest = manifest.clone();
            let sub = retry_manifest
                .run
                .sub_runs
                .iter_mut()
                .find(|sub| sub.agent_id == agent_id)
                .expect("validated above");
            sub.status = MultiAgentRunStatus::Queued;
            sub.error = None;
            sub.result = None;
            sub.completed_at = None;
            sub.started_at = None;
            sub.worktree = None;
            sub.branch = None;
            retry_manifest.run.status = MultiAgentRunStatus::Queued;
            retry_manifest.run.completed_at = None;
            if let Err(error) = save_manifest(&root, &retry_manifest) {
                let _ = release_multi_agent_run(&state, &root);
                return Err(error);
            }
            let response = retry_manifest.run.clone();
            drop(guard);
            // A retry is a single new worker within the same logical run.
            tauri::async_runtime::spawn(execute_run(
                app,
                root.clone(),
                retry_manifest,
                vec![retry],
                Channel::new(|_| Ok(())),
                Arc::clone(&manifest_lock),
            ));
            Ok(response)
        }
    }
}

/// Downgrade persisted queued/running/waiting records after application restart.
/// There is no live process after restart, so retaining those labels would make
/// the UI lie and prevent a retry. The retained worktree is deliberately not
/// removed because it may contain partial edits.
pub fn reconcile_stale_runs(root: &Path) -> Result<bool, AppError> {
    let base = root.join(".loopdeck").join(RUNS_DIR);
    let entries = match fs::read_dir(base) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(error) => return Err(AppError::Io(error)),
    };
    let mut changed = false;
    for entry in entries.filter_map(Result::ok) {
        let run_id = entry.file_name().to_string_lossy().to_string();
        let mut manifest = match load_manifest(root, &run_id) {
            Ok(manifest) => manifest,
            Err(error) => {
                tracing::warn!("could not reconcile multi-agent run: {error}");
                continue;
            }
        };
        let mut run_changed = false;
        for sub in &mut manifest.run.sub_runs {
            if matches!(
                sub.status,
                MultiAgentRunStatus::Queued
                    | MultiAgentRunStatus::Running
                    | MultiAgentRunStatus::Waiting
            ) {
                sub.status = MultiAgentRunStatus::Cancelled;
                sub.error = Some("interrupted by application restart".into());
                sub.completed_at = Some(now());
                run_changed = true;
            }
        }
        if run_changed {
            manifest.run.status = aggregate_status(&manifest.run.sub_runs);
            manifest.run.completed_at.get_or_insert_with(now);
            save_manifest(root, &manifest)?;
            changed = true;
        }
    }
    Ok(changed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;
    use tokio::sync::Barrier;

    fn test_root() -> PathBuf {
        let root =
            std::env::temp_dir().join(format!("loopdeck-multi-agent-test-{}", Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        fs::canonicalize(root).unwrap()
    }

    fn test_subrun(status: MultiAgentRunStatus) -> MultiAgentSubRun {
        MultiAgentSubRun {
            id: "sub-1".into(),
            agent_id: Uuid::new_v4().to_string(),
            agent_name: "Test agent".into(),
            harness: AgentHarness::Claude,
            model: None,
            status,
            branch: None,
            worktree: None,
            result: None,
            error: None,
            started_at: None,
            completed_at: None,
        }
    }

    fn test_manifest(root: &Path, status: MultiAgentRunStatus) -> RunManifest {
        RunManifest {
            run: MultiAgentRun {
                id: Uuid::new_v4().to_string(),
                path: root.to_path_buf(),
                loop_id: None,
                status,
                started_at: now(),
                completed_at: None,
                sub_runs: vec![test_subrun(status)],
            },
            prompt: "test prompt".into(),
            title: None,
        }
    }

    #[test]
    fn validation_rejects_empty_duplicate_and_overflow_assignments() {
        assert!(validate_agent_ids(&[]).is_err());
        assert!(validate_agent_ids(&["a".into(), "a".into()]).is_err());
        assert!(validate_agent_ids(&(0..9).map(|i| i.to_string()).collect::<Vec<_>>()).is_err());
        assert!(validate_agent_ids(&["a".into(), "b".into()]).is_ok());
    }

    #[test]
    fn aggregate_preserves_partial_failure_visibility() {
        let statuses = [MultiAgentRunStatus::Done, MultiAgentRunStatus::Failed];
        let sub_runs = statuses
            .into_iter()
            .enumerate()
            .map(|(i, status)| MultiAgentSubRun {
                id: i.to_string(),
                agent_id: i.to_string(),
                agent_name: i.to_string(),
                harness: AgentHarness::Claude,
                model: None,
                status,
                branch: None,
                worktree: None,
                result: None,
                error: None,
                started_at: None,
                completed_at: None,
            })
            .collect::<Vec<_>>();
        assert_eq!(aggregate_status(&sub_runs), MultiAgentRunStatus::Failed);
    }

    #[test]
    fn restart_reconciliation_cancels_a_queued_manifest() {
        let root = test_root();
        let manifest = test_manifest(&root, MultiAgentRunStatus::Queued);
        save_manifest(&root, &manifest).unwrap();

        assert!(reconcile_stale_runs(&root).unwrap());
        let restored = load_manifest(&root, &manifest.run.id).unwrap();
        assert_eq!(restored.run.status, MultiAgentRunStatus::Cancelled);
        assert_eq!(
            restored.run.sub_runs[0].status,
            MultiAgentRunStatus::Cancelled
        );
        assert!(restored.run.sub_runs[0]
            .error
            .as_deref()
            .is_some_and(|error| error.contains("application restart")));
        fs::remove_dir_all(root).unwrap();
    }

    #[tokio::test]
    async fn shared_manifest_lock_serializes_cancel_before_start_transition() {
        let root = test_root();
        let manifest = test_manifest(&root, MultiAgentRunStatus::Queued);
        let run_id = manifest.run.id.clone();
        let agent_id = manifest.run.sub_runs[0].agent_id.clone();
        save_manifest(&root, &manifest).unwrap();
        let lock = Arc::new(AsyncMutex::new(()));

        {
            let _guard = lock.lock().await;
            update_subrun(&root, &run_id, &agent_id, |sub| {
                sub.status = MultiAgentRunStatus::Cancelled;
                sub.error = Some("interrupted by user".into());
            })
            .unwrap();
        }
        {
            let _guard = lock.lock().await;
            let current = load_manifest(&root, &run_id).unwrap();
            assert_eq!(
                current.run.sub_runs[0].status,
                MultiAgentRunStatus::Cancelled
            );
            // Mirrors the worker guard: cancelled is terminal and must not be
            // overwritten by a delayed queued → running transition.
            if current.run.sub_runs[0].status != MultiAgentRunStatus::Cancelled {
                update_subrun(&root, &run_id, &agent_id, |sub| {
                    sub.status = MultiAgentRunStatus::Running;
                })
                .unwrap();
            }
        }
        assert_eq!(
            load_manifest(&root, &run_id).unwrap().run.sub_runs[0].status,
            MultiAgentRunStatus::Cancelled
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[tokio::test]
    async fn assigned_agents_work_concurrently_in_isolated_worktrees_then_cleanup() {
        let root = test_root();
        for args in [
            vec!["init"],
            vec!["config", "user.email", "test@loopdeck.dev"],
            vec!["config", "user.name", "LoopDeck Test"],
        ] {
            assert!(Command::new("git")
                .arg("-C")
                .arg(&root)
                .args(args)
                .status()
                .unwrap()
                .success());
        }
        fs::write(root.join("README.md"), "# multi-agent fixture\n").unwrap();
        assert!(Command::new("git")
            .arg("-C")
            .arg(&root)
            .args(["add", "."])
            .status()
            .unwrap()
            .success());
        assert!(Command::new("git")
            .arg("-C")
            .arg(&root)
            .args(["commit", "-m", "initial"])
            .status()
            .unwrap()
            .success());

        let run_id = Uuid::new_v4().to_string();
        let agent_a = Uuid::new_v4().to_string();
        let agent_b = Uuid::new_v4().to_string();
        let path_a = worktree_path(&root, &run_id, &agent_a).unwrap();
        let path_b = worktree_path(&root, &run_id, &agent_b).unwrap();
        let linked_a = git::worktree_add(&root, &path_a, "test/multi-agent-a").unwrap();
        let linked_b = git::worktree_add(&root, &path_b, "test/multi-agent-b").unwrap();

        let barrier = Arc::new(Barrier::new(3));
        let task_a = {
            let barrier = Arc::clone(&barrier);
            let path = linked_a.path.clone();
            tokio::spawn(async move {
                fs::write(path.join("agent-a.txt"), "a").unwrap();
                barrier.wait().await;
                barrier.wait().await;
            })
        };
        let task_b = {
            let barrier = Arc::clone(&barrier);
            let path = linked_b.path.clone();
            tokio::spawn(async move {
                fs::write(path.join("agent-b.txt"), "b").unwrap();
                barrier.wait().await;
                barrier.wait().await;
            })
        };
        barrier.wait().await;
        assert!(linked_a.path.join("agent-a.txt").exists());
        assert!(!linked_a.path.join("agent-b.txt").exists());
        assert!(linked_b.path.join("agent-b.txt").exists());
        assert!(!linked_b.path.join("agent-a.txt").exists());
        barrier.wait().await;
        task_a.await.unwrap();
        task_b.await.unwrap();

        fs::remove_file(linked_a.path.join("agent-a.txt")).unwrap();
        fs::remove_file(linked_b.path.join("agent-b.txt")).unwrap();
        assert!(git::worktree_is_pristine(&linked_a.path));
        assert!(git::worktree_is_pristine(&linked_b.path));
        git::worktree_remove(&root, &linked_a.path).unwrap();
        git::worktree_remove(&root, &linked_b.path).unwrap();
        assert!(!path_a.exists());
        assert!(!path_b.exists());
        fs::remove_dir_all(root).unwrap();
    }
}
