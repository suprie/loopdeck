---
name: loopdeck:rust-expert
description: Use when writing or modifying Rust backend code in src-tauri/ for the LoopDeck desktop app. Covers idiomatic Rust patterns, error handling with thiserror + serde, Tauri v2 command design, async patterns, safe state management, and testing.
allowed-tools: [Read, Write, Edit, Glob, Grep, Bash]
---

# Rust Expert — LoopDeck Backend

You are a senior Rust engineer working on the LoopDeck Tauri v2 desktop application. Follow these conventions.

## Architecture

```
src-tauri/src/
├── main.rs          # Thin entry point — calls app_lib::run()
├── lib.rs           # Tauri builder, state, plugin + command registration
├── error.rs         # AppError enum (thiserror) + manual Serialize
├── config.rs        # GlobalConfig, ProjectEntry, Settings, load/save
├── scanner.rs       # Repository discovery by marker files
├── project.rs       # .loopdeck/ bootstrap + description generation
└── commands.rs      # All #[tauri::command] handlers
```

- `main.rs` is minimal — just `fn main() { app_lib::run(); }`
- `lib.rs` assembles the app: tracing init, state, plugin registration, `generate_handler!`
- All new backend logic goes in the appropriate module, not in `lib.rs`

## Error Handling

Use `thiserror` derive for the `AppError` enum. Every variant must have a manual `serde::Serialize` impl that emits `{ "message": "...", "kind": "..." }` so the frontend receives structured errors.

```rust
use serde::Serialize;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum AppError {
    #[error("Configuration error: {0}")]
    Config(String),

    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Serialization error: {0}")]
    Serde(#[from] serde_yaml::Error),

    #[error("Project not found at path: {0}")]
    ProjectNotFound(String),

    #[error("Invalid path: {0}")]
    InvalidPath(String),

    #[error("Scan error: {0}")]
    Scan(String),

    #[error("Lock poisoned")]
    LockError,
}

impl Serialize for AppError {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;
        let mut state = serializer.serialize_struct("AppError", 3)?;
        state.serialize_field("message", &self.to_string())?;
        state.serialize_field(
            "kind",
            match self {
                AppError::Config(_) => "config",
                AppError::Io(_) => "io",
                AppError::Serde(_) => "serde",
                AppError::ProjectNotFound(_) => "projectNotFound",
                AppError::InvalidPath(_) => "invalidPath",
                AppError::Scan(_) => "scan",
                AppError::LockError => "lockError",
            },
        )?;
        state.end()
    }
}
```

- No `unwrap()` in production code — use `map_err(|e| AppError::...)` or `?`
- Every fallible function returns `Result<T, AppError>`
- Error variants carry context — no bare strings

## Command Design

```rust
use std::sync::Mutex;
use tauri::State;
use crate::config::GlobalConfig;
use crate::error::AppError;

pub struct AppState {
    pub config: Mutex<GlobalConfig>,
}

#[tauri::command]
pub async fn get_projects(
    state: State<'_, AppState>,
) -> Result<Vec<ProjectEntry>, AppError> {
    let config = state.config.lock().map_err(|_| AppError::LockError)?;
    Ok(config.projects.clone())
}
```

- All `#[tauri::command]` functions are `async`
- Accept `tauri::State<'_, AppState>` to access shared config
- Mutate config under `state.config.lock().map_err(|_| AppError::LockError)?`
- Call `config.save()` after every mutation
- Heavy I/O (directory scanning) runs inside `tokio::task::spawn_blocking`

## State Management

- `AppState` wraps `Mutex<GlobalConfig>` — single writer, Tauri serializes command execution
- Never hold the lock across an await point
- Every mutation is followed by `config.save()?` for durability

## Config Structure

```rust
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectEntry {
    pub path: PathBuf,
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub status: ProjectStatus,
    pub last_opened: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub enum ProjectStatus {
    #[default]
    Active,
    Archived,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Settings {
    #[serde(default = "default_scan_depth")]
    pub scan_depth: u8,
}

fn default_scan_depth() -> u8 { 5 }

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GlobalConfig {
    #[serde(default)]
    pub projects: Vec<ProjectEntry>,
    #[serde(default)]
    pub settings: Settings,
}
```

## Scanner

Use `walkdir` with marker-file detection:

```rust
const PROJECT_MARKERS: &[&str] = &[
    ".git",           // directory
    "Cargo.toml",     // file
    "package.json",   // file
    "go.mod",         // file
    "Package.swift",  // file
    "Gemfile",        // file
    "Podfile",        // file
];

const IGNORED_DIRS: &[&str] = &[
    "node_modules", "target", ".git", "__pycache__",
    ".venv", "venv", "dist", "build", ".DS_Store",
];
```

- A directory is a candidate repo if it contains ANY project marker
- Exclude `IGNORED_DIRS` from traversal using `filter_entry`
- Use configured `settings.scan_depth` for max depth
- Return `DiscoveredRepo { path, name, markers, has_readme, has_loopdeck }`

## .loopdeck Bootstrap

```yaml
# repo/.loopdeck/project.yaml
name: Budget Manager
description: |
  Local-first budgeting application for importing
  and categorizing Indonesian bank statements.
status: active
created_at: 2026-06-22
```

- Create `.loopdeck/` directory if absent
- Generate description from README.md (first meaningful paragraph)
- Fallback: `"{name} — {detected_stack} project"` from markers
- Write `project.yaml` with serde_yaml

## Logging

- Use `tracing::{info, debug, warn, error}` macros
- Log entry/exit of every command at debug level
- Log scan results at info level with timing (`Instant::now()`)

## Testing

- Write unit tests in the same file with `#[cfg(test)] mod tests { ... }`
- Test config serialization round-trips
- Test scanner with temp directories (`tempfile` crate)
- Test description generation with various README inputs
- Test each command's happy path and error path

## Build & Run

```bash
# Dev
npm run tauri dev

# Rust tests
cd src-tauri && cargo test

# Rust lint
cd src-tauri && cargo clippy

# TypeScript type-check
npx tsc --noEmit
```
