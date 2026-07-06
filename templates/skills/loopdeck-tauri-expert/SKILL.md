---
name: loopdeck:tauri-expert
description: Use when working on Tauri v2 configuration, IPC design, capabilities/permissions, window management, plugin integration, build/packaging, or cross-platform concerns for the LoopDeck desktop app.
allowed-tools: [Read, Write, Edit, Glob, Grep, Bash]
---

# Tauri Expert — LoopDeck Desktop Shell

You are a Tauri v2 desktop application specialist working on LoopDeck. Follow these conventions for all Tauri-related work.

## Configuration (`src-tauri/tauri.conf.json`)

```json
{
  "$schema": "https://raw.githubusercontent.com/nicedoc/tauri/dev/crates/tauri-config-schema/schema.json",
  "productName": "LoopDeck",
  "version": "0.1.0",
  "identifier": "com.loopdeck.app",
  "build": {
    "devUrl": "http://localhost:5173",
    "frontendDist": "../dist",
    "beforeDevCommand": "npm run dev",
    "beforeBuildCommand": "npm run build"
  },
  "app": {
    "withGlobalTauri": false,
    "windows": [
      {
        "label": "main",
        "title": "LoopDeck - AI Project Manager",
        "width": 1200,
        "height": 800,
        "minWidth": 900,
        "minHeight": 600,
        "resizable": true,
        "fullscreen": false,
        "center": true
      }
    ],
    "security": {
      "csp": null
    }
  },
  "bundle": {
    "active": true,
    "targets": "all",
    "icon": [
      "icons/32x32.png",
      "icons/128x128.png",
      "icons/128x128@2x.png",
      "icons/icon.icns",
      "icons/icon.ico"
    ]
  }
}
```

Key rules:
- `identifier` must be reverse-domain: `com.loopdeck.app`
- `build.devUrl` must match the Vite dev server port (5173)
- `build.frontendDist` points to `../dist` (Vite output, relative to `src-tauri/`)
- Window label is `"main"` — used in capability files
- `withGlobalTauri` is `false` — no `window.__TAURI__` pollution
- CSP is `null` for local development

## Capabilities (v2 ACL)

Files live in `src-tauri/capabilities/` as JSON:

```json
{
  "identifier": "default",
  "description": "Default capability for the main window",
  "windows": ["main"],
  "permissions": [
    "core:default",
    "shell:allow-open",
    "dialog:allow-open",
    "dialog:allow-ask",
    "dialog:allow-confirm",
    "fs:allow-read-text-file",
    "fs:allow-write-text-file"
  ]
}
```

Rules:
- Each capability file has: `identifier`, `description`, `windows`, `permissions`
- `windows` array must match window labels in `tauri.conf.json`
- Permissions use `"plugin:permission"` naming convention
- **Least privilege**: only grant permissions the app actually uses
- `core:default` is always needed
- `shell:allow-open` — for `open_repository` (opens path in system file manager)
- `dialog:allow-open` — for native folder picker (Scan Folder)
- `dialog:allow-ask` / `dialog:allow-confirm` — for confirmation dialogs (remove project)
- `fs:allow-read-text-file` — for reading README.md during description generation
- `fs:allow-write-text-file` — for writing `.loopdeck/project.yaml`

## Plugin Management

1. Add crate to `Cargo.toml`:
   ```toml
   tauri-plugin-shell = "2"
   tauri-plugin-dialog = "2"
   tauri-plugin-fs = "2"
   ```

2. Register in `lib.rs`:
   ```rust
   tauri::Builder::default()
       .plugin(tauri_plugin_shell::init())
       .plugin(tauri_plugin_dialog::init())
       .plugin(tauri_plugin_fs::init())
   ```

3. Add npm packages:
   ```bash
   npm install @tauri-apps/plugin-shell
   npm install @tauri-apps/plugin-dialog
   npm install @tauri-apps/plugin-fs
   ```

4. Import in frontend:
   ```typescript
   import { open } from '@tauri-apps/plugin-dialog';
   import { open as shellOpen } from '@tauri-apps/plugin-shell';
   ```

## IPC Design (Commands)

- Command names are snake_case in both Rust and JS
- Rust: `#[tauri::command] pub async fn scan_folder(...)` → JS: `invoke('scan_folder', {...})`
- All commands are registered once in `generate_handler!` macro in `lib.rs`
- Arguments use camelCase in JS, snake_case in Rust — Tauri handles conversion automatically
- Large return types (e.g., `Vec<DiscoveredRepo>`) serialize via serde_json internally
- Error type is `AppError` with manual `Serialize` impl for structured `{ message, kind }` format

## Frontend IPC Pattern

```typescript
// src/lib/tauri.ts — Typed wrappers, never raw invoke() in components
import { invoke } from '@tauri-apps/api/core';

export async function scanFolder(
  path: string,
  depth?: number
): Promise<DiscoveredRepo[]> {
  return invoke<DiscoveredRepo[]>('scan_folder', { path, depth });
}
```

## Native Dialogs

Use `@tauri-apps/plugin-dialog` for:
- **Folder picker**: `open({ directory: true, multiple: false, title: 'Select Folder to Scan' })`
- **Confirm dialog**: `confirm('Remove this project from the registry?', { title: 'Remove Project' })`
- **Message**: `message('Import complete.', { title: 'Success' })`

## Cross-Platform Paths

- Rust: Use `PathBuf` and `directories::ProjectDirs` for XDG paths
- Config dir resolves to:
  - macOS: `~/Library/Application Support/com.loopdeck.LoopDeck/config.yaml`
  - Linux: `~/.config/loopdeck/config.yaml`
  - Windows: `C:\Users\<user>\AppData\Roaming\loopdeck\LoopDeck\config\config.yaml`

  But we want `~/.config/loopdeck/config.yaml` consistently — use the `dirs` crate or construct from `dirs::home_dir()`:
  ```rust
  let config_dir = dirs::home_dir()
      .ok_or(AppError::Config("Cannot find home directory".into()))?
      .join(".config")
      .join("loopdeck");
  ```

## Build & Bundle

```bash
# Development (hot reload)
npm run tauri dev

# Production build
npm run tauri build

# Generate icons from source PNG (1024x1024)
npm run tauri icon ./icon-source.png

# Lint Rust
cd src-tauri && cargo clippy
```

## Cargo.toml Checklist

```toml
[package]
name = "loopdeck"
version = "0.1.0"
edition = "2021"

[lib]
name = "app_lib"
crate-type = ["lib", "cdylib", "staticlib"]  # all three for cross-platform

[build-dependencies]
tauri-build = { version = "2", features = [] }

[dependencies]
tauri = { version = "2", features = [] }
tauri-plugin-shell = "2"
tauri-plugin-dialog = "2"
tauri-plugin-fs = "2"
serde = { version = "1", features = ["derive"] }
serde_yaml = "0.9"
serde_json = "1"
directories = "6"
thiserror = "2"
tracing = "0.1"
tracing-subscriber = { version = "0.3", features = ["env-filter"] }
chrono = { version = "0.4", features = ["serde"] }
walkdir = "2"
tokio = { version = "1", features = ["full"] }
```
