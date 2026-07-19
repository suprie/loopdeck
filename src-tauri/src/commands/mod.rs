//! Tauri IPC command layer.
//!
//! Split into domain modules — `state` (shared plumbing), `composer`
//! (`@`/`/` autocomplete + scan), `project` (import/list/get/update/remove),
//! `config_cmds` (agent config + auth token), `epics` (decisions/loops/specs),
//! and `agent` (the 16 `agent_*` Claude-session commands). Each domain module
//! owns its private helpers and unit tests; `state` holds the cross-cutting
//! `AppState`, shared types, and lock plumbing.
//!
//! `lib.rs` registers commands at `commands::<module>::<name>` (Tauri's
//! `#[command]` macro emits `__cmd__<name>` items in the defining module, so
//! they must be referenced from there rather than via `pub use`).

pub mod agent;
pub mod composer;
pub mod config_cmds;
pub mod epics;
pub mod project;
pub mod state;

// `AppState` is the one type consumed at the call site in `lib.rs`
// (`use commands::AppState`); re-export it so that import stays stable.
pub use state::AppState;
