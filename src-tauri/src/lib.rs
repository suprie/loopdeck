mod agents;
mod claude_session;
mod commands;
mod config;
mod conversation;
mod epic;
mod error;
mod git;
mod logging;
mod memory;
mod permission;
mod project;
mod scanner;
mod skills;

use commands::AppState;
use config::GlobalConfig;
use std::{collections::HashMap, sync::Mutex};

pub fn run() {
    // Initialize tracing FIRST, before any plugin/spawn — so early logs from
    // config load, Tauri, and the claude process are captured. Idempotent.
    logging::init_logging();

    // Load config — if it fails, start with a clean default and persist it
    let config = GlobalConfig::load().unwrap_or_else(|e| {
        tracing::warn!("failed to load config, starting fresh: {e}");
        let fresh = GlobalConfig::default();
        // Try to save the fresh config so next restart succeeds
        if let Err(save_err) = fresh.save() {
            tracing::warn!("failed to save default config: {save_err}");
        }
        fresh
    });

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_shell::init())
        .manage(AppState {
            config: Mutex::new(config),
            claude_sessions: Mutex::new(HashMap::new()),
            pending_answers: Mutex::new(HashMap::new()),
            pending_permissions: Mutex::new(HashMap::new()),
            interrupt_slots: Mutex::new(HashMap::new()),
        })
        .invoke_handler(tauri::generate_handler![
            commands::list_dir_entries,
            commands::search_project_files,
            commands::list_skills,
            commands::scan_directory,
            commands::import_project,
            commands::list_projects,
            commands::get_project,
            commands::update_description,
            commands::remove_project,
            commands::open_in_finder,
            commands::open_in_terminal,
            commands::regenerate_description,
            commands::rescan_project,
            commands::get_decisions,
            commands::get_loops,
            commands::get_epics,
            commands::get_epics_by_milestone,
            commands::promote_epic_loop,
            commands::toggle_loop_step,
            commands::toggle_prd_loop,
            commands::read_spec_file,
            commands::write_spec_file,
            commands::get_agent_config,
            commands::set_agent_config,
            commands::agent_start_loop,
            commands::agent_start_loop_streaming,
            commands::agent_send_message,
            commands::agent_send_message_streaming,
            commands::agent_get_conversation,
            commands::agent_list_conversations,
            commands::agent_get_conversation_by_id,
            commands::agent_promote_to_active,
            commands::agent_reset_session,
            commands::agent_answer_question,
            commands::agent_answer_permission,
            commands::agent_add_allow_rule,
            commands::agent_interrupt,
            commands::agent_is_busy,
            commands::agent_pending_permission,
            commands::agent_pending_question,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
