//! Composer commands: filesystem listing/search and skill discovery for the
//! chat composer's `@`-mention and `/`-skill menus.

use super::state::{blocking_task_failed, AppState, DirEntry, SkillEntry};
use crate::error::AppError;
use crate::limits;
use crate::paths;
use crate::scanner::{self, DiscoveredRepo};
use std::path::{Path, PathBuf};
use tauri::State;
use tracing::{debug, info};

/// List the direct children of a directory inside a project, for the
/// composer's `@`-mention autocomplete.
///
/// `path` is the canonical project root (the same key used by every other
/// project command); `subdir` is a project-relative path identifying which
/// directory to list (empty string = project root). The user navigates into
/// subfolders by selecting folders, which the frontend turns into successive
/// calls with deeper `subdir` values.
///
/// Security: `path` is resolved to a canonical, **registered** project root
/// via `paths::resolve_registered_root`, and `subdir` is resolved beneath it
/// via `paths::resolve_within` — so `../` escapes, absolute paths, symlink
/// redirects out of the project, and unregistered paths are all rejected by
/// the shared boundary helpers (PRD FR3). Hidden entries (dotfiles) and the
/// same `IGNORED_DIRS` used by project scanning are filtered out.
#[tauri::command]
pub async fn list_dir_entries(
    path: String,
    subdir: String,
    state: State<'_, AppState>,
) -> Result<Vec<DirEntry>, AppError> {
    debug!("list_dir_entries called: path={path:?}, subdir={subdir:?}");

    // Resolve the canonical, registered project root (PRD FR3): the IPC path is
    // untrusted input, so it must canonicalize to a real directory that's
    // actually in the registry before we list anything beneath it.
    let root = {
        let config = state.config.lock().map_err(|_| AppError::LockError)?;
        paths::resolve_registered_root(&config, &path)?
    };

    // Resolve the subdirectory beneath the canonical root via the shared
    // boundary helper: rejects `..` traversal, absolute paths, and symlink
    // escape (a symlinked subdir pointing outside the root canonicalizes out
    // and fails the starts_with check). An empty `subdir` resolves to the root
    // itself.
    let target = paths::resolve_within(&root, &subdir, true)?;

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
    dirs.sort_by_key(|a| a.name.to_lowercase());
    files.sort_by_key(|a| a.name.to_lowercase());
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
///
/// Within a tier, shorter paths rank first (shallower, less noise).
///
/// `walk_root` is a recursion helper that walks `dir` and pushes matches.
///
/// Recursion is bounded by [`SearchBudget`] (PRD FR4): a max depth (also guards
/// the call stack), a max visited-entry count, and a wall-clock cap. When any
/// budget is exhausted the walk stops — the autocomplete popup returns what it
/// found rather than freezing on a huge tree.
fn walk_root(
    dir: &Path,
    root: &Path,
    query_lower: &str,
    out: &mut Vec<(DirEntry, u8, usize)>,
    depth: u8,
    budget: &mut SearchBudget,
) {
    if !budget.allow(depth) {
        return;
    }

    let read_dir = match std::fs::read_dir(dir) {
        Ok(rd) => rd,
        Err(_) => return, // permission denied / vanished — skip this subtree
    };
    for entry in read_dir.flatten() {
        // Each iteration consumes one entry budget unit; stop descending once
        // exhausted.
        if !budget.visit() {
            return;
        }
        let file_type = match entry.file_type() {
            Ok(ft) => ft,
            Err(_) => continue,
        };
        let name = entry.file_name().to_string_lossy().into_owned();

        // Do not follow symlinked directories (PRD FR3): skip symlinks entirely
        // so the search can't traverse out of the project root via a planted
        // link and never descends into one. `DirEntry::file_type` does not
        // follow symlinks, so this also keeps the walk bounded to real entries.
        if file_type.is_symlink() {
            continue;
        }

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
                walk_root(&entry.path(), root, query_lower, out, depth + 1, budget);
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
            walk_root(&entry.path(), root, query_lower, out, depth + 1, budget);
        }
    }
}

/// Resource budget for a single `@`-mention search walk (PRD FR4).
///
/// Tracks visited entries and elapsed time against configurable ceilings.
/// [`new`] seeds them from the [`limits`] constants; [`with`] lets tests inject
/// small values. Once any budget is breached the walk short-circuits: [`allow`]
/// returns `false` (don't descend further) and [`visit`] returns `false` (stop
/// iterating the current directory). The result cap itself lives in
/// `search_project_files` (`max_results`), since it bounds the returned list
/// rather than the walk.
struct SearchBudget {
    entries: usize,
    max_entries: usize,
    max_depth: u8,
    max_duration: std::time::Duration,
    start: std::time::Instant,
    exhausted: bool,
}

impl SearchBudget {
    fn new() -> Self {
        Self::with(
            limits::SEARCH_MAX_DEPTH,
            limits::SEARCH_MAX_ENTRIES,
            limits::SEARCH_MAX_DURATION,
        )
    }

    /// Construct with explicit ceilings. Production uses [`new`] (the
    /// [`limits`] constants); tests pass small values to exercise the budgets.
    fn with(max_depth: u8, max_entries: usize, max_duration: std::time::Duration) -> Self {
        Self {
            entries: 0,
            max_entries,
            max_depth,
            max_duration,
            start: std::time::Instant::now(),
            exhausted: false,
        }
    }

    /// Whether the walk may descend to `depth`. Records exhaustion (and returns
    /// `false`) once depth, entries, or time is over budget.
    fn allow(&mut self, depth: u8) -> bool {
        if self.exhausted {
            return false;
        }
        if depth > self.max_depth
            || self.entries >= self.max_entries
            || self.start.elapsed() >= self.max_duration
        {
            self.exhausted = true;
            tracing::warn!(
                depth = depth,
                entries = self.entries,
                elapsed = ?self.start.elapsed(),
                "search walk hit a budget — returning partial results"
            );
            return false;
        }
        true
    }

    /// Consume one visited entry; returns `false` when the entry budget is now
    /// exhausted (caller stops iterating the current directory).
    fn visit(&mut self) -> bool {
        self.entries += 1;
        if self.entries > self.max_entries {
            if !self.exhausted {
                self.exhausted = true;
                tracing::warn!(
                    entries = self.entries,
                    limit = self.max_entries,
                    "search walk hit entry budget — returning partial results"
                );
            }
            return false;
        }
        true
    }
}

#[tauri::command]
pub async fn search_project_files(
    path: String,
    query: String,
    max_results: Option<usize>,
    state: State<'_, AppState>,
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

    // Resolve the canonical, registered project root (PRD FR3) before walking.
    let root = {
        let config = state.config.lock().map_err(|_| AppError::LockError)?;
        paths::resolve_registered_root(&config, &path)?
    };

    let cap = max_results.unwrap_or(50);
    let mut hits: Vec<(DirEntry, u8, usize)> = Vec::new();
    let mut budget = SearchBudget::new();
    walk_root(&root, &root, &query_lower, &mut hits, 0, &mut budget);

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
pub async fn list_skills(
    path: String,
    state: State<'_, AppState>,
) -> Result<Vec<SkillEntry>, AppError> {
    debug!("list_skills called: path={path:?}");

    // Resolve the canonical, registered project root (PRD FR3) before reading
    // `.claude/skills/` beneath it.
    let root = {
        let config = state.config.lock().map_err(|_| AppError::LockError)?;
        paths::resolve_registered_root(&config, &path)?
    };

    list_skills_at(&root)
}

/// Read and parse the skills under `<root>/.claude/skills/`.
///
/// Factored out of the [`list_skills`] command so the read path is unit-
/// testable without a registered `AppState` — the registration check is the
/// command layer's concern; this is pure filesystem read + frontmatter parse.
fn list_skills_at(root: &Path) -> Result<Vec<SkillEntry>, AppError> {
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
        // Bounded read (PRD FR4): a planted oversized SKILL.md shouldn't be
        // loaded wholly into memory — only the frontmatter is parsed anyway.
        let content = match limits::read_bounded_to_string(&skill_md, limits::SKILL_MAX_BYTES) {
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    // ── search walk resource budgets (PRD FR4) ─────────────────────────

    /// Build a temp project root with `depth` nested dirs, each containing a
    /// file whose basename matches `query` — so the walk would visit every
    /// level without the budget.
    fn nested_match_tree(depth: usize, query: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!("loopdeck-walk-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let mut cur = root.clone();
        for _ in 0..depth {
            cur = cur.join("sub");
            std::fs::create_dir_all(&cur).unwrap();
            std::fs::write(cur.join(format!("{query}.txt")), "x").unwrap();
        }
        root
    }

    #[test]
    fn walk_root_depth_budget_stops_descent() {
        // A tree 6 levels deep, but a depth budget of 2 — only the first two
        // levels' matches should be returned; deeper ones are pruned.
        let query = "match";
        let root = nested_match_tree(6, query);
        let mut hits: Vec<(DirEntry, u8, usize)> = Vec::new();
        let mut budget = SearchBudget::with(2, 100_000, std::time::Duration::from_secs(30));
        walk_root(&root, &root, query, &mut hits, 0, &mut budget);

        // Depth 0 = root level (no match file at root), depth 1 = sub/match,
        // depth 2 = sub/sub/match. Beyond depth 2 must be pruned.
        let paths: Vec<&str> = hits.iter().map(|(e, _, _)| e.path.as_str()).collect();
        assert!(
            paths.iter().any(|p| p.ends_with("sub/match.txt")),
            "depth-1 hit present"
        );
        assert!(
            paths.iter().any(|p| p.ends_with("sub/sub/match.txt")),
            "depth-2 hit present"
        );
        assert!(
            !paths.iter().any(|p| p.ends_with("sub/sub/sub/match.txt")),
            "depth-3 hit must be pruned by the depth budget"
        );

        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn walk_root_entry_budget_stops_walk() {
        // 50 sibling files all matching, but an entry budget of 5 — the walk
        // must stop early and return only a handful of hits.
        let query = "f";
        let root = std::env::temp_dir().join(format!("loopdeck-walk-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        for i in 0..50 {
            std::fs::write(root.join(format!("{query}{i}.txt")), "x").unwrap();
        }

        let mut hits: Vec<(DirEntry, u8, usize)> = Vec::new();
        let mut budget = SearchBudget::with(15, 5, std::time::Duration::from_secs(30));
        walk_root(&root, &root, query, &mut hits, 0, &mut budget);

        assert!(
            hits.len() < 50,
            "entry budget should stop the walk before visiting all 50 files"
        );
        assert!(!hits.is_empty(), "some hits should still be returned");

        std::fs::remove_dir_all(&root).unwrap();
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

        let result = list_skills_at(&dir).unwrap();

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

    #[test]
    fn list_skills_empty_when_not_bootstrapped() {
        // A project with no `.claude/skills/` (not yet bootstrapped) returns an
        // empty list, not an error — the menu shows "no skills".
        let dir = std::env::temp_dir().join(format!("loopdeck-skills-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();

        let result = list_skills_at(&dir).unwrap();
        assert!(result.is_empty());

        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn list_skills_skips_dir_without_skillmd() {
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

        let result = list_skills_at(&dir).unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].name, "valid");

        std::fs::remove_dir_all(&dir).unwrap();
    }
}
