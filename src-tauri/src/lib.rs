mod commands;
mod config;
mod error;
mod git;
mod memory;
mod project;
mod scanner;

use commands::AppState;
use config::GlobalConfig;
use std::sync::Mutex;

pub fn run() {
    // Load config — if it fails, start with a clean default and persist it
    let config = GlobalConfig::load().unwrap_or_else(|e| {
        eprintln!("Warning: Failed to load config, starting fresh: {e}");
        let fresh = GlobalConfig::default();
        // Try to save the fresh config so next restart succeeds
        if let Err(save_err) = fresh.save() {
            eprintln!("Warning: Failed to save default config: {save_err}");
        }
        fresh
    });

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_shell::init())
        .manage(AppState {
            config: Mutex::new(config),
        })
        .invoke_handler(tauri::generate_handler![
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
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
