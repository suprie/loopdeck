# Phase 2 — Crash-safe Persistence Implementation Plan

> **For Hermes:** Use subagent-driven-development skill to implement this plan task-by-task.

**Goal:** Make LoopDeck's critical state — the global registry, project-local config, memory files, generated Claude settings, and whole-transcript rewrites — survive process crashes and full disks without truncation or data loss. Add a single shared atomic-write helper used by every critical write site, and a last-known-good backup for the registry so a malformed primary never gets overwritten with a fresh default.

**Architecture:** A new `persist` module owns one primitive — `atomic_write(path, contents)`: write to a sibling temp file, `fsync`, then same-filesystem `rename` (atomic on POSIX, `ReplaceFile` semantics on Windows via `same_file::hard_link_file` fallback if needed — but the temp-file-beside-target + rename pattern works portably with `std::fs::rename` on Windows for same-volume moves, which is the case here since the temp is created in the target's parent). The registry gains a `.bak` sibling written atomically *before* every primary overwrite, and `GlobalConfig::load` recovers from the backup instead of silently defaulting when the primary is malformed. No database, no event sourcing, no journal — just the write pattern the filesystem already gives us for free.

**Tech Stack:** Rust stdlib (`std::fs`, `std::path`), existing `serde_yaml`/`serde_json`, no new crates.

**Source of truth:** `docs/PRD-trust-boundary-hardening.md` FR2 + Phase 2; `.loopdeck/loops.md` Gate A items 3 (Crash-safe critical state) and 4 (Recoverable registry).

---

## Current State (reference — what's wrong today)

### Write sites that truncate-on-crash

Every critical write today uses `std::fs::write(path, contents)` directly. That's a truncate-then-write: if the process is killed, the disk fills, or the OS drops the write between the open and the final byte, the file is left partial. Concrete sites:

- `config.rs:231` — `GlobalConfig::save()` overwrites `~/.config/loopdeck/config.yaml` (the **registry** — the global index of all registered projects + provider config). A crash here loses the project list.
- `memory.rs:121, 126, 178` — `ensure_memory_files` (safe — only writes when absent) and `toggle_loop_step` (unsafe — rewrites `loops.md`).
- `epic.rs:267, 333, 453` — `toggle_epic_step`, `toggle_prd_step`, and a PRD-write helper all rewrite their markdown files in place.
- `skills.rs:350` — `setup_hooks` writes `.claude/settings.json` (the generated permission allowlist).
- `conversation.rs:446` — `write_full_conversation` rewrites `active.jsonl` (used by whole-transcript paths). Append-only writes (`conversation.rs:467` `append_turn`) are already safe-ish but should be flushed.

### Malformed-registry behavior gap

`config.rs:204-214` `GlobalConfig::load()`:
```rust
let contents = std::fs::read_to_string(&config_path)?;
let config: GlobalConfig = serde_yaml::from_str(&contents)?;
Ok(config)
```
On malformed YAML this returns `Err`, and `lib.rs:29-37` catches it with `unwrap_or_else(|e| { warn!(...); let fresh = GlobalConfig::default(); fresh.save(); fresh })` — **the fresh default is persisted over the malformed primary**. The PRD calls this out explicitly (FR2: "On malformed registry data, do not overwrite the malformed file. Load the backup when valid or return a recoverable startup error"). Today: the malformed file is destroyed.

### What's already safe

- `append_turn` (`conversation.rs:467`) — `OpenOptions::new().create(true).append(true)`. Append-mode writes are atomic at the line granularity on local filesystems under most conditions; the PRD accepts this for transcripts (FR2: "Append-only transcript writes must be line-atomic"). Worth a `flush()` addition but not a rewrite.
- `ensure_memory_files` — only writes when the file doesn't exist, so there's nothing to truncate.

---

## Pre-flight (Task 0): baseline verification

**Step 1:** Confirm the tree is green.
```
cd src-tauri && cargo fmt --check && cargo clippy --all-targets && cargo test --lib
cd .. && npm run build
```
Expected: 258 passed / 0 failed / 8 ignored, build passes, 0 new clippy warnings.

---

## Task 1: Add the `persist` module with `atomic_write`

**Objective:** One shared, tested atomic-write primitive.

**Files:**
- Create: `src-tauri/src/persist.rs`
- Modify: `src-tauri/src/lib.rs` (add `mod persist;`)

**Step 1:** Create `src-tauri/src/persist.rs`:

```rust
//! Atomic file writes for critical LoopDeck state.
//!
//! `std::fs::write` is truncate-then-write: a crash, full disk, or OS dropped
//! write between the open and the final byte leaves a partial file. For
//! recoverable state (the registry, project config, loops, decisions, PRDs,
//! generated Claude settings) that's a data-loss bug.
//!
//! This module's single primitive, [`atomic_write`], writes to a sibling
//! temporary file in the *same directory* as the target, flushes + fsyncs it,
//! then renames it over the target. Same-directory rename is atomic on POSIX
//! and atomic-with-replace semantics on Windows for same-volume moves (the
//! temp is in the target's parent, so always same-volume). The old file's
//! contents survive until the rename commits, then are replaced in one step.
//!
//! See `docs/PRD-trust-boundary-hardening.md` FR2.

use std::fs::{self, File};
use std::io::{self, Write};
use std::path::Path;

/// Suffix for the sibling temp file. `.<pid>.tmp` so two writers in the same
/// directory (e.g. concurrent saves on different state files) don't collide.
/// Not `.tmp` alone because a stale temp from a crashed prior run would
/// otherwise be shared state.
fn temp_suffix() -> String {
    format!(".{}.tmp", std::process::id())
}

/// Atomically write `contents` to `path`.
///
/// Writes to `<path><temp_suffix()>` in the same directory, flushes + fsyncs,
/// then renames over `path`. On success the temp file is gone (renamed). On
/// any error the temp file is removed and the original `path` is untouched.
///
/// Creates parent directories if needed. Does NOT apply permission
/// restrictions — callers that need a permission floor (e.g. `config.yaml`
/// gets 0600) apply it after this returns, same as today.
pub fn atomic_write(path: &Path, contents: &str) -> io::Result<()> {
    // Parent must exist. `File::create` in a missing dir fails with NotFound;
    // create the dir first so a fresh install doesn't trip.
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }

    let temp_path = path.with_extension(
        path.extension()
            .map(|e| {
                let mut s = e.to_os_string();
                s.push(&temp_suffix());
                s
            })
            .unwrap_or_else(|| temp_suffix().into()),
    );

    // Write + flush + fsync the temp file. fsync is what makes the write
    // durable: without it a crash after the rename can still lose data that
    // only made it to the OS page cache.
    let write_result = (|| -> io::Result<()> {
        let mut file = File::create(&temp_path)?;
        file.write_all(contents.as_bytes())?;
        file.flush()?;
        // fsync is fallible on some filesystems (network mounts) but a failure
        // here means we can't guarantee durability — propagate so the caller
        // knows the write wasn't durable, rather than silently succeeding.
        file.sync_all()?;
        drop(file); // Windows won't rename a file with an open handle.
        Ok(())
    })();

    if let Err(e) = write_result {
        // Clean up the partial temp file. Best-effort — a failure here is
        // logged but doesn't mask the original error.
        let _ = fs::remove_file(&temp_path);
        return Err(e);
    }

    // Same-directory rename: atomic on POSIX, atomic-on-same-volume on
    // Windows. If the target is on a different volume from its parent we'd
    // need a cross-device fallback, but that can't happen here (the temp is
    // in the target's parent).
    fs::rename(&temp_path, path)?;

    Ok(())
}

/// Read `path`, returning `Ok(Some(contents))` if it exists, `Ok(None)` if it
/// doesn't. Thin wrapper so callers don't pattern-match on `NotFound`
/// everywhere.
pub fn read_if_exists(path: &Path) -> io::Result<Option<String>> {
    match fs::read_to_string(path) {
        Ok(contents) => Ok(Some(contents)),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir() -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "loopdeck-persist-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn atomic_write_creates_new_file() {
        let dir = temp_dir();
        let target = dir.join("newfile.yaml");
        atomic_write(&target, "hello").unwrap();
        assert_eq!(fs::read_to_string(&target).unwrap(), "hello");
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn atomic_write_overwrites_existing() {
        let dir = temp_dir();
        let target = dir.join("existing.yaml");
        fs::write(&target, "old").unwrap();
        atomic_write(&target, "new").unwrap();
        assert_eq!(fs::read_to_string(&target).unwrap(), "new");
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn atomic_write_creates_parent_dirs() {
        let dir = temp_dir();
        let target = dir.join("nested/deep/file.yaml");
        atomic_write(&target, "x").unwrap();
        assert_eq!(fs::read_to_string(&target).unwrap(), "x");
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn atomic_write_leaves_no_temp_on_success() {
        let dir = temp_dir();
        let target = dir.join("clean.yaml");
        atomic_write(&target, "data").unwrap();
        // No `.tmp` sibling should remain.
        let entries: Vec<_> = fs::read_dir(&dir).unwrap().collect();
        assert_eq!(entries.len(), 1, "expected only the target file to remain");
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn atomic_write_leaves_original_untouched_on_failure() {
        // Simulate a write failure by pointing at an unwritable target: a
        // path whose parent is a file, not a directory. create_dir_all will
        // fail, so the original (if any) must survive.
        let dir = temp_dir();
        let blocker = dir.join("blocker");
        fs::write(&blocker, "block").unwrap();
        let target = blocker.join("underneath.yaml"); // parent is a file
        let result = atomic_write(&target, "new");
        assert!(result.is_err(), "should fail when parent is a file");
        // The blocker file is untouched.
        assert_eq!(fs::read_to_string(&blocker).unwrap(), "block");
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn read_if_exists_returns_none_for_missing() {
        let dir = temp_dir();
        let missing = dir.join("nope.yaml");
        assert!(read_if_exists(&missing).unwrap().is_none());
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn read_if_exists_returns_contents_for_present() {
        let dir = temp_dir();
        let present = dir.join("here.yaml");
        fs::write(&present, "data").unwrap();
        assert_eq!(
            read_if_exists(&present).unwrap().as_deref(),
            Some("data")
        );
        fs::remove_dir_all(&dir).unwrap();
    }
}
```

**Step 2:** Add `mod persist;` to `lib.rs` (alphabetically between `permission` and `project`, matching the existing `mod` ordering at `lib.rs:10-16`).

**Step 3:** Verify:
```
cd src-tauri && cargo check --lib && cargo test --lib persist
```
Expected: compiles, 7 new tests pass.

**Step 4:** Full suite:
```
cargo test --lib
```
Expected: 265 passed / 0 failed / 8 ignored (was 258; +7).

**Step 5:** Commit
```
git add src-tauri/src/persist.rs src-tauri/src/lib.rs
git commit -m "feat(persist): add atomic_write helper for crash-safe critical state"
```

---

## Task 2: Migrate `GlobalConfig::save` to `atomic_write` + add last-known-good backup

**Objective:** The registry is the single most critical file — the index of every registered project. It gets atomic writes + a `.bak` that survives primary corruption.

**Files:**
- Modify: `src-tauri/src/config.rs` — `save()`, `load()`, add `save_with_backup()`
- Modify: `src-tauri/src/lib.rs` — no change (already calls `save()`)

**Step 1:** Read the current `save()` and `load()`:
```
sed -n '200,235p' src-tauri/src/config.rs
```

**Step 2:** Add `use crate::persist;` near the top of `config.rs` (after the existing `use std::path::...`).

**Step 3:** Rewrite `save()` to back up + atomic-write:
```rust
    /// Save global config to `~/.config/loopdeck/config.yaml`.
    ///
    /// Crash-safe via [`persist::atomic_write`]: writes to a sibling temp
    /// file, fsyncs, then same-directory renames over the primary. Before
    /// overwriting, copies the existing primary (if any) to `config.yaml.bak`
    /// so a malformed future primary can be recovered from the backup.
    ///
    /// Also applies an owner-only permission floor (0600 on Unix) as
    /// defense-in-depth: the auth token itself lives in the OS keychain now
    /// (see `secrets`), but the file still holds provider config, so we don't
    /// rely on the process umask to keep it private.
    pub fn save(&self) -> Result<(), AppError> {
        let config_path = Self::config_path()?;

        // Preserve the current primary as last-known-good before overwriting.
        // Best-effort: a missing primary (first launch) or a backup failure
        // is logged but doesn't abort the save — the primary is the source of
        // truth, the backup is a recovery floor.
        if config_path.exists() {
            let backup = backup_path(&config_path);
            if let Err(e) = std::fs::copy(&config_path, &backup) {
                tracing::warn!(
                    "failed to update registry backup at {}: {e}",
                    backup.display()
                );
            }
        }

        let contents = serde_yaml::to_string(self)?;
        persist::atomic_write(&config_path, &contents)?;
        restrict_file_perms(&config_path);

        Ok(())
    }
```

**Step 4:** Rewrite `load()` to recover from backup instead of defaulting when the primary is malformed:
```rust
    /// Load global config from `~/.config/loopdeck/config.yaml`.
    ///
    /// Recovery order:
    /// 1. Primary exists + parses → load it.
    /// 2. Primary missing → fresh default (first launch).
    /// 3. Primary malformed → try the `.bak`. If the backup parses, load it
    ///    and warn. If the backup is also malformed (or missing), return
    ///    `Err` — the caller (`lib.rs`) MUST NOT silently overwrite the
    ///    malformed primary with a fresh default, per PRD FR2. The user gets
    ///    a visible startup error and the primary file is preserved for
    ///    manual recovery.
    pub fn load() -> Result<Self, AppError> {
        let config_path = Self::config_path()?;
        let backup = backup_path(&config_path);

        // Primary missing entirely → first launch.
        if !config_path.exists() {
            return Ok(Self::default());
        }

        // Primary exists — try to parse it.
        let contents = std::fs::read_to_string(&config_path)?;
        match serde_yaml::from_str::<GlobalConfig>(&contents) {
            Ok(config) => Ok(config),
            Err(primary_err) => {
                // Primary is malformed. Do NOT overwrite it. Try the backup.
                tracing::warn!(
                    "malformed registry at {}: {primary_err}",
                    config_path.display()
                );
                if backup.exists() {
                    let backup_contents = std::fs::read_to_string(&backup)?;
                    match serde_yaml::from_str::<GlobalConfig>(&backup_contents) {
                        Ok(config) => {
                            tracing::warn!(
                                "recovered registry from backup at {}",
                                backup.display()
                            );
                            return Ok(config);
                        }
                        Err(backup_err) => {
                            tracing::warn!(
                                "backup at {} also malformed: {backup_err}",
                                backup.display()
                            );
                        }
                    }
                }
                // Both missing/malformed — surface the error, preserve the
                // primary for manual recovery.
                Err(AppError::Config(format!(
                    "registry at {} is malformed and no valid backup was found; \
                     the file has been preserved for manual recovery. Parse error: {primary_err}",
                    config_path.display()
                )))
            }
        }
    }
```

**Step 5:** Add the `backup_path` helper at the bottom of the file (near `restrict_file_perms`):
```rust
/// Sibling `.bak` path for a registry primary.
fn backup_path(primary: &Path) -> PathBuf {
    let mut name = primary
        .file_name()
        .expect("registry path has a file name")
        .to_os_string();
    name.push(".bak");
    primary.with_file_name(name)
}
```

**Step 6:** Update `lib.rs:28-37` — the `unwrap_or_else` that silently overwrites must become a hard error path:
```rust
    let mut config = match GlobalConfig::load() {
        Ok(c) => c,
        Err(e) => {
            // Per PRD FR2: a malformed registry MUST NOT be silently
            // overwritten with a fresh default. The primary is preserved on
            // disk for manual recovery. Surface a structured log + exit so
            // the user knows their project list is intact-but-recoverable,
            // not silently wiped.
            tracing::error!("registry load failed: {e}");
            tracing::error!(
                "the malformed registry has NOT been overwritten; \
                 repair it manually or delete it to start fresh"
            );
            std::process::exit(1);
        }
    };
```
Remove the `fresh.save()` call — it was the exact overwrite the PRD prohibits.

**Step 7:** Add a test for the recovery path. In `config.rs` `#[cfg(test)]`:
```rust
    #[test]
    fn load_recovers_from_backup_when_primary_malformed() {
        let dir = std::env::temp_dir().join(format!(
            "loopdeck-registry-recovery-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        std::env::set_var("LOOPDECK_CONFIG_DIR", &dir);

        // Seed a valid backup, then a malformed primary.
        let primary = dir.join("config.yaml");
        let backup = dir.join("config.yaml.bak");
        std::fs::write(&backup, "agent:\n  model: backup-model\n").unwrap();
        std::fs::write(&primary, ":::not yaml:::").unwrap();

        let config = GlobalConfig::load().unwrap();
        assert_eq!(
            config.agent.and_then(|a| a.model),
            Some("backup-model".into()),
            "should recover the backup's contents"
        );

        // Primary must still be malformed on disk — not overwritten.
        assert_eq!(
            std::fs::read_to_string(&primary).unwrap(),
            ":::not yaml:::",
            "malformed primary must be preserved for manual recovery"
        );

        std::env::remove_var("LOOPDECK_CONFIG_DIR");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn load_errors_when_both_primary_and_backup_malformed() {
        let dir = std::env::temp_dir().join(format!(
            "loopdeck-registry-both-bad-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        std::env::set_var("LOOPDECK_CONFIG_DIR", &dir);

        let primary = dir.join("config.yaml");
        let backup = dir.join("config.yaml.bak");
        std::fs::write(&primary, ":::bad primary:::").unwrap();
        std::fs::write(&backup, ":::bad backup:::").unwrap();

        let result = GlobalConfig::load();
        assert!(
            result.is_err(),
            "should error when neither primary nor backup parses"
        );

        std::env::remove_var("LOOPDECK_CONFIG_DIR");
        std::fs::remove_dir_all(&dir).unwrap();
    }
```

**NOTE:** The `LOOPDECK_CONFIG_DIR` env-var override only works if `config_path()` reads it. Check whether `config_path()` already supports an env override; if not, add one gated behind `#[cfg(test)]` so the tests can redirect the path. If adding the override is more than a 3-line change, instead refactor these tests to construct a `GlobalConfig` via a `load_from(primary, backup)` test-only helper that takes explicit paths.

**Step 8:** Verify:
```
cd src-tauri && cargo check --lib && cargo clippy --all-targets && cargo test --lib config
```
Expected: compiles, 0 new clippy warnings, config tests pass (including the 2 new recovery tests).

**Step 9:** Full suite:
```
cargo test --lib
```
Expected: 267 passed (was 265 after Task 1; +2).

**Step 10:** Commit
```
git add src-tauri/src/config.rs src-tauri/src/lib.rs
git commit -m "fix(config): atomic registry writes + last-known-good backup recovery

GlobalConfig::save now uses persist::atomic_write (temp+fsync+rename) so a
crash during save leaves either the old complete file or the new complete
file, never a truncated primary. Before overwriting, copies the existing
primary to config.yaml.bak.

GlobalConfig::load recovers from the backup when the primary is malformed,
rather than silently defaulting + overwriting the malformed file with an
empty registry (the pre-Phase-2 behavior, which turned recoverable
corruption into silent data loss). When both primary and backup are
malformed, returns Err — the caller (lib.rs startup) logs + exits rather
than wiping the user's project list."
```

---

## Task 3: Migrate project-local critical writes (`memory.rs`, `epic.rs`)

**Objective:** Apply the same atomic-write pattern to the project-scoped files the PRD names: `.loopdeck/loops.md`, `.loopdeck/decisions.md`, PRDs.

**Files:**
- Modify: `src-tauri/src/memory.rs:121,126,178` — `ensure_memory_files` + `toggle_loop_step`
- Modify: `src-tauri/src/epic.rs:267,333,453` — `toggle_epic_step`, `toggle_prd_step`, PRD write

**Step 1:** Add `use crate::persist;` to both files (if not already imported via the prelude pattern).

**Step 2:** Swap each `std::fs::write(&path, contents)?` at the named sites for `persist::atomic_write(&path, &contents)?`. The signatures are almost identical; the only difference is `atomic_write` takes `&str` (which these all already produce) and returns `io::Result` (which `?` already converts via `From<io::Error>` for whatever the function's error type is — verify each call site's return type converts, or add `.map_err(AppError::from)` if needed).

Specific sites:
- `memory.rs:178` — `toggle_loop_step`: `std::fs::write(&loops_file, new_content)?` → `persist::atomic_write(&loops_file, &new_content)?`
- `epic.rs:267` — `toggle_epic_step`: `std::fs::write(&loops_file, rewritten)?` → `persist::atomic_write(&loops_file, &rewritten)?`
- `epic.rs:333` — `toggle_prd_step`: `std::fs::write(&prd_path, new_content)?` → `persist::atomic_write(&prd_path, &new_content)?`
- `epic.rs:453` — generic PRD write: `std::fs::write(&path, content)?` → `persist::atomic_write(&path, &content)?`

**Leave alone:**
- `memory.rs:121,126` `ensure_memory_files` — these only write when the file is absent; nothing to truncate. But for consistency, you *can* swap them too — atomic_write to a fresh path works fine and deduplicates the pattern. Optional; do it if the code gets simpler, skip if it adds noise.
- All `#[cfg(test)]` writes in `memory.rs`/`epic.rs` — they're test fixtures that own their temp dirs, not recoverable state. Leave as `std::fs::write` for speed (atomic_write's fsync slows tests).

**Step 3:** Verify:
```
cd src-tauri && cargo check --lib && cargo clippy --all-targets && cargo test --lib
```
Expected: compiles, 0 new clippy warnings, all 267 tests still pass (no behavior change for offline tests).

**Step 4:** Commit
```
git add src-tauri/src/memory.rs src-tauri/src/epic.rs
git commit -m "fix(persist): atomic writes for loops/decisions/PRD state files"
```

---

## Task 4: Migrate generated Claude settings (`skills.rs`)

**Objective:** The generated `.claude/settings.json` — the permission allowlist — must survive a mid-write crash too.

**Files:**
- Modify: `src-tauri/src/skills.rs:350` — `setup_hooks` final settings write

**Step 1:** Swap `std::fs::write(&settings_path, formatted)?` for `persist::atomic_write(&settings_path, &formatted)?`.

**Step 2:** Verify:
```
cd src-tauri && cargo check --lib && cargo test --lib skills
```
Expected: all skills tests pass (including `test_setup_hooks_writes_curated_allowlist` from Phase 1).

**Step 3:** Full suite + commit:
```
cargo test --lib
git add src-tauri/src/skills.rs
git commit -m "fix(persist): atomic write for generated .claude/settings.json"
```

---

## Task 5: Migrate whole-transcript rewrites (`conversation.rs`)

**Objective:** `write_full_conversation` rewrites `active.jsonl` — make it atomic. Append-only writes get a `flush()` so they're durable.

**Files:**
- Modify: `src-tauri/src/conversation.rs:446` — `write_full_conversation`
- Modify: `src-tauri/src/conversation.rs:467` — `append_turn` (add flush)

**Step 1:** Swap `write_full_conversation`'s `std::fs::write(active_file(repo_path), content)?` for `persist::atomic_write(&active_file(repo_path), &content)?`.

**Step 2:** In `append_turn`, add a flush before the `Ok(())`:
```rust
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(active_file(repo_path))?;
    file.write_all(line.as_bytes())?;
    file.flush()?; // make the append durable before returning
    Ok(())
```
Leave append-mode as-is — the PRD accepts line-atomic appends, and a full fsync per turn line would tank throughput.

**Step 3:** Tolerate a partial final line on load. Find where `active.jsonl` is read (likely `load_conversation` near the top of `conversation.rs`) and, on a JSON parse failure of the *last* line, drop it and continue rather than failing the whole load. This matches PRD FR2: "tolerate a partial final line during recovery".
```rust
    // When parsing turns, the final line may be partial (a crash mid-append).
    // Skip unparseable lines at the end rather than failing the whole load.
    let mut turns: Vec<ConversationTurn> = Vec::new();
    for line in contents.lines() {
        if line.trim().is_empty() {
            continue;
        }
        match serde_json::from_str::<ConversationTurn>(line) {
            Ok(turn) => turns.push(turn),
            Err(e) => {
                tracing::warn!("skipping unparseable transcript line: {e}");
                // Continue — a single bad line (partial append, disk hiccup)
                // shouldn't lose every prior turn.
            }
        }
    }
```
Check whether `load_conversation` already does this; if it currently fails fast, the change is a behavior improvement worth calling out in the commit message.

**Step 4:** Add a test for the partial-line tolerance:
```rust
    #[test]
    fn load_conversation_tolerates_partial_final_line() {
        let dir = /* temp dir */;
        // Write a valid turn, then a partial (unterminated JSON) line.
        let valid = r#"{"role":"user","text":"hello","timestamp":"...","usage":null}"#;
        std::fs::write(
            active_file(&dir),
            format!("{valid}\n{{\"role\":\"user\",\"text\":\"partial"),
        ).unwrap();
        let turns = load_conversation(&dir).unwrap();
        assert_eq!(turns.len(), 1, "valid turn should load; partial line skipped");
        // ... cleanup
    }
```

**Step 5:** Verify:
```
cd src-tauri && cargo check --lib && cargo clippy --all-targets && cargo test --lib conversation
```
Expected: compiles, conversation tests pass including the new partial-line test.

**Step 6:** Commit
```
git add src-tauri/src/conversation.rs
git commit -m "fix(persist): atomic transcript rewrites + durable appends + partial-line tolerance"
```

---

## Task 6: Loops.md + decisions.md records + Gate A check-off

**Objective:** Document the work and mark Gate A items 3 + 4 done.

**Files:**
- Modify: `.loopdeck/loops.md` (History entry + check off items 3 + 4)
- Modify: `.loopdeck/decisions.md` (new decision record)

**Step 1:** Add a History entry at the top of `## History` following the Phase 1 entry format. Cover: the pre-Phase-2 risk (truncate-then-write everywhere, malformed registry silently overwritten), the new `persist` module, the backup-recovery flow, the sites migrated, the partial-line tolerance for transcripts. Real verification numbers from Task 5's final run.

**Step 2:** Add a decision record "Atomic writes via temp-file + fsync + same-dir rename; last-known-good backup for the registry". Cover the alternatives considered (SQLite WAL, the `atomicwrites` crate, just using `std::fs::write` with an `O_TRUNC`-avoidance hack) and why the in-module primitive won.

**Step 3:** In `.loopdeck/loops.md` Gate A, check off:
```
- [x] **Crash-safe critical state:** add one shared atomic-write helper and use it for the registry, `project.yaml`, `loops.md`, PRDs, and generated Claude settings
- [x] **Recoverable registry:** keep one last-known-good backup and never overwrite a malformed primary registry with a fresh default
```

**Step 4:** Final verification:
```
cd src-tauri && cargo fmt --check && cargo clippy --all-targets && cargo test --lib
cd .. && npm run build
```
Expected: all green, test count up by ~10 from Phase 1's 258 (7 persist + 2 registry recovery + 1 partial-line).

**Step 5:** Commit
```
git add .loopdeck/loops.md .loopdeck/decisions.md
git commit -m "docs(persist): record Phase 2 crash-safe persistence + check off Gate A items 3+4"
```

---

## Risks / Tradeoffs / Open Questions

- **`fsync` performance.** Every critical save now pays an fsync. In practice these saves are infrequent (registry updates, loop toggles, settings regen) — not per-token — so the latency is invisible. Transcript *appends* deliberately skip fsync (only `flush()` to the OS page cache) for throughput; the PRD accepts line-atomic appends as sufficient.
- **Windows rename semantics.** `std::fs::rename` on Windows uses `MoveFileEx` with `MOVEFILE_REPLACE_EXISTING` when both paths are on the same volume, which is atomic-with-replace. The temp is always in the target's parent, so same-volume is guaranteed. If a future code path writes across volumes, this assumption breaks — but that can't happen for the named sites.
- **Backup file grows unbounded?** No — `config.yaml.bak` is overwritten on every save, so it's at most 1× the primary size. Worth a one-line note in the loops.md entry.
- **`LOOPDECK_CONFIG_DIR` env override may not exist.** Task 2 Step 7's tests assume it does. Check `config_path()` first; if there's no test override, either add one (small, gated `#[cfg(test)]`) or refactor the tests to call a `load_from(primary_path, backup_path)` helper. The second option is cleaner and doesn't touch production code paths.
- **What about `secrets.rs` (the keychain)?** Out of scope — the OS keychain is already atomic from the app's perspective (it's a transactional API). The auth-token plaintext floor in `config.yaml` is covered by Task 2 (it's part of the registry save).
- **Migration of existing installs.** No migration needed — the first save after upgrading creates the `.bak` and switches to atomic writes. Existing primary files keep working.

## Out of Scope (deferred per PRD / Gate A)

- Bounded expiry for parked approvals/questions (Phase 4)
- Run-record persistence for session recovery (Phase 4)
- Atomic writes for non-critical state (logs, caches) — not recoverable, not worth the fsync cost
- Moving the registry off YAML entirely (considered and rejected in the PRD — non-goal)
