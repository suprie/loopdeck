mod agents;
mod binary;
mod claude_session;
mod codex_session;
mod commands;
mod config;
mod conversation;
mod epic;
mod error;
mod execution;
mod git;
mod graphify;
mod harness;
mod limits;
mod logging;
mod memory;
mod migration;
mod multi_agent;
mod paths;
mod permission;
mod persist;
mod progress;
mod project;
pub mod retry;
mod run_executor;
mod runplan;
mod scanner;
mod secret_scan;
mod secrets;
mod skills;
mod state_cli;

/// Entry point for the `loopdeck state` subcommand (Phase 3 `state-command`).
/// Branch is taken in `main.rs` when the binary's first arg is `state`.
pub use state_cli::run_state_cli;

/// Entry point for the `loopdeck secret-scan` subcommand
/// (`prd-unattended-ship.md` P1). Branch is taken in `main.rs` when the
/// binary's first arg is `secret-scan`.
pub use secret_scan::run_secret_scan_cli;

use commands::AppState;
use config::GlobalConfig;
use std::{
    collections::{HashMap, HashSet},
    sync::Mutex,
};

pub fn run() {
    // Initialize tracing FIRST, before any plugin/spawn — so early logs from
    // config load, Tauri, and the claude process are captured. Idempotent.
    logging::init_logging();

    // Load the registry. Per PRD FR2, a malformed primary MUST NOT be silently
    // overwritten with a fresh default — that would turn recoverable corruption
    // into silent data loss. `GlobalConfig::load` tries the primary, then the
    // `.bak`, then returns Err. Surface that as a structured error + exit so
    // the user knows their project list is intact-but-needing-repair, not
    // wiped. The malformed file stays on disk for manual recovery.
    let mut config = match GlobalConfig::load() {
        Ok(c) => c,
        Err(e) => {
            tracing::error!("registry load failed: {e}");
            tracing::error!(
                "the malformed registry has NOT been overwritten; repair it \
                 manually or delete it to start fresh"
            );
            std::process::exit(1);
        }
    };

    // One-time migration: move any plaintext auth token out of config.yaml into
    // the local secrets file. Best-effort — if the secrets file write fails we
    // keep the token in memory and carry on; `save()` still applies the 0600
    // interim floor below so any plaintext copy that remains is owner-only.
    match config.migrate_auth_token_to_secrets_file() {
        Ok(true) => tracing::info!(
            "migrated plaintext auth token from config.yaml to local secrets file"
        ),
        Ok(false) => {}
        Err(e) => tracing::warn!(
            "secrets file write failed; keeping auth token in 0600 config.yaml as interim floor: {e}"
        ),
    }
    // Persist: scrubs the token from the file when migration succeeded, and
    // (re)applies the 0600 permission floor in every case so a pre-existing
    // world-readable file is tightened even if nothing else changed.
    if let Err(e) = config.save() {
        tracing::warn!("failed to persist config after auth-token migration: {e}");
    }

    // Reconcile interrupted sessions (PRD FR5). A turn killed mid-flight by a
    // restart/crash leaves the user turn (appended before the send) with no
    // following assistant reply — a trailing orphan in `active.jsonl`. Mark each
    // such orphan `interrupted` now, before any IPC load, so the transcript
    // truthfully records that the turn did not complete and a new turn is
    // unblocked (the in-memory busy/waiting state is already clear — it is
    // ephemeral and starts empty on a fresh process). This is the
    // restart-recovery half; the send-failure half runs in the agent pipelines.
    // Transcript-based, so no separate run record is persisted. Best-effort per
    // project: a reconciliation failure is logged, never fatal.
    for project in &config.projects {
        match conversation::reconcile_interrupted(&project.path) {
            Ok(true) => tracing::info!(
                "reconciled interrupted session for {}",
                project.path.display()
            ),
            Ok(false) => {}
            Err(e) => tracing::warn!(
                "failed to reconcile interrupted session for {}: {e}",
                project.path.display()
            ),
        }
    }

    // Same restart-recovery reasoning as the transcript reconciliation above,
    // for `prd-run-queue` Phase 2's run plans: a `Running` phase left on disk
    // by a killed/crashed process is not actually running on this fresh
    // process (`AppState.run_handles` starts empty) — downgrade it to
    // `Interrupted` so it's requeueable instead of the plan silently claiming
    // forward progress that never happened.
    for project in &config.projects {
        match run_executor::reconcile_running_phases(&project.path) {
            Ok(true) => tracing::info!(
                "reconciled interrupted run plan for {}",
                project.path.display()
            ),
            Ok(false) => {}
            Err(e) => tracing::warn!(
                "failed to reconcile run plan for {}: {e}",
                project.path.display()
            ),
        }
        if let Err(e) = multi_agent::reconcile_stale_runs(&project.path) {
            tracing::warn!(
                "failed to reconcile interrupted multi-agent runs for {}: {e}",
                project.path.display()
            );
        }
    }

    if let Err(e) = tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_shell::init())
        .manage(AppState {
            config: Mutex::new(config),
            claude_sessions: Mutex::new(HashMap::new()),
            pending_answers: Mutex::new(HashMap::new()),
            pending_permissions: Mutex::new(HashMap::new()),
            pending_plans: Mutex::new(HashMap::new()),
            interrupt_slots: Mutex::new(HashMap::new()),
            run_handles: Mutex::new(HashMap::new()),
            multi_agent_active_runs: Mutex::new(HashSet::new()),
            multi_agent_manifest_locks: Mutex::new(HashMap::new()),
        })
        .invoke_handler(tauri::generate_handler![
            // Composer + scan (composer.rs)
            commands::composer::list_dir_entries,
            commands::composer::search_project_files,
            commands::composer::list_skills,
            commands::composer::scan_directory,
            // Project management (project.rs)
            commands::project::import_project,
            commands::project::list_projects,
            commands::project::get_project,
            commands::project::update_description,
            commands::project::set_project_autonomous,
            commands::project::remove_project,
            commands::project::open_in_finder,
            commands::project::open_in_terminal,
            commands::project::regenerate_description,
            commands::project::rescan_project,
            commands::project::refresh_skills,
            commands::project::get_graphify_stats,
            // Decisions / loops / epics / specs (epics.rs)
            commands::epics::get_decisions,
            commands::epics::get_loops,
            commands::epics::get_epics,
            commands::epics::get_epics_by_milestone,
            commands::epics::promote_epic_loop,
            commands::epics::toggle_loop_step,
            commands::epics::toggle_prd_loop,
            commands::epics::assign_loop_id,
            commands::epics::read_spec_file,
            commands::epics::write_spec_file,
            // Structured execution state (execution.rs) — 0.2.1
            commands::execution::get_execution_state,
            commands::execution::promote_loop_by_id,
            commands::execution::complete_current_loop,
            commands::execution::abandon_current_loop,
            commands::execution::promote_next_queued_loop,
            commands::execution::get_progress_snapshot,
            commands::execution::export_execution_summary,
            // Migration from legacy loops.md → execution.yaml — 0.2.1 Phase 4
            commands::execution::get_migration_preview,
            commands::execution::apply_migration,
            // Run queue — prd-run-queue Phase 2 (+ create_run_plan, Phase 5 picker)
            commands::run_queue::create_run_plan,
            commands::run_queue::queue_run,
            commands::run_queue::cancel_run,
            commands::run_queue::get_run_status,
            commands::run_queue::get_run_report,
            commands::run_queue::answer_parked_question,
            commands::run_queue::requeue_run_phase,
            // Run queue pre-flight interview — prd-run-queue Phase 3
            commands::run_queue::run_phase_interview,
            commands::run_queue::skip_phase_interview,
            // Agent config + auth token + diagnostics (config_cmds.rs)
            commands::config_cmds::get_agent_config,
            commands::config_cmds::set_agent_config,
            commands::config_cmds::clear_auth_token,
            commands::config_cmds::list_agent_configs,
            commands::config_cmds::create_agent_config,
            commands::config_cmds::update_agent_config,
            commands::config_cmds::delete_agent_config,
            commands::config_cmds::get_default_agent_config,
            commands::config_cmds::set_default_agent_config,
            commands::config_cmds::get_log_info,
            commands::config_cmds::reveal_log_dir,
            // Agent / Claude session (agent.rs)
            commands::agent::agent_start_loop,
            commands::agent::agent_start_loop_streaming,
            multi_agent::agent_start_multi_loop_streaming,
            multi_agent::agent_get_multi_agent_run,
            multi_agent::agent_list_multi_agent_runs,
            multi_agent::agent_control_multi_agent_run,
            commands::agent::agent_send_message,
            commands::agent::agent_send_message_streaming,
            commands::agent::agent_read_image_attachment,
            commands::agent::agent_get_conversation,
            commands::agent::agent_list_conversations,
            commands::agent::agent_get_conversation_by_id,
            commands::agent::agent_promote_to_active,
            commands::agent::agent_reset_session,
            commands::agent::agent_answer_question,
            commands::agent::agent_answer_permission,
            commands::agent::agent_answer_plan,
            commands::agent::agent_add_allow_rule,
            commands::agent::agent_interrupt,
            commands::agent::agent_is_busy,
            commands::agent::agent_pending_permission,
            commands::agent::agent_pending_question,
            commands::agent::agent_pending_plan,
            commands::agent::list_pending_questions,
            commands::agent::list_pending_permissions,
            commands::agent::list_pending_plans,
        ])
        .run(tauri::generate_context!())
    {
        // `run()` only errors on an unrecoverable startup failure (WebView
        // init, context generation, plugin init) — there is no recovery path,
        // so terminating is correct either way. But under `panic = "abort"` the
        // `.expect()` that lived here aborted the process with a raw panic
        // message; logging via tracing (initialized above) and exiting cleanly
        // removes that panic source and lands a structured log line a user can
        // actually find when the app won't start.
        tracing::error!("error while running tauri application: {e}");
        std::process::exit(1);
    }
}
