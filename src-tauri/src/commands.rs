use crate::agents::AgentResponse;
use crate::agents::ClaudeEvent;
use crate::claude_session::{
    ClaudeSession, InterruptSlot, PermissionSlot, QuestionAnswers, QuestionSlot,
};
use crate::config::{self, AgentConfig, GlobalConfig, ProjectEntry, ProjectStatus, RunState};
use crate::conversation::{self, ConversationSummary, ConversationTurn};
use crate::epic::{self, Epic};
use crate::error::AppError;
use crate::git;
use crate::memory::{self, Decision, LoopStatus};
use crate::permission::Decision as PermissionDecision;
use crate::permission::PermissionPolicy;
use crate::project::{self, ProjectMeta};
use crate::retry;
use crate::scanner::{self, DiscoveredRepo};
use crate::secrets;
use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use tauri::ipc::Channel;
use tauri::State;
use tracing::{debug, info, warn};

/// Shared application state managed by Tauri.
///
/// `claude_sessions` uses a two-layer lock so projects run concurrently while
/// turns within one project serialize (one process, one stdin):
/// - **Outer** `std::sync::Mutex` guards the map for insert/lookup only —
///   held for microseconds, NEVER across `.await` (would deadlock / is unsound
///   across threads). The guard is dropped before any async work.
/// - **Inner** `tokio::sync::Mutex` per project, held for one full turn
///   (seconds–minutes). Different projects take different inner locks, so
///   they run in true parallel.
pub struct AppState {
    pub config: Mutex<GlobalConfig>,
    pub claude_sessions: Mutex<HashMap<PathBuf, Arc<tokio::sync::Mutex<ClaudeSession>>>>,
    /// Per-project pending `AskUserQuestion` slots. When Claude asks a
    /// question mid-turn, the read loop parks on the slot's oneshot receiver;
    /// the `agent_answer_question` command pops the sender here to deliver the
    /// user's answers and wake the parked turn. Keyed by project path so
    /// different projects' questions can't collide.
    pub pending_answers: Mutex<HashMap<PathBuf, QuestionSlot>>,
    /// Per-project pending manual-approval slots for `can_use_tool` requests on
    /// mutating/executing tools (Bash/Edit/Write/…). When such a tool call
    /// arrives, the read loop parks on the slot's oneshot receiver; the
    /// `agent_answer_permission` command pops the sender here to deliver the
    /// user's Allow/Deny and wake the parked turn. Mirrors `pending_answers`.
    pub pending_permissions: Mutex<HashMap<PathBuf, PermissionSlot>>,
    /// Per-project interrupt slots for graceful Stop. The streaming read loop
    /// installs a fresh oneshot sender per turn and `select!`s on the
    /// receiver; `agent_interrupt` pops + fires the sender, the loop wakes and
    /// writes the `interrupt` control_request, ending the turn while keeping
    /// the live process (and its context) alive.
    pub interrupt_slots: Mutex<HashMap<PathBuf, InterruptSlot>>,
}

/// A single child entry of a project directory, for the chat composer's
/// `@`-mention file/folder autocomplete. `path` is project-relative (forward
/// slashes) so the frontend can insert it verbatim as `@<path>`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DirEntry {
    /// Entry basename (e.g. `Chat.tsx`).
    pub name: String,
    /// Whether the entry is a directory.
    pub is_dir: bool,
    /// Project-relative path (forward slashes), e.g. `src/components/detail/Chat.tsx`.
    pub path: String,
}

/// A skill installed for a project, surfaced by the composer's `/`-skill
/// discovery menu. Read from `<repo>/.claude/skills/<dir>/SKILL.md` — the files
/// `copy_skills()` writes during project bootstrap.
///
/// `name` is the SKILL.md frontmatter `name` (e.g. `loopdeck:rust-expert`),
/// which is what the `claude` CLI invokes the skill by — so the frontend
/// inserts it verbatim as `/<name>`. It is distinct from `directory`, the
/// on-disk folder name (`loopdeck-rust-expert`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillEntry {
    /// Frontmatter `name` — the invocation token the `claude` CLI recognizes.
    pub name: String,
    /// On-disk skill directory name (e.g. `loopdeck-rust-expert`).
    pub directory: String,
    /// Frontmatter `description`, used by the menu to show what each skill does.
    /// Empty string if the SKILL.md has no `description:` field.
    pub description: String,
    /// Frontmatter `argument-hint`, shown as a dimmed placeholder next to the
    /// skill name (e.g. `<prd-file-path>`) to cue the user what to type after.
    /// Empty string when the skill takes no arguments.
    #[serde(default)]
    pub argument_hint: String,
}

/// List the direct children of a directory inside a project, for the
/// composer's `@`-mention autocomplete.
///
/// `path` is the canonical project root (the same key used by every other
/// project command); `subdir` is a project-relative path identifying which
/// directory to list (empty string = project root). The user navigates into
/// subfolders by selecting folders, which the frontend turns into successive
/// calls with deeper `subdir` values.
///
/// Security: `subdir` is joined onto the canonicalized root and the result is
/// canonicalized and checked with `starts_with(&root)`, so `../` escapes and
/// symlink-redirected paths are rejected. Hidden entries (dotfiles) and the
/// same `IGNORED_DIRS` used by project scanning are filtered out.
#[tauri::command]
pub async fn list_dir_entries(path: String, subdir: String) -> Result<Vec<DirEntry>, AppError> {
    debug!("list_dir_entries called: path={path:?}, subdir={subdir:?}");

    // Canonicalize the root first — every comparison below depends on an
    // absolute, normalized form. `subdir` is then resolved relative to it.
    let root = PathBuf::from(&path)
        .canonicalize()
        .map_err(|e| AppError::InvalidPath(format!("Invalid project path: {e}")))?;

    // An empty subdir means "list the root itself". `join("")` would behave
    // correctly, but being explicit avoids ambiguity on edge cases (trailing
    // slashes, root-relative weirdness on some platforms).
    let target = if subdir.is_empty() {
        root.clone()
    } else {
        root.join(&subdir)
    };

    // Canonicalize the target too, then verify it's still under the root. This
    // is the path-escape guard: a `subdir` like `../../etc/passwd` resolves to
    // a path outside `root`, and symlinks pointing out of the project likewise
    // resolve out, so both are rejected here.
    let target = target
        .canonicalize()
        .map_err(|e| AppError::InvalidPath(format!("Invalid subdirectory: {e}")))?;
    if !target.starts_with(&root) {
        return Err(AppError::InvalidPath(
            "Subdirectory is outside the project root".to_string(),
        ));
    }

    let read_dir = std::fs::read_dir(&target).map_err(AppError::Io)?;

    let mut dirs: Vec<DirEntry> = Vec::new();
    let mut files: Vec<DirEntry> = Vec::new();

    for entry in read_dir {
        let entry = entry.map_err(AppError::Io)?;
        let file_type = match entry.file_type() {
            Ok(ft) => ft,
            // Couldn't stat (broken symlink, permission) — skip rather than
            // failing the whole listing.
            Err(_) => continue,
        };
        let name = entry.file_name().to_string_lossy().into_owned();

        // Filter hidden entries (dotfiles/dotdirs) and ignored build/dep dirs
        // so the autocomplete surface stays relevant. Mirrors what gets hidden
        // during project scanning.
        if name.starts_with('.') || scanner::IGNORED_DIRS.iter().any(|d| *d == name) {
            continue;
        }

        // Build the project-relative path with forward slashes so the frontend
        // can insert `@<path>` verbatim regardless of platform.
        let rel = entry
            .path()
            .strip_prefix(&root)
            .map(|p| p.to_string_lossy().replace('\\', "/"))
            .unwrap_or_else(|_| name.clone());

        let item = DirEntry {
            name: name.clone(),
            is_dir: file_type.is_dir(),
            path: rel,
        };
        if file_type.is_dir() {
            dirs.push(item);
        } else {
            files.push(item);
        }
    }

    // Sort each bucket case-insensitively, directories first then files — the
    // conventional file-explorer ordering, and stable for arrow-key navigation.
    dirs.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    files.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    dirs.append(&mut files);

    Ok(dirs)
}

/// Recursively search the project for files/folders whose path contains the
/// query string (case-insensitive). Used by the `@`-mention autocomplete when
/// the user types a filter after `@` — it searches the whole tree at once
/// rather than requiring the user to drill in folder by folder.
///
/// Results are ranked and capped (`max_results`, default 50) so a giant
/// monorepo doesn't flood the popup. Ranking:
///   1. Exact basename match (score 0)
///   2. Basename starts with query (score 1)
///   3. Basename contains query (score 2)
///   4. Path contains query elsewhere (score 3)
/// Within a tier, shorter paths rank first (shallower, less noise).
///
/// `walk_root` is a recursion helper that walks `dir` and pushes matches.
fn walk_root(dir: &Path, root: &Path, query_lower: &str, out: &mut Vec<(DirEntry, u8, usize)>) {
    let read_dir = match std::fs::read_dir(dir) {
        Ok(rd) => rd,
        Err(_) => return, // permission denied / vanished — skip this subtree
    };
    for entry in read_dir.flatten() {
        let file_type = match entry.file_type() {
            Ok(ft) => ft,
            Err(_) => continue,
        };
        let name = entry.file_name().to_string_lossy().into_owned();

        // Skip hidden entries and the same ignored dirs as scanning. We do NOT
        // descend into them either, so node_modules/target/.git are pruned
        // entirely — critical for performance and result relevance.
        if name.starts_with('.') || scanner::IGNORED_DIRS.iter().any(|d| *d == name) {
            continue;
        }

        let is_dir = file_type.is_dir();
        // Build the project-relative path (forward slashes for the frontend).
        let rel = entry
            .path()
            .strip_prefix(root)
            .map(|p| p.to_string_lossy().replace('\\', "/"))
            .unwrap_or_else(|_| name.clone());

        // Match against basename AND full relative path. Score by closeness of
        // the match to the basename (exact > prefix > contains > path-elsewhere).
        let name_lower = name.to_lowercase();
        let rel_lower = rel.to_lowercase();
        let score: u8 = if name_lower == query_lower {
            0
        } else if name_lower.starts_with(query_lower) {
            1
        } else if name_lower.contains(query_lower) {
            2
        } else if rel_lower.contains(query_lower) {
            3
        } else {
            // No match — descend if it's a dir, otherwise skip this entry.
            if is_dir {
                walk_root(&entry.path(), root, query_lower, out);
            }
            continue;
        };

        out.push((
            DirEntry {
                name,
                is_dir,
                path: rel.clone(),
            },
            score,
            rel.len(),
        ));

        // Keep walking into matched directories too, so a query like "src"
        // surfaces nested files under it. The ranking keeps shallow/exact
        // matches on top regardless of how many deeper matches pile up.
        if is_dir {
            walk_root(&entry.path(), root, query_lower, out);
        }
    }
}

#[tauri::command]
pub async fn search_project_files(
    path: String,
    query: String,
    max_results: Option<usize>,
) -> Result<Vec<DirEntry>, AppError> {
    let query_lower = query.trim().to_lowercase();
    debug!(
        "search_project_files called: path={path:?}, query={query:?} ({} chars)",
        query_lower.len()
    );

    // Empty query → nothing to search. The frontend should use
    // `list_dir_entries` for the no-filter (root listing) case, but guard here
    // too so an accidental empty call doesn't walk the whole tree.
    if query_lower.is_empty() {
        return Ok(Vec::new());
    }

    let root = PathBuf::from(&path)
        .canonicalize()
        .map_err(|e| AppError::InvalidPath(format!("Invalid project path: {e}")))?;

    let cap = max_results.unwrap_or(50);
    let mut hits: Vec<(DirEntry, u8, usize)> = Vec::new();
    walk_root(&root, &root, &query_lower, &mut hits);

    // Sort by (score asc, path-length asc) → best, shallowest matches first.
    hits.sort_by(|a, b| a.1.cmp(&b.1).then(a.2.cmp(&b.2)));
    let results = hits.into_iter().take(cap).map(|(e, _, _)| e).collect();
    Ok(results)
}

/// Parse the `name`, `description`, and `argument-hint` fields out of a
/// SKILL.md's YAML frontmatter.
///
/// The frontmatter is the block between an opening `---` and the next `---` at
/// the very start of the file. We only need a few simple `key: value` lines, so
/// a line-based scan avoids pulling in a YAML crate — the loopdeck SKILL.md
/// templates use only flat scalar values here. Returns `(name, description,
/// argument_hint)`, defaulting to empty strings when a field is absent or
/// there's no frontmatter block at all.
fn parse_skill_frontmatter(content: &str) -> (String, String, String) {
    let mut name = String::new();
    let mut description = String::new();
    let mut argument_hint = String::new();

    // Collect the frontmatter block: the lines strictly between the first `---`
    // and the following `---`. If the file doesn't start with `---`, there's no
    // frontmatter — bail with empties (the body alone is still a usable skill,
    // just without a discoverable name).
    let mut lines = content.lines();
    let first = lines.next();
    if first.map(str::trim) != Some("---") {
        return (name, description, argument_hint);
    }
    let mut block: Vec<&str> = Vec::new();
    for line in lines {
        if line.trim() == "---" {
            break;
        }
        block.push(line);
    }

    // Walk the block pulling `name:`, `description:`, and `argument-hint:`
    // values. We take the rest of the line after the `key:` prefix and trim it
    // — YAML scalars don't need quoting here, and the templates never wrap them
    // in quotes. `argument-hint` carries a hyphen, so its prefix is matched
    // literally (the others are bare `word:`).
    for line in block {
        let trimmed = line.trim_start();
        if let Some(rest) = trimmed.strip_prefix("name:") {
            if name.is_empty() {
                name = rest.trim().to_string();
            }
        } else if let Some(rest) = trimmed.strip_prefix("description:") {
            if description.is_empty() {
                description = rest.trim().to_string();
            }
        } else if let Some(rest) = trimmed.strip_prefix("argument-hint:") {
            if argument_hint.is_empty() {
                argument_hint = rest.trim().to_string();
            }
        }
    }

    (name, description, argument_hint)
}

/// List the skills installed for a project, for the composer's `/`-skill
/// discovery menu.
///
/// Reads `<root>/.claude/skills/<dir>/SKILL.md` for each installed skill and
/// parses its YAML frontmatter for `name` (the invocation token the `claude`
/// CLI recognizes) and `description`. The skills land there via
/// `import_project` → `copy_skills` during project bootstrap; a project that
/// hasn't been bootstrapped yet simply has no `.claude/skills/` directory, which
/// is reported as an empty list (not an error) — the menu shows "no skills".
///
/// `path` is the canonical project root (the same key used by every other
/// project command). Results are sorted by `name` for stable arrow-key
/// navigation.
#[tauri::command]
pub async fn list_skills(path: String) -> Result<Vec<SkillEntry>, AppError> {
    debug!("list_skills called: path={path:?}");

    let root = PathBuf::from(&path)
        .canonicalize()
        .map_err(|e| AppError::InvalidPath(format!("Invalid project path: {e}")))?;

    let skills_dir = root.join(".claude").join("skills");

    // Not bootstrapped yet — no skills to show. Treat as empty rather than an
    // error so the menu degrades gracefully on a fresh project.
    let read_dir = match std::fs::read_dir(&skills_dir) {
        Ok(rd) => rd,
        Err(_) => return Ok(Vec::new()),
    };

    let mut entries: Vec<SkillEntry> = Vec::new();
    for entry in read_dir.flatten() {
        // Only directories are skills — a stray file directly under skills/ is
        // ignored. We don't filter dotfiles here: `.claude/skills/` is itself a
        // dotfile path, but the skill dirs inside (e.g. `loopdeck-rust-expert`)
        // aren't, and any user-added skill should appear regardless.
        let file_type = match entry.file_type() {
            Ok(ft) => ft,
            Err(_) => continue,
        };
        if !file_type.is_dir() {
            continue;
        }

        let directory = entry.file_name().to_string_lossy().into_owned();
        let skill_md = entry.path().join("SKILL.md");
        let content = match std::fs::read_to_string(&skill_md) {
            Ok(c) => c,
            Err(_) => continue, // dir without a SKILL.md — skip silently
        };

        let (name, description, argument_hint) = parse_skill_frontmatter(&content);
        // A skill with no parseable `name` can't be invoked, so don't surface
        // it — the frontend inserts `/<name>` verbatim and an empty name would
        // produce a malformed token.
        if name.is_empty() {
            continue;
        }

        entries.push(SkillEntry {
            name,
            directory,
            description,
            argument_hint,
        });
    }

    entries.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(entries)
}

/// Map a `spawn_blocking` join failure (the task panicked or was cancelled) to
/// an `AppError`.
///
/// The blocking tasks we offload (recursive `walkdir`, per-repo `git`
/// subprocesses, file reads) don't panic on their own, but the join error
/// surface must still be converted to `AppError` so it can cross the Tauri IPC
/// boundary instead of leaking a raw `tokio::task::JoinError`.
fn blocking_task_failed(e: tokio::task::JoinError) -> AppError {
    AppError::BlockingTask(format!("background task failed: {e}"))
}

/// Scan a directory for project repositories.
///
/// Recursively walks the directory tree looking for marker files
/// (`.git`, `Cargo.toml`, `package.json`, etc.). Returns discovered
/// repos with metadata — does NOT modify any files.
///
/// Cross-references with the global config: `has_loopdeck` is only
/// true if the project is actually registered, not just if a
/// `.loopdeck/` directory exists on disk.
#[tauri::command]
pub async fn scan_directory(
    path: String,
    state: State<'_, AppState>,
) -> Result<Vec<DiscoveredRepo>, AppError> {
    debug!("scan_directory called with path: {path}");

    let scan_root = PathBuf::from(&path);
    let max_depth = {
        let config = state.config.lock().map_err(|_| AppError::LockError)?;
        config.settings.scan_depth
    };

    // The scanner does a recursive `walkdir` AND spawns a `git` subprocess per
    // discovered repo (freshness) — seconds of blocking I/O that must NOT run on
    // a tokio worker thread, where it would stall every other async command for
    // the duration. `spawn_blocking` moves it onto the dedicated blocking pool
    // and frees the worker while it runs.
    let mut repos =
        tokio::task::spawn_blocking(move || scanner::scan_directory(&scan_root, max_depth))
            .await
            .map_err(blocking_task_failed)??;

    // Cross-reference with global config: override `has_loopdeck` so it
    // reflects actual registration status, not just filesystem state.
    // This prevents repos that were removed from the registry but still
    // have a .loopdeck/ directory from appearing as "Imported".
    {
        let config = state.config.lock().map_err(|_| AppError::LockError)?;
        for repo in &mut repos {
            repo.has_loopdeck = config.find_by_path(&repo.path).is_some();
        }
    }

    info!("scan_directory found {} repos", repos.len());
    Ok(repos)
}

/// Import a repository: bootstrap `.loopdeck/project.yaml` and register in global config.
///
/// If the repo is already registered, returns the existing entry.
/// If `.loopdeck/project.yaml` already exists, loads it instead of overwriting.
#[tauri::command]
pub async fn import_project(
    path: String,
    state: State<'_, AppState>,
) -> Result<ProjectEntry, AppError> {
    debug!("import_project called with path: {path}");

    let repo_path = PathBuf::from(&path);

    // Canonicalize early so config lookups use the same path form
    let canonical = repo_path
        .canonicalize()
        .map_err(|e| AppError::Scan(format!("Failed to resolve path: {e}")))?;

    // Check if already registered (use canonical path for lookup). Done under a
    // brief lock so we don't hold the config mutex across the heavy bootstrapping
    // below — and so the early "already imported" return short-circuits before
    // any filesystem work.
    {
        let config = state.config.lock().map_err(|_| AppError::LockError)?;
        if let Some(existing) = config.find_by_path(&canonical) {
            return Ok(existing.clone());
        }
    }

    // Quick-scan for markers/README, bootstrap `.loopdeck/project.yaml`, gather
    // git info, and read the current loop. All blocking I/O — `quick_scan` +
    // `bootstrap_project` touch the filesystem and `check_git_info` spawns git
    // subprocesses — so it runs on the blocking pool, off the tokio worker.
    // `canonical` is cloned into the task; the outer value is retained to build
    // the registry entry after it completes.
    let (project_meta, git_info, current_loop) = tokio::task::spawn_blocking({
        let canonical = canonical.clone();
        move || -> Result<(project::ProjectMeta, git::GitInfo, Option<String>), AppError> {
            let (name, markers, has_readme) = scanner::quick_scan_directory(&canonical);
            let project_meta = project::bootstrap_project(&canonical, &name, &markers, has_readme)?;
            let git_info = git::check_git_info(&canonical);
            let current_loop = project::read_current_loop(canonical.as_path());
            Ok((project_meta, git_info, current_loop))
        }
    })
    .await
    .map_err(blocking_task_failed)??;

    // Build project entry and add to config
    let entry = ProjectEntry {
        path: canonical,
        name: project_meta.name,
        description: project_meta.description,
        status: ProjectStatus::Active,
        current_loop,
        last_opened: Some(Utc::now()),
        created_at: Utc::now(),
        last_commit_date: git_info.last_commit_date,
        last_commit_message: git_info.last_commit_message,
        last_modified: git_info.last_modified,
        uncommitted: git_info.uncommitted.into(),
        run_state: RunState::Idle,
    };

    {
        let mut config = state.config.lock().map_err(|_| AppError::LockError)?;
        // Idempotent under a race: if a concurrent import registered this path
        // between our early check above and now, return its entry instead of
        // letting `add_project` error with `ProjectAlreadyExists`. Honors the
        // documented "already registered → return existing entry" contract.
        if let Some(existing) = config.find_by_path(&entry.path) {
            return Ok(existing.clone());
        }
        config.add_project(entry.clone())?;
        config.save()?;
    }

    info!("import_project complete: {entry:?}");
    Ok(entry)
}

/// List all registered projects.
#[tauri::command]
pub async fn list_projects(state: State<'_, AppState>) -> Result<Vec<ProjectEntry>, AppError> {
    debug!("list_projects called");

    // Snapshot the project paths under a brief lock. We deliberately do NOT hold
    // the config mutex across the per-project git probing below — that probing
    // spawns subprocesses and walks each tree, which would block every other
    // command (and the whole tokio worker) for as long as it takes across every
    // registered project.
    let paths: Vec<PathBuf> = {
        let config = state.config.lock().map_err(|_| AppError::LockError)?;
        config.projects.iter().map(|e| e.path.clone()).collect()
    };

    // Refresh git info + current loop per project on the blocking pool. Results
    // are keyed by path so the apply pass stays aligned even if the registry
    // changed between snapshot and apply — a project added/removed by a
    // concurrent command simply won't match and is left untouched. Projects
    // whose path no longer exists are skipped (mirrors the prior inline guard).
    let refreshed: HashMap<PathBuf, (git::GitInfo, Option<String>)> =
        tokio::task::spawn_blocking(move || {
            let mut map = HashMap::with_capacity(paths.len());
            for path in paths {
                if !path.exists() {
                    continue;
                }
                let git_info = git::check_git_info(&path);
                let current_loop = project::read_current_loop(&path);
                map.insert(path, (git_info, current_loop));
            }
            map
        })
        .await
        .map_err(blocking_task_failed)?;

    // Apply the fresh data under a brief lock, persisting only if something
    // moved. The lock is held just for the mutation + save — not the git work.
    let mut out = {
        let mut config = state.config.lock().map_err(|_| AppError::LockError)?;
        let mut changed = false;

        for entry in &mut config.projects {
            let Some((git_info, current_loop)) = refreshed.get(&entry.path) else {
                continue;
            };

            if entry.last_commit_date != git_info.last_commit_date {
                entry.last_commit_date = git_info.last_commit_date.clone();
                changed = true;
            }
            if entry.last_modified != git_info.last_modified {
                entry.last_modified = git_info.last_modified.clone();
                changed = true;
            }
            if entry.last_commit_message != git_info.last_commit_message {
                entry.last_commit_message = git_info.last_commit_message.clone();
                changed = true;
            }
            let fresh_uncommitted: config::UncommittedStats = git_info.uncommitted.into();
            if entry.uncommitted != fresh_uncommitted {
                entry.uncommitted = fresh_uncommitted;
                changed = true;
            }

            // Always re-read the current loop text. It isn't part of the
            // change/save decision — this matches the prior inline behaviour,
            // which set it unconditionally per project.
            entry.current_loop = current_loop.clone();

            // Recompute status from the (possibly refreshed) git dates so the
            // Dashboard reflects current freshness without a manual rescan.
            let before = entry.status;
            config::update_project_status(entry);
            if entry.status != before {
                changed = true;
            }
        }

        if changed {
            config.save()?;
        }

        config.projects.clone()
    };

    // Derive ephemeral run_state per project from live session + pending slots.
    // Done after the save (and outside the config lock — it only touches the
    // session/pending-slot maps) so transient state never reaches disk.
    derive_run_states(&state, &mut out);

    Ok(out)
}

/// Get the global agent configuration.
///
/// Returns `None` if no agent config has been saved yet. The auth token is
/// **never** returned to the renderer: `auth_token` is always `None` here, and
/// `has_auth_token` reports whether one is stored in the OS keychain so the UI
/// can show a masked "token stored" affordance without the secret crossing IPC.
#[tauri::command]
pub async fn get_agent_config(state: State<'_, AppState>) -> Result<Option<AgentConfig>, AppError> {
    let config = state.config.lock().map_err(|_| AppError::LockError)?;
    let Some(mut agent) = config.agent.clone() else {
        return Ok(None);
    };
    agent.auth_token = None;
    agent.has_auth_token = secrets::load_auth_token()?.is_some();
    Ok(Some(agent))
}

/// Set (create or update) the global agent configuration.
///
/// If a non-empty `auth_token` is supplied it is stored in the OS keychain
/// (overwriting any existing value); the token is never written to
/// `config.yaml`. An empty/`None` token means "leave the stored keychain token
/// untouched" — because `get_agent_config` never returns the plaintext token,
/// an unchanged Settings field shows up empty and must not be interpreted as a
/// request to clear. Use [`clear_auth_token`] to remove a stored token.
#[tauri::command]
pub async fn set_agent_config(
    agent_config: AgentConfig,
    state: State<'_, AppState>,
) -> Result<AgentConfig, AppError> {
    // Store a newly-typed token in the keychain; otherwise keep whatever is
    // already there.
    let has_token = if let Some(token) = agent_config.auth_token.as_ref().filter(|t| !t.is_empty())
    {
        secrets::store_auth_token(token)?;
        true
    } else {
        secrets::load_auth_token()?.is_some()
    };

    // Persist only the non-secret fields. The token and the presence flag are
    // scrubbed so they never reach config.yaml.
    let mut persisted = agent_config.clone();
    persisted.auth_token = None;
    persisted.has_auth_token = false;

    {
        let mut config = state.config.lock().map_err(|_| AppError::LockError)?;
        config.agent = Some(persisted.clone());
        config.save()?;
    }

    // Reflect actual keychain presence back to the caller (used by the UI to
    // flip the field into its "stored" state).
    persisted.has_auth_token = has_token;
    Ok(persisted)
}

/// Remove the stored auth token from the OS keychain.
///
/// Idempotent: succeeding when no token is stored. The non-secret agent config
/// (base_url / model / effort) in `config.yaml` is left untouched.
#[tauri::command]
pub async fn clear_auth_token() -> Result<(), AppError> {
    secrets::delete_auth_token()
}

/// Get a single project by path.
#[tauri::command]
pub async fn get_project(
    path: String,
    state: State<'_, AppState>,
) -> Result<ProjectEntry, AppError> {
    debug!("get_project called with path: {path}");
    let repo_path = PathBuf::from(&path);
    let config = state.config.lock().map_err(|_| AppError::LockError)?;
    config
        .find_by_path(&repo_path)
        .cloned()
        .ok_or(AppError::ProjectNotFound(path))
}

/// Update the project description (both in `.loopdeck/project.yaml` and config registry).
#[tauri::command]
pub async fn update_description(
    path: String,
    description: String,
    state: State<'_, AppState>,
) -> Result<ProjectMeta, AppError> {
    debug!("update_description called for path: {path}");

    let repo_path = PathBuf::from(&path);

    // Update the project.yaml file
    let meta = project::update_description(&repo_path, &description)?;

    // Update in config registry
    {
        let mut config = state.config.lock().map_err(|_| AppError::LockError)?;
        if let Some(entry) = config.find_by_path_mut(&repo_path) {
            entry.description = description;
            config.save()?;
        }
    }

    info!("update_description complete for: {path}");
    Ok(meta)
}

/// Remove a project from the registry.
/// Does NOT delete the `.loopdeck/` directory or any project files.
#[tauri::command]
pub async fn remove_project(path: String, state: State<'_, AppState>) -> Result<(), AppError> {
    debug!("remove_project called for path: {path}");

    let repo_path = PathBuf::from(&path);

    // Canonicalize so we match the stored path (which is always canonical)
    let canonical = repo_path
        .canonicalize()
        .map_err(|e| AppError::Scan(format!("Failed to resolve path: {e}")))?;

    let mut config = state.config.lock().map_err(|_| AppError::LockError)?;

    if !config.remove_project(&canonical) {
        return Err(AppError::ProjectNotFound(path));
    }

    config.save()?;
    info!("remove_project complete for: {path}");
    Ok(())
}

/// Resolve and validate a user-supplied path before handing it to an OS
/// opener (Finder/Terminal/explorer). The path originates from a Tauri IPC
/// argument, so although it is normally a legitimate repo path we treat it as
/// untrusted: this canonicalizes it and rejects anything that doesn't resolve
/// to an existing directory. That blocks scheme-handler tricks (e.g. macOS
/// `open "x-apple-..."`) and shell-metachar injection downstream.
fn resolve_dir_arg(path: &str) -> Result<PathBuf, AppError> {
    let resolved = std::fs::canonicalize(path)
        .map_err(|_| AppError::Scan(format!("Path does not exist or is not accessible: {path}")))?;
    if !resolved.is_dir() {
        return Err(AppError::Scan(format!("Not a directory: {path}")));
    }
    Ok(resolved)
}

/// Open the repository path in the system file manager (Finder on macOS).
#[tauri::command]
pub async fn open_in_finder(path: String) -> Result<(), AppError> {
    debug!("open_in_finder called for path: {path}");
    // Validate before any opener sees it. `open`, `xdg-open`, and `explorer`
    // otherwise interpret some strings as URLs / handlers.
    let resolved = resolve_dir_arg(&path)?;

    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open")
            .arg(&resolved)
            .spawn()
            .map_err(|e| AppError::Scan(format!("Failed to open Finder: {e}")))?;
    }

    #[cfg(target_os = "linux")]
    {
        std::process::Command::new("xdg-open")
            .arg(&resolved)
            .spawn()
            .map_err(|e| AppError::Scan(format!("Failed to open file manager: {e}")))?;
    }

    #[cfg(target_os = "windows")]
    {
        std::process::Command::new("explorer")
            .arg(&resolved)
            .spawn()
            .map_err(|e| AppError::Scan(format!("Failed to open Explorer: {e}")))?;
    }

    info!("open_in_finder complete for: {path}");
    Ok(())
}

/// Open the repository path in the system terminal.
#[tauri::command]
pub async fn open_in_terminal(path: String) -> Result<(), AppError> {
    debug!("open_in_terminal called for path: {path}");
    let resolved = resolve_dir_arg(&path)?;
    let path_str = resolved.to_string_lossy().to_string();

    #[cfg(target_os = "macos")]
    {
        use std::process::Stdio;

        // Try popular terminal apps in order of preference. The path is NEVER
        // interpolated into a shell/AppleScript string body: where a terminal
        // needs a working directory it is either passed as a dedicated argv
        // (Ghostty `--working-directory`, OSAScript `on run argv`) or set as
        // the spawned process's `current_dir`.
        fn ghostty(p: &str) -> std::process::Command {
            let mut cmd = std::process::Command::new("ghostty");
            cmd.arg(format!("--working-directory={p}"));
            cmd.stdout(Stdio::null());
            cmd.stderr(Stdio::null());
            cmd
        }
        // OSAScript: read the path from argv and `cd` to its quoted form. The
        // path content is never embedded in the script source.
        fn iterm(p: &str) -> std::process::Command {
            let script = "on run argv\n tell application \"iTerm\" to create window with default profile command (\"cd \" & quoted form of (item 1 of argv))\nend run";
            let mut cmd = std::process::Command::new("osascript");
            cmd.args(["-e", script, p]);
            cmd
        }
        fn terminal_app(p: &str) -> std::process::Command {
            let script = "on run argv\n tell application \"Terminal\" to do script (\"cd \" & quoted form of (item 1 of argv))\nend run";
            let mut cmd = std::process::Command::new("osascript");
            cmd.args(["-e", script, p]);
            cmd
        }

        type Builder = fn(&str) -> std::process::Command;
        let builders: &[(&str, Builder)] = &[
            ("Ghostty", ghostty),
            ("iTerm", iterm),
            ("Terminal", terminal_app),
        ];

        let mut spawned = false;
        for (_name, builder) in builders {
            if builder(&path_str).spawn().is_ok() {
                spawned = true;
                break;
            }
        }

        if !spawned {
            // Last resort: just open the directory in Finder.
            std::process::Command::new("open")
                .arg(&resolved)
                .spawn()
                .map_err(|e| AppError::Scan(format!("Failed to open directory: {e}")))?;
        }
    }

    #[cfg(target_os = "linux")]
    {
        use std::os::unix::process::CommandExt; // enables Command::process_group()

        // Try common terminal emulators. Path is passed as a distinct argv,
        // never concatenated into a shell string.
        let terminals = [
            "gnome-terminal",
            "konsole",
            "xfce4-terminal",
            "x-terminal-emulator",
            "alacritty",
            "kitty",
        ];

        let mut spawned = false;
        for term in &terminals {
            let mut cmd = std::process::Command::new(term);
            cmd.arg("--working-directory").arg(&path_str);
            // Detach the terminal into its own process group so it keeps running
            // independently of LoopDeck (won't receive SIGHUP when we exit).
            cmd.process_group(0);
            if cmd.spawn().is_ok() {
                spawned = true;
                break;
            }
        }

        if !spawned {
            return Err(AppError::Scan(
                "Could not find a terminal emulator. Please install gnome-terminal, konsole, or alacritty.".into(),
            ));
        }
    }

    #[cfg(target_os = "windows")]
    {
        // `current_dir` sets the working directory without injecting the path
        // into a `cd /d <path>` shell string (the previous form ran arbitrary
        // commands if `path` contained `&` or other metacharacters).
        std::process::Command::new("cmd")
            .arg("/K")
            .current_dir(&resolved)
            .spawn()
            .map_err(|e| AppError::Scan(format!("Failed to open Command Prompt: {e}")))?;
    }

    info!("open_in_terminal complete for: {path}");
    Ok(())
}

/// Rescan a single project to refresh git info (last commit, last modified).
/// Updates the project entry in the global config and returns it.
#[tauri::command]
pub async fn rescan_project(
    path: String,
    state: State<'_, AppState>,
) -> Result<ProjectEntry, AppError> {
    debug!("rescan_project called for path: {path}");

    let repo_path = PathBuf::from(&path);

    // Resolve the target under a brief lock: confirm it's registered and its
    // path still exists. We capture the on-disk path to probe and release the
    // lock before the git subprocess runs — same rationale as `list_projects`.
    let target = {
        let config = state.config.lock().map_err(|_| AppError::LockError)?;
        let entry = config
            .find_by_path(&repo_path)
            .ok_or(AppError::ProjectNotFound(path.clone()))?;
        if !entry.path.exists() {
            return Err(AppError::ProjectNotFound(format!(
                "Project path no longer exists: {}",
                entry.path.display()
            )));
        }
        entry.path.clone()
    };

    // Refresh git info on the blocking pool — spawns git subprocesses and walks
    // the tree for last-modified, so it must not run on the tokio worker.
    let git_info = tokio::task::spawn_blocking(move || git::check_git_info(&target))
        .await
        .map_err(blocking_task_failed)?;

    // Apply + persist under a brief lock. Note: `last_commit_message` is
    // intentionally not refreshed here — preserved from the prior behaviour.
    let mut result = {
        let mut config = state.config.lock().map_err(|_| AppError::LockError)?;
        let entry = config
            .find_by_path_mut(&repo_path)
            .ok_or(AppError::ProjectNotFound(path.clone()))?;
        entry.last_commit_date = git_info.last_commit_date.clone();
        entry.last_modified = git_info.last_modified.clone();
        entry.uncommitted = git_info.uncommitted.into();

        config::update_project_status(entry);

        let result = entry.clone();
        config.save()?;
        result
    };

    // Stamp the ephemeral run_state before returning so callers see the truth.
    let mut single = vec![result];
    derive_run_states(&state, &mut single);
    result = single.into_iter().next().unwrap();

    info!("rescan_project complete for: {path}");
    Ok(result)
}

/// Regenerate the project description by re-scanning README and structure.
/// Updates both `.loopdeck/project.yaml` and the config registry.
#[tauri::command]
pub async fn regenerate_description(
    path: String,
    state: State<'_, AppState>,
) -> Result<String, AppError> {
    debug!("regenerate_description called for path: {path}");

    let repo_path = PathBuf::from(&path);
    let repo_path_clone = repo_path.clone();

    // Re-scan markers and README status for this repo
    let (name, markers, has_readme) = scanner::quick_scan_directory(&repo_path_clone);
    let desc = project::regenerate_description(&repo_path_clone, &name, &markers, has_readme)?;

    // Update in config registry
    {
        let mut config = state.config.lock().map_err(|_| AppError::LockError)?;
        if let Some(entry) = config.find_by_path_mut(&repo_path) {
            entry.description = desc.clone();
            config.save()?;
        }
    }

    info!("regenerate_description complete for: {path}");
    Ok(desc)
}

/// Get all decisions from `.loopdeck/decisions.md`.
/// Returns an empty list if the file does not exist.
#[tauri::command]
pub async fn get_decisions(
    path: String,
    _state: State<'_, AppState>,
) -> Result<Vec<Decision>, AppError> {
    debug!("get_decisions called for path: {path}");

    let repo_path = PathBuf::from(&path);
    if !repo_path.exists() {
        return Err(AppError::ProjectNotFound(format!(
            "Path does not exist: {path}"
        )));
    }

    let decisions = memory::parse_decisions(&repo_path);
    Ok(decisions)
}

/// Get loop status from `.loopdeck/loops.md`.
/// Returns an empty/default LoopStatus if the file does not exist.
#[tauri::command]
pub async fn get_loops(path: String, _state: State<'_, AppState>) -> Result<LoopStatus, AppError> {
    debug!("get_loops called for path: {path}");

    let repo_path = PathBuf::from(&path);
    if !repo_path.exists() {
        return Err(AppError::ProjectNotFound(format!(
            "Path does not exist: {path}"
        )));
    }

    let status = memory::parse_loops(&repo_path);
    Ok(status)
}

/// Get all epics from `docs/epics/`, each with its PRDs and phase checklists.
/// Returns an empty list if `docs/epics/` does not exist.
#[tauri::command]
pub async fn get_epics(path: String, _state: State<'_, AppState>) -> Result<Vec<Epic>, AppError> {
    debug!("get_epics called for path: {path}");

    let repo_path = PathBuf::from(&path);
    if !repo_path.exists() {
        return Err(AppError::ProjectNotFound(format!(
            "Path does not exist: {path}"
        )));
    }

    Ok(epic::parse_epics(&repo_path))
}

/// Get epics grouped by milestone (ordered), for the cross-project `/epics` view.
/// Epics with no milestone land in an "Unmilestoned" bucket.
#[tauri::command]
pub async fn get_epics_by_milestone(
    path: String,
    _state: State<'_, AppState>,
) -> Result<std::collections::BTreeMap<String, Vec<Epic>>, AppError> {
    debug!("get_epics_by_milestone called for path: {path}");

    let repo_path = PathBuf::from(&path);
    if !repo_path.exists() {
        return Err(AppError::ProjectNotFound(format!(
            "Path does not exist: {path}"
        )));
    }

    Ok(epic::epics_by_milestone(&repo_path))
}

/// Promote a PRD checklist item into `.loopdeck/loops.md ## Current`.
///
/// Writes the loop with `**Epic**` / `**PRD**` back-reference bullets. Refuses
/// to clobber a non-empty `## Current` — the caller must complete or abandon
/// the current loop first.
#[tauri::command]
pub async fn promote_epic_loop(
    path: String,
    epic_slug: String,
    prd_filename: String,
    loop_title: String,
    _state: State<'_, AppState>,
) -> Result<(), AppError> {
    debug!("promote_epic_loop called for path: {path}, epic: {epic_slug}, prd: {prd_filename}");

    let repo_path = PathBuf::from(&path);
    if !repo_path.exists() {
        return Err(AppError::ProjectNotFound(format!(
            "Path does not exist: {path}"
        )));
    }

    epic::promote_epic_loop(&repo_path, &epic_slug, &prd_filename, &loop_title)?;
    Ok(())
}

/// Toggle a `- [ ]` / `- [x]` next-step checklist item in `.loopdeck/loops.md`.
/// Returns the new checked state.
#[tauri::command]
pub async fn toggle_loop_step(
    path: String,
    step_text: String,
    _state: State<'_, AppState>,
) -> Result<bool, AppError> {
    debug!("toggle_loop_step called for path: {path}");

    let repo_path = PathBuf::from(&path);
    if !repo_path.exists() {
        return Err(AppError::ProjectNotFound(format!(
            "Path does not exist: {path}"
        )));
    }

    let now_checked = memory::toggle_loop_step(&repo_path, &step_text)?;
    Ok(now_checked)
}

/// Toggle a `- [ ]` / `- [x]` checklist item in a PRD file under docs/epics/.
/// Returns the new checked state.
#[tauri::command]
pub async fn toggle_prd_loop(
    path: String,
    epic_slug: String,
    prd_filename: String,
    loop_title: String,
    _state: State<'_, AppState>,
) -> Result<bool, AppError> {
    debug!("toggle_prd_loop called for path: {path}, epic: {epic_slug}, prd: {prd_filename}");

    let repo_path = PathBuf::from(&path);
    if !repo_path.exists() {
        return Err(AppError::ProjectNotFound(format!(
            "Path does not exist: {path}"
        )));
    }

    let now_checked = epic::toggle_prd_loop(&repo_path, &epic_slug, &prd_filename, &loop_title)?;
    Ok(now_checked)
}

/// Read a spec file (epic README or PRD) under `docs/epics/`.
/// `rel_path` is relative to `docs/epics/` (e.g. `<slug>/prd-x.md`).
#[tauri::command]
pub async fn read_spec_file(
    path: String,
    rel_path: String,
    _state: State<'_, AppState>,
) -> Result<String, AppError> {
    debug!("read_spec_file called for path: {path}, rel: {rel_path}");

    let repo_path = PathBuf::from(&path);
    if !repo_path.exists() {
        return Err(AppError::ProjectNotFound(format!(
            "Path does not exist: {path}"
        )));
    }

    epic::read_spec_file(&repo_path, &rel_path)
}

/// Write (create or overwrite) a spec file under `docs/epics/`.
/// `rel_path` is relative to `docs/epics/`. Raw write — does not validate
/// frontmatter; a broken file will be logged and skipped by parse_epics.
#[tauri::command]
pub async fn write_spec_file(
    path: String,
    rel_path: String,
    content: String,
    _state: State<'_, AppState>,
) -> Result<(), AppError> {
    debug!("write_spec_file called for path: {path}, rel: {rel_path}");

    let repo_path = PathBuf::from(&path);
    if !repo_path.exists() {
        return Err(AppError::ProjectNotFound(format!(
            "Path does not exist: {path}"
        )));
    }

    epic::write_spec_file(&repo_path, &rel_path, &content)
}

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
    let repo_path = PathBuf::from(&path);
    if !repo_path.exists() {
        return Err(AppError::ProjectNotFound(format!(
            "Path does not exist: {path}"
        )));
    }

    let prompt = build_next_loop_prompt(&repo_path);
    let response = start_fresh_and_record(&state, &repo_path, &prompt).await?;
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
    state: State<'_, AppState>,
) -> Result<AgentResponse, AppError> {
    debug!("agent_send_message called for path: {path}");
    let repo_path = PathBuf::from(&path);
    if !repo_path.exists() {
        return Err(AppError::ProjectNotFound(format!(
            "Path does not exist: {path}"
        )));
    }

    let response = send_and_record(&state, &repo_path, &prompt).await?;
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
    let repo_path = PathBuf::from(&path);
    if !repo_path.exists() {
        return Err(AppError::ProjectNotFound(format!(
            "Path does not exist: {path}"
        )));
    }

    let prompt = build_next_loop_prompt(&repo_path);
    start_fresh_and_record_streaming(&state, &repo_path, &prompt, &on_event).await?;
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
#[tauri::command]
pub async fn agent_send_message_streaming(
    path: String,
    prompt: String,
    on_event: Channel<ClaudeEvent>,
    state: State<'_, AppState>,
) -> Result<(), AppError> {
    debug!("agent_send_message_streaming called for path: {path}");
    let repo_path = PathBuf::from(&path);
    if !repo_path.exists() {
        return Err(AppError::ProjectNotFound(format!(
            "Path does not exist: {path}"
        )));
    }

    send_and_record_streaming(&state, &repo_path, &prompt, &on_event).await?;
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
    _state: State<'_, AppState>,
) -> Result<Vec<ConversationTurn>, AppError> {
    debug!("agent_get_conversation called for path: {path}");
    let repo_path = PathBuf::from(&path);
    if !repo_path.exists() {
        return Err(AppError::ProjectNotFound(format!(
            "Path does not exist: {path}"
        )));
    }
    Ok(conversation::load_conversation(&repo_path))
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
    _state: State<'_, AppState>,
) -> Result<Vec<ConversationSummary>, AppError> {
    debug!("agent_list_conversations called for path: {path}");
    let repo_path = PathBuf::from(&path);
    if !repo_path.exists() {
        return Err(AppError::ProjectNotFound(format!(
            "Path does not exist: {path}"
        )));
    }
    Ok(conversation::list_conversations(&repo_path))
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
    _state: State<'_, AppState>,
) -> Result<Vec<ConversationTurn>, AppError> {
    debug!("agent_get_conversation_by_id called for path: {path}, id: {id}");
    let repo_path = PathBuf::from(&path);
    if !repo_path.exists() {
        return Err(AppError::ProjectNotFound(format!(
            "Path does not exist: {path}"
        )));
    }
    Ok(conversation::load_conversation_by_id(&repo_path, &id))
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
    let repo_path = PathBuf::from(&path);
    if !repo_path.exists() {
        return Err(AppError::ProjectNotFound(format!(
            "Path does not exist: {path}"
        )));
    }

    // Extract the resume id BEFORE promoting (after promotion the turns live
    // in active.jsonl, but reading from the source id is unambiguous).
    let resume_id = conversation::session_id_for_conversation(&repo_path, &id);

    // Promote: archive current active, seed new active from the source. No-op
    // for `id == "active"` or an unknown/empty source — both safe here.
    conversation::promote_to_active(&repo_path, &id)?;

    // Drop any live session — its in-process context is now stale relative to
    // the promoted transcript. The next send re-spawns with `--resume <id>`
    // via `with_session` (which reads `last_session_id` off the new active).
    let removed = state
        .claude_sessions
        .lock()
        .map_err(|_| AppError::LockError)?
        .remove(&repo_path)
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
    let repo_path = PathBuf::from(&path);

    // Remove the live session from the map. The Arc's last reference drops
    // here → `ClaudeSession::Drop` closes stdin and reaps the child.
    let removed = state
        .claude_sessions
        .lock()
        .map_err(|_| AppError::LockError)?
        .remove(&repo_path)
        .is_some();
    if removed {
        debug!("dropped live claude session for: {path}");
    }

    // Archive the transcript regardless of whether a live session existed —
    // a reset should always mean "next Start is fresh".
    conversation::archive_conversation(&repo_path)?;

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

/// Persist a permission allow-rule into the project's `.claude/settings.local.json`.
///
/// "Always allow" affordance for the manual-approval card: alongside the
/// per-call Allow/Deny verdict, the user can ask LoopDeck to remember the
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
pub async fn agent_add_allow_rule(path: String, rule: String) -> Result<(), AppError> {
    debug!("agent_add_allow_rule called for path: {path}, rule: {rule}");
    let repo_path = PathBuf::from(&path);
    let claude_dir = repo_path.join(".claude");
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

    // Pop + fire the sender. There's at most one per project (cleared at turn
    // end), so this is a single take. `send` failing means the receiver was
    // already dropped (turn ended between the UI click and here) — treat as a
    // no-op success so the button feels responsive either way.
    let fired = {
        let guard = state
            .interrupt_slots
            .lock()
            .map_err(|_| AppError::LockError)?;
        guard
            .get(&repo_path)
            .and_then(|slot| slot.lock().ok().and_then(|mut g| g.take()))
            .map(|sender| sender.send(()).is_ok())
    };

    if !fired.unwrap_or(false) {
        debug!("agent_interrupt: no in-flight turn for {path} (no-op)");
    } else {
        info!("agent_interrupt fired for: {path}");
    }
    Ok(())
}

// ── Agent session helpers ──────────────────────────────────────────────────
//
// Two pipelines own session lifecycle:
// - `with_session` (below) — get-or-spawn + `lock().await`. Used by
//   `agent_send_message`: reuses the live process, or `--resume`s after a
//   restart. Queues behind a running turn.
// - `spawn_fresh_locked` (further below) — force-spawn + `try_lock`. Used by
//   `agent_start_loop[_streaming]`: always a fresh conversation, rejects when
//   busy.
//
// Both uphold the per-project turn-lock invariant (one stdin, one process):
// same-project turns never run concurrently, different projects run in parallel.

/// Read the agent config from the registry and inject the auth token from the
/// OS keychain.
///
/// The token is never stored in `config.yaml` (it lives in the keychain — see
/// `secrets`), so it must be resolved here, at spawn time. The returned value
/// is a local owned `AgentConfig` passed by reference to `ClaudeSession::spawn`,
/// which sets it as a child env var (`ANTHROPIC_AUTH_TOKEN`) and then drops it —
/// the plaintext token is never held on the long-lived `Mutex<GlobalConfig>`.
///
/// A missing keychain token resolves to `None`, preserving the prior behaviour
/// where a user may rely on `ANTHROPIC_AUTH_TOKEN` inherited from their shell.
fn resolve_agent_config(state: &AppState) -> Result<AgentConfig, AppError> {
    let mut agent_config = state
        .config
        .lock()
        .map_err(|_| AppError::LockError)?
        .agent
        .clone()
        .ok_or_else(|| {
            AppError::Agent(
                "no agent config set; configure it in Settings before starting a loop".into(),
            )
        })?;
    agent_config.auth_token = secrets::load_auth_token()?;
    Ok(agent_config)
}

/// Get the live `ClaudeSession` for `path` as an owned `Arc`, spawning one if
/// none exists (or resuming a prior conversation via `--resume`).
///
/// The caller `.lock().await`s the Arc to take the per-project turn lock — so
/// turns within one project serialize, while different projects (different map
/// entries, different inner locks) run concurrently. Returning the Arc (not a
/// guard) avoids lifetime tangles: the Arc is owned and stays referenced in
/// the map, keeping the process alive between turns.
///
/// # Lock invariant
/// The outer `std::sync::Mutex` guard is dropped **before** the caller awaits
/// the inner lock. Holding the std Mutex across `.await` would deadlock (or, if
/// sent across threads, be unsound). The map guard is a plainly-scoped
/// block-local that ends inside this function, before it returns.
async fn with_session(
    state: &AppState,
    path: &Path,
) -> Result<Arc<tokio::sync::Mutex<ClaudeSession>>, AppError> {
    // ── Outer (map) lock: held only to read/insert the Arc. ──
    let mut map_guard = state
        .claude_sessions
        .lock()
        .map_err(|_| AppError::LockError)?;

    if let Some(arc) = map_guard.get(path) {
        // Existing live session — clone the Arc and reuse it. No .await here.
        return Ok(Arc::clone(arc));
    }

    // No live session — spawn one. Read agent config + resume id while we
    // still hold the map lock cheaply (still no .await in this scope).
    let agent_config = resolve_agent_config(state)?;

    let resume_id = conversation::last_session_id(path);
    let session = ClaudeSession::spawn(
        &path.to_path_buf(),
        &agent_config,
        resume_id.as_deref(),
        PermissionPolicy::confirm_changes(),
    )?;
    let arc = Arc::new(tokio::sync::Mutex::new(session));
    map_guard.insert(path.to_path_buf(), Arc::clone(&arc));
    // ── map_guard (std Mutex) dropped here as this scope ends. ──
    Ok(arc)
}

/// Build the prompt that kicks off the next development loop.
///
/// Scans `.loopdeck/loops.md` raw text for the first unchecked `- [ ]` under
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

/// `## Next Steps` (the structured `memory::parse_loops` flattens checked and
/// unchecked steps together, so we read the raw file here to preserve the
/// distinction). Falls back to a "propose the next loop" prompt when every
/// step is done or there is no `loops.md` yet.
fn build_next_loop_prompt(path: &Path) -> String {
    let next_step = next_unchecked_loop_step(path);

    match next_step {
        Some(step) => format!(
            "You are working on this LoopDeck project. Use the `loopdeck-orchestrator` \
             skill conventions. Read `.loopdeck/loops.md` for full context. The next \
             unchecked step is: \"{step}\". Implement it. When done, update \
             `.loopdeck/loops.md` (mark the step `[x]`, refresh `## Current`) and \
             append any architectural decisions to `.loopdeck/decisions.md` per the \
             memory convention."
        ),
        None => String::from(
            "You are working on this LoopDeck project. Use the `loopdeck-orchestrator` \
             skill conventions. Review `.loopdeck/loops.md`, then propose and start \
             the next loop. When done, update `.loopdeck/loops.md` (refresh \
             `## Current`, add new steps under `## Next Steps`) and append any \
             architectural decisions to `.loopdeck/decisions.md` per the memory \
             convention.",
        ),
    }
}

/// Scan `.loopdeck/loops.md` for the first unchecked `- [ ]` step under
/// `## Next Steps`. Returns `None` if the file is missing, the section is
/// absent, or every step is already checked.
fn next_unchecked_loop_step(path: &Path) -> Option<String> {
    let content = std::fs::read_to_string(path.join(".loopdeck").join("loops.md")).ok()?;

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
                return Some(rest.trim().to_string());
            }
        }
    }
    None
}

/// Get (or create) the per-project `AskUserQuestion` slot.
///
/// One slot per project path, shared (via `Arc`) between the read loop (which
/// stores the oneshot sender when Claude asks a question) and the
/// `agent_answer_question` command (which pops the sender to deliver answers).
/// The slot persists across turns so it doesn't need re-creating each time;
/// its contents are always `None` outside a pending question.
fn question_slot(state: &AppState, path: &Path) -> Result<QuestionSlot, AppError> {
    let mut guard = state
        .pending_answers
        .lock()
        .map_err(|_| AppError::LockError)?;
    Ok(guard
        .entry(path.to_path_buf())
        .or_insert_with(|| Arc::new(Mutex::new(None)))
        .clone())
}

/// Get (or create) the per-project manual-approval slot.
///
/// The permission counterpart of `question_slot`: one `PermissionSlot` per
/// project path, shared between the read loop (stores the oneshot sender when
/// a mutating tool needs approval) and the `agent_answer_permission` command
/// (pops the sender to deliver the verdict). Persistent across turns; always
/// `None` outside a pending approval.
fn permission_slot(state: &AppState, path: &Path) -> Result<PermissionSlot, AppError> {
    let mut guard = state
        .pending_permissions
        .lock()
        .map_err(|_| AppError::LockError)?;
    Ok(guard
        .entry(path.to_path_buf())
        .or_insert_with(|| Arc::new(Mutex::new(None)))
        .clone())
}

/// Get (or create) the per-project graceful-interrupt slot.
///
/// The streaming read loop installs a oneshot sender here at turn start and
/// clears it at turn end; `agent_interrupt` pops + fires the sender to end the
/// turn gracefully. Persistent across turns; always `None` outside a running
/// turn.
fn interrupt_slot(state: &AppState, path: &Path) -> Result<InterruptSlot, AppError> {
    let mut guard = state
        .interrupt_slots
        .lock()
        .map_err(|_| AppError::LockError)?;
    Ok(guard
        .entry(path.to_path_buf())
        .or_insert_with(|| Arc::new(Mutex::new(None)))
        .clone())
}

/// Derive the ephemeral `RunState` for each project from live `AppState`.
///
/// Strict live state (no recency heuristic):
/// - `Waiting` — there is a pending manual-approval (`pending_permissions`) or
///   a pending `AskUserQuestion` (`pending_answers`) for the project path. The
///   in-flight turn is parked awaiting the user.
/// - `Working` — a `claude_sessions` entry exists for the path AND its inner
///   `tokio::Mutex` is currently held (a turn is streaming). We check by
///   `try_lock`; success ⇒ idle, failure ⇒ busy.
/// - `Idle` — no live session, or session exists but no turn is in flight.
///
/// Note: `Done` is intentionally never set here. We don't track "last turn
/// finished at" timestamps globally; the frontend derives its own transient
/// "done" affordance from streaming-result events if desired.
fn derive_run_states(state: &AppState, projects: &mut [ProjectEntry]) {
    // Snapshot the pending-slot keys without holding the locks across the
    // (potentially async-blocking) inner session `try_lock`.
    let pending_paths: std::collections::HashSet<PathBuf> = {
        let perms = state
            .pending_permissions
            .lock()
            .map_err(|_| AppError::LockError);
        let answers = state
            .pending_answers
            .lock()
            .map_err(|_| AppError::LockError);
        let (Ok(perms), Ok(answers)) = (perms, answers) else {
            // If we can't observe pending state, leave everything as-is.
            return;
        };
        perms.keys().chain(answers.keys()).cloned().collect()
    };

    for entry in projects.iter_mut() {
        if pending_paths.contains(&entry.path) {
            entry.run_state = RunState::Waiting;
            continue;
        }
        entry.run_state = if project_busy(state, &entry.path) {
            RunState::Working
        } else {
            RunState::Idle
        };
    }
}

/**
 * Is a streaming agent turn currently in flight for `path`?
 *
 * True when a `claude_sessions` entry exists AND its inner `tokio::Mutex` is
 * held (a turn is streaming). Checked via non-blocking `try_lock`. Used both by
 * `derive_run_states` (for `list_projects`/`rescan_project`) and by the
 * `agent_is_busy` command (for frontend reconciliation after navigation).
 *
 * Outer-map guards are held only over the synchronous Arc lookup, never across
 * an `.await`.
 */
fn project_busy(state: &AppState, path: &Path) -> bool {
    let session_arc = state
        .claude_sessions
        .lock()
        .ok()
        .and_then(|m| m.get(path).cloned());
    match session_arc {
        Some(arc) => arc.try_lock().is_err(),
        None => false,
    }
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
    session: &mut ClaudeSession,
    prompt: &str,
    question_slot: &QuestionSlot,
    permission_slot: &PermissionSlot,
    interrupt_slot: &InterruptSlot,
) -> Result<AgentResponse, AppError> {
    // `attempt` is the 0-based index of the attempt that just ran. It doubles
    // as the retry count via `retry::backoff_ms(attempt)`.
    let mut attempt: u32 = 0;
    loop {
        let response = session
            .send_message(prompt, question_slot, permission_slot, interrupt_slot)
            .await?;

        // Done unless this is a retryable transient overload.
        if !(response.is_error && retry::is_overloaded(&response.result)) {
            return Ok(response);
        }

        // Overloaded — back off and retry if attempts remain.
        match retry::backoff_ms(attempt) {
            Some(delay) => {
                warn!(
                    attempt = attempt + 1,
                    max_attempts = retry::MAX_ATTEMPTS,
                    delay_ms = delay,
                    "provider overloaded; retrying in {} ms: {}",
                    delay,
                    response.result.trim(),
                );
                tokio::time::sleep(std::time::Duration::from_millis(delay)).await;
                attempt += 1;
            }
            None => {
                warn!(
                    max_attempts = retry::MAX_ATTEMPTS,
                    "provider overloaded; exhausted {} attempts, surfacing error: {}",
                    retry::MAX_ATTEMPTS,
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
async fn send_streaming_with_retry(
    session: &mut ClaudeSession,
    prompt: &str,
    channel: &Channel<ClaudeEvent>,
    question_slot: &QuestionSlot,
    permission_slot: &PermissionSlot,
    interrupt_slot: &InterruptSlot,
) -> Result<AgentResponse, AppError> {
    let mut attempt: u32 = 0;
    loop {
        let response = session
            .send_message_streaming(
                prompt,
                channel,
                question_slot,
                permission_slot,
                interrupt_slot,
            )
            .await?;

        if !(response.is_error && retry::is_overloaded(&response.result)) {
            return Ok(response);
        }

        match retry::backoff_ms(attempt) {
            Some(delay) => {
                warn!(
                    attempt = attempt + 1,
                    max_attempts = retry::MAX_ATTEMPTS,
                    delay_ms = delay,
                    "provider overloaded [streaming]; retrying in {} ms: {}",
                    delay,
                    response.result.trim(),
                );
                // `attempt` is the 0-based index of the attempt that just
                // failed, so the upcoming retry is the 1-based `attempt + 2`.
                let _ = channel.send(ClaudeEvent::Retrying {
                    attempt: attempt + 2,
                    max_attempts: retry::MAX_ATTEMPTS,
                    backoff_ms: delay,
                    error: response.result.clone(),
                });
                tokio::time::sleep(std::time::Duration::from_millis(delay)).await;
                attempt += 1;
            }
            None => {
                warn!(
                    max_attempts = retry::MAX_ATTEMPTS,
                    "provider overloaded [streaming]; exhausted {} attempts, surfacing error: {}",
                    retry::MAX_ATTEMPTS,
                    response.result.trim(),
                );
                return Ok(response);
            }
        }
    }
}

/// Shared send pipeline used by `agent_start_loop` and `agent_send_message`.
///
/// Records the user turn to the transcript *before* sending (so a crash
/// mid-turn still captures intent), sends it, records the assistant turn, and
/// returns the structured response. The transcript append is best-effort: a
/// write failure is logged but doesn't fail the turn — the live result still
/// reaches the UI.
async fn send_and_record(
    state: &AppState,
    path: &Path,
    prompt: &str,
) -> Result<AgentResponse, AppError> {
    let session_arc = with_session(state, path).await?;
    let mut session = session_arc.lock().await;
    let qslot = question_slot(state, path)?;
    let pslot = permission_slot(state, path)?;
    let islot = interrupt_slot(state, path)?;

    // 1. Record the user turn first (crash-safety: intent survives).
    if let Err(e) = conversation::append_turn(path, &ConversationTurn::user(prompt)) {
        tracing::warn!("failed to append user turn to transcript: {e}");
    }

    // 2. Send + receive (with retry on transient 529 overload).
    let response = send_with_retry(&mut session, prompt, &qslot, &pslot, &islot).await?;

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
    channel: &Channel<ClaudeEvent>,
) -> Result<(), AppError> {
    let session_arc = with_session(state, path).await?;
    let mut session = session_arc.lock().await;
    let qslot = question_slot(state, path)?;
    let pslot = permission_slot(state, path)?;
    let islot = interrupt_slot(state, path)?;

    // 1. Record the user turn first (crash-safety: intent survives).
    if let Err(e) = conversation::append_turn(path, &ConversationTurn::user(prompt)) {
        tracing::warn!("failed to append user turn to transcript: {e}");
    }

    // 2. Send + stream (with retry on transient 529 overload).
    let response =
        send_streaming_with_retry(&mut session, prompt, channel, &qslot, &pslot, &islot).await?;

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
) -> Result<Arc<tokio::sync::Mutex<ClaudeSession>>, AppError> {
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
    conversation::archive_conversation(path)?;

    // ── Phase 4: spawn fresh (no --resume) and insert. ──
    let agent_config = resolve_agent_config(state)?;

    let session = ClaudeSession::spawn(
        &path.to_path_buf(),
        &agent_config,
        None,
        PermissionPolicy::confirm_changes(),
    )?;
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
async fn start_fresh_and_record(
    state: &AppState,
    path: &Path,
    prompt: &str,
) -> Result<AgentResponse, AppError> {
    let session_arc = spawn_fresh(state, path).await?;
    let mut session = session_arc.lock().await;
    let qslot = question_slot(state, path)?;
    let pslot = permission_slot(state, path)?;
    let islot = interrupt_slot(state, path)?;

    // 1. Record the user turn first (crash-safety: intent survives).
    //    Marked `user_loop` — this prompt was auto-built from
    //    `.loopdeck/loops.md` by `build_next_loop_prompt`, not typed by the
    //    human. The UI renders these as compact system rows instead of
    //    user chat bubbles so they don't drown out real messages.
    if let Err(e) = conversation::append_turn(path, &ConversationTurn::user_loop(prompt)) {
        tracing::warn!("failed to append user turn to transcript: {e}");
    }

    tracing::info!(
        "sending loop prompt ({} chars) to claude: {:?}",
        prompt.len(),
        truncate_prompt(prompt),
    );

    // 2. Send + receive (with retry on transient 529 overload).
    let response = send_with_retry(&mut session, prompt, &qslot, &pslot, &islot).await?;

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
async fn start_fresh_and_record_streaming(
    state: &AppState,
    path: &Path,
    prompt: &str,
    channel: &Channel<ClaudeEvent>,
) -> Result<(), AppError> {
    let session_arc = spawn_fresh(state, path).await?;
    let mut session = session_arc.lock().await;
    let qslot = question_slot(state, path)?;
    let pslot = permission_slot(state, path)?;
    let islot = interrupt_slot(state, path)?;

    // 1. Record the user turn first (crash-safety: intent survives).
    //    Marked `user_loop` (see `start_fresh_and_record` for rationale).
    if let Err(e) = conversation::append_turn(path, &ConversationTurn::user_loop(prompt)) {
        tracing::warn!("failed to append user turn to transcript: {e}");
    }

    tracing::info!(
        "sending loop prompt ({} chars) to claude [streaming]: {:?}",
        prompt.len(),
        truncate_prompt(prompt),
    );

    // 2. Send + stream (with retry on transient 529 overload).
    let response =
        send_streaming_with_retry(&mut session, prompt, channel, &qslot, &pslot, &islot).await?;

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

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

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

        let prompt = build_next_loop_prompt(&dir);
        assert!(prompt.contains("Wire up the thing"));
        assert!(prompt.contains("loopdeck-orchestrator"));

        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn build_prompt_falls_back_when_no_step() {
        let dir = std::env::temp_dir().join(format!("loopdeck-prompt-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();

        let prompt = build_next_loop_prompt(&dir);
        assert!(prompt.contains("propose and start"));
        assert!(!prompt.contains("next unchecked step is"));

        std::fs::remove_dir_all(&dir).unwrap();
    }

    // ── parse_skill_frontmatter ──

    #[test]
    fn parse_frontmatter_extracts_name_and_description() {
        let content = "---\n\
             name: loopdeck:rust-expert\n\
             description: Use when writing Rust backend code.\n\
             allowed-tools: [Read, Write, Edit]\n\
             ---\n\n\
             # Rust Expert\n\nBody text.";
        let (name, description, argument_hint) = parse_skill_frontmatter(content);
        assert_eq!(name, "loopdeck:rust-expert");
        assert_eq!(description, "Use when writing Rust backend code.");
        // No `argument-hint:` field → empty.
        assert!(argument_hint.is_empty());
    }

    #[test]
    fn parse_frontmatter_extracts_argument_hint() {
        // The orchestrator skill carries an `argument-hint` with a hyphenated
        // key — the parser must match it literally, not just bare `word:` keys.
        let content = "---\n\
             name: loopdeck:orchestrator\n\
             description: Orchestrate feature implementation from a PRD.\n\
             argument-hint: <prd-file-path>\n\
             allowed-tools: [Read, Write, Edit, Skill]\n\
             ---\n\n\
             # Orchestrator";
        let (name, description, argument_hint) = parse_skill_frontmatter(content);
        assert_eq!(name, "loopdeck:orchestrator");
        assert_eq!(
            description,
            "Orchestrate feature implementation from a PRD."
        );
        assert_eq!(argument_hint, "<prd-file-path>");
    }

    #[test]
    fn parse_frontmatter_handles_indented_keys() {
        // Frontmatter keys may carry leading spaces in hand-edited files; the
        // parser trims before matching the `key:` prefix.
        let content = "---\n  name: spaced-name\n  description: spaced desc\n---\nbody";
        let (name, description, _) = parse_skill_frontmatter(content);
        assert_eq!(name, "spaced-name");
        assert_eq!(description, "spaced desc");
    }

    #[test]
    fn parse_frontmatter_returns_empties_without_frontmatter() {
        // A SKILL.md with no frontmatter block can't be invoked by name, so the
        // parser returns empties and `list_skills` skips it.
        let content = "# Just a heading\n\nNo frontmatter here.";
        let (name, description, argument_hint) = parse_skill_frontmatter(content);
        assert!(name.is_empty());
        assert!(description.is_empty());
        assert!(argument_hint.is_empty());
    }

    #[test]
    fn parse_frontmatter_missing_description_defaults_empty() {
        // `name` is required for the menu, `description` is optional.
        let content = "---\nname: only-name\n---\nbody";
        let (name, description, _) = parse_skill_frontmatter(content);
        assert_eq!(name, "only-name");
        assert!(description.is_empty());
    }

    #[test]
    fn parse_frontmatter_ignores_body_hrs() {
        // A `---` in the body (horizontal rule) must not confuse the parser —
        // only the FIRST closing `---` after the opening one ends the block.
        let content = "---\nname: a\n---\n\nparagraph\n\n---\n\nmore";
        let (name, _, _) = parse_skill_frontmatter(content);
        assert_eq!(name, "a");
    }

    // ── list_skills ──

    #[tokio::test]
    async fn list_skills_reads_installed_skills() {
        let dir = std::env::temp_dir().join(format!("loopdeck-skills-{}", uuid::Uuid::new_v4()));
        let skills_dir = dir.join(".claude").join("skills");
        std::fs::create_dir_all(skills_dir.join("loopdeck-rust-expert")).unwrap();
        std::fs::write(
            skills_dir.join("loopdeck-rust-expert").join("SKILL.md"),
            "---\nname: loopdeck:rust-expert\ndescription: Rust expert skill.\n---\nbody",
        )
        .unwrap();
        // The orchestrator carries an `argument-hint` — verify it round-trips.
        std::fs::create_dir_all(skills_dir.join("loopdeck-orchestrator")).unwrap();
        std::fs::write(
            skills_dir
                .join("loopdeck-orchestrator")
                .join("SKILL.md"),
            "---\nname: loopdeck:orchestrator\ndescription: Orchestrates.\nargument-hint: <prd-file-path>\n---\nbody",
        )
        .unwrap();

        let result = list_skills(dir.to_string_lossy().to_string())
            .await
            .unwrap();

        // Sorted by name → orchestrator before rust-expert.
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].name, "loopdeck:orchestrator");
        assert_eq!(result[0].directory, "loopdeck-orchestrator");
        assert_eq!(result[0].description, "Orchestrates.");
        assert_eq!(result[0].argument_hint, "<prd-file-path>");
        assert_eq!(result[1].name, "loopdeck:rust-expert");
        assert_eq!(result[1].directory, "loopdeck-rust-expert");
        assert_eq!(result[1].description, "Rust expert skill.");
        // No `argument-hint` field → empty string.
        assert!(result[1].argument_hint.is_empty());

        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[tokio::test]
    async fn list_skills_empty_when_not_bootstrapped() {
        // A project with no `.claude/skills/` (not yet bootstrapped) returns an
        // empty list, not an error — the menu shows "no skills".
        let dir = std::env::temp_dir().join(format!("loopdeck-skills-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();

        let result = list_skills(dir.to_string_lossy().to_string())
            .await
            .unwrap();
        assert!(result.is_empty());

        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[tokio::test]
    async fn list_skills_skips_dir_without_skillmd() {
        let dir = std::env::temp_dir().join(format!("loopdeck-skills-{}", uuid::Uuid::new_v4()));
        let skills_dir = dir.join(".claude").join("skills");
        std::fs::create_dir_all(skills_dir.join("valid-skill")).unwrap();
        std::fs::write(
            skills_dir.join("valid-skill").join("SKILL.md"),
            "---\nname: valid\n---\nbody",
        )
        .unwrap();
        // A dir with no SKILL.md — should be skipped, not crash.
        std::fs::create_dir_all(skills_dir.join("empty-dir")).unwrap();
        // A dir whose SKILL.md has no parseable name — also skipped.
        std::fs::create_dir_all(skills_dir.join("no-name")).unwrap();
        std::fs::write(
            skills_dir.join("no-name").join("SKILL.md"),
            "# no frontmatter\nbody",
        )
        .unwrap();
        // A loose file directly under skills/ — ignored (only dirs are skills).
        std::fs::write(skills_dir.join("loose-file.md"), "whatever").unwrap();

        let result = list_skills(dir.to_string_lossy().to_string())
            .await
            .unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].name, "valid");

        std::fs::remove_dir_all(&dir).unwrap();
    }
}
