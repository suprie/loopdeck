//! Parser for the spec layer under `docs/epics/`.
//!
//! Sibling to `memory.rs`. Where `memory.rs` parses the runtime layer
//! (`.loopdeck/*.md`, lenient `**Field**` bullets), this module parses the
//! spec layer (`docs/epics/**/*.md`, strict YAML frontmatter + prose body).
//!
//! The layer difference is deliberate (ADR-3 in the support-project-management
//! epic): spec files are *indexed* — grouped by milestone, filtered by status
//! — so the index fields are structured frontmatter; the body is prose and is
//! only line-scanned where structure is needed (`### Phase` checklists in
//! PRDs). Aggregate calls (`parse_epics`) are lenient (missing
//! `docs/epics/` → empty); per-file parsing is strict (a missing required
//! frontmatter field is an error that is logged with the file path and the
//! epic skipped, never a panic).

use crate::error::AppError;
use crate::limits;
use crate::memory;
use crate::paths;
use crate::persist;
use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};

// ── Frontmatter (strict; parsed via serde_yaml) ─────────────────────

/// YAML frontmatter for an epic README.
///
/// Required: `title`, `slug`, `milestone`, `status`, `description`.
/// `milestone` is a string (quoted `"0.2.0"` in the file to avoid YAML
/// parsing it as a float).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct EpicFrontmatter {
    pub title: String,
    pub slug: String,
    pub milestone: String,
    pub status: String,
    pub description: String,
    #[serde(default)]
    pub started: Option<String>,
    #[serde(default)]
    pub completed: Option<String>,
    #[serde(default)]
    pub owner: Option<String>,
}

/// YAML frontmatter for a PRD (`prd-*.md`).
///
/// Required: `prd`, `epic`, `status`, `description`. `milestone` is
/// denormalized from the epic so the `/epics` view can group PRDs without
/// joining back to the README.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PrdFrontmatter {
    pub prd: String,
    pub epic: String,
    pub status: String,
    pub description: String,
    #[serde(default)]
    pub milestone: Option<String>,
}

// ── Public structs (returned to the UI) ─────────────────────────────

/// A parsed epic from `docs/epics/<slug>/README.md`, with its PRDs attached.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Epic {
    pub slug: String,
    pub title: String,
    pub milestone: String,
    pub status: String,
    pub description: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub started: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub completed: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub owner: Option<String>,
    /// Path to the epic directory (back-reference for the promote action).
    pub dir: String,
    #[serde(default)]
    pub prds: Vec<Prd>,
}

/// A parsed PRD, with its phase checklists attached.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Prd {
    /// PRD slug — the `prd:` frontmatter field (filename without `.md`).
    pub slug: String,
    /// Parent epic slug.
    pub epic: String,
    pub status: String,
    pub description: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub milestone: Option<String>,
    /// Filename of the PRD file (back-reference for the promote action).
    pub file: String,
    #[serde(default)]
    pub phases: Vec<PrdPhase>,
}

/// A `### Phase N — Name` section within a PRD's `## Phases`.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct PrdPhase {
    /// Full heading text, e.g. `Phase 1 — Core structs and parser`.
    pub name: String,
    #[serde(default)]
    pub loops: Vec<PrdLoop>,
}

/// A single checklist item inside a phase — the atomic unit the
/// promote-to-loop action acts on.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct PrdLoop {
    pub title: String,
    pub checked: bool,
    /// Read-only sync from `loops.md ## History`: true when a history entry's
    /// goal matches this loop's title. Title-drift is accepted (no fuzzy match).
    #[serde(default)]
    pub done_in_history: bool,
    /// Stable, project-scoped loop ID of the form `<prd-short-slug>/<loop-slug>`,
    /// parsed from a leading backtick token on the checklist item (e.g.
    /// `structured-state/parse-loop-id`). `None` for legacy ID-less items and
    /// for items whose leading token is not a well-formed ID — both stay
    /// readable but cannot be promoted (Phase 1). Titles are presentation; the
    /// ID is the identity the spec→execution bridge joins on (Phase 3).
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub id: Option<String>,
}

// ── Public API ──────────────────────────────────────────────────────

/// Parse every epic under `<repo>/docs/epics/`, attaching each epic's PRDs.
///
/// Lenient at the aggregate level: a missing `docs/epics/` directory returns
/// an empty Vec (the UI degrades to "no epics yet"). A single malformed epic
/// or PRD is logged with its path and skipped — it never aborts the whole
/// collection.
pub fn parse_epics(repo_path: &Path) -> Vec<Epic> {
    let epics_dir = repo_path.join("docs").join("epics");
    let entries = match std::fs::read_dir(&epics_dir) {
        Ok(e) => e,
        Err(_) => {
            tracing::debug!("docs/epics not readable at {}", epics_dir.display());
            return Vec::new();
        }
    };

    let mut epics: Vec<Epic> = Vec::new();
    for entry in entries.flatten() {
        let dir = entry.path();
        if !dir.is_dir() {
            continue;
        }
        let readme = dir.join("README.md");
        if !readme.exists() {
            continue;
        }
        match parse_epic_readme(&readme) {
            Ok(mut epic) => {
                epic.prds = read_prds(&dir);
                epics.push(epic);
            }
            Err(e) => {
                tracing::warn!("failed to parse epic README {}: {e}", readme.display());
            }
        }
    }

    // Deterministic order for the UI.
    epics.sort_by(|a, b| a.slug.cmp(&b.slug));

    // Enrich each PRD loop with done_in_history: a loop is "done" when a
    // `## History` entry's goal matches its title. Scanned once here so the
    // cost is one `parse_loops` per aggregate call, not per PRD.
    let history = memory::parse_loops(repo_path).history;
    let done_titles: HashSet<&str> = history.iter().map(|h| h.goal.as_str()).collect();
    if !done_titles.is_empty() {
        for epic in &mut epics {
            for prd in &mut epic.prds {
                for phase in &mut prd.phases {
                    for l in &mut phase.loops {
                        if done_titles.contains(l.title.as_str()) {
                            l.done_in_history = true;
                        }
                    }
                }
            }
        }
    }

    // Surface stable-loop-ID problems (malformed / duplicate) as warnings. Non-
    // fatal — the lenient aggregate ethos is preserved; a future reconciliation
    // UI (Phase 5) renders these diagnostics. See `validate_loop_ids`.
    for diag in validate_loop_ids(repo_path) {
        match diag.kind {
            LoopIdDiagKind::Malformed => tracing::warn!(
                "malformed loop ID `{}` at docs/epics/{}:{} \
                 (expected `<lowercase-kebab>/<lowercase-kebab>`)",
                diag.id,
                diag.file,
                diag.line
            ),
            LoopIdDiagKind::Duplicate => tracing::warn!(
                "duplicate loop ID `{}` at docs/epics/{}:{} \
                 (an item with this ID is already defined)",
                diag.id,
                diag.file,
                diag.line
            ),
        }
    }

    epics
}

/// Group epics by milestone, ordered by milestone label.
///
/// Epics whose frontmatter has an empty/missing milestone fall into the
/// `"Unmilestoned"` bucket.
pub fn epics_by_milestone(repo_path: &Path) -> BTreeMap<String, Vec<Epic>> {
    let mut map: BTreeMap<String, Vec<Epic>> = BTreeMap::new();
    for epic in parse_epics(repo_path) {
        let key = if epic.milestone.trim().is_empty() {
            "Unmilestoned".to_string()
        } else {
            epic.milestone.clone()
        };
        map.entry(key).or_default().push(epic);
    }
    map
}

// ── Promote-to-loop bridge (spec → runtime) ─────────────────────────

/// Promote a PRD checklist item into `.loopdeck/loops.md ## Current`.
///
/// The bridge between the spec layer (intention) and the runtime layer
/// (execution). Writes the loop with `**Epic**` / `**PRD**` back-reference
/// bullets so the runner skill can follow them to the origin PRD. The agent's
/// `build_next_loop_prompt` is unchanged — it reads `**Goal**` as today and
/// ignores the unknown back-reference fields.
///
/// **Clobber guard:** refuses to overwrite a non-empty `## Current`. The
/// caller must complete or abandon the current loop first. Never clobbers
/// in-progress work.
///
/// **Postcondition:** the PRD checklist item is NOT auto-checked. The human
/// marks it done after the loop completes and lands in History.
pub fn promote_epic_loop(
    repo_path: &Path,
    epic_slug: &str,
    prd_filename: &str,
    loop_title: &str,
) -> Result<(), AppError> {
    let loopdeck_dir = repo_path.join(".loopdeck");
    let loops_file = loopdeck_dir.join("loops.md");

    // Ensure the runtime dir + seed file exist before we read/rewrite.
    // `ensure_memory_files` only seeds files when `.loopdeck/` already exists,
    // so create the dir first (idempotent).
    std::fs::create_dir_all(&loopdeck_dir)?;
    if !loops_file.exists() {
        memory::ensure_memory_files(repo_path)?;
    }

    let content = limits::read_bounded_to_string(&loops_file, limits::SPEC_MAX_BYTES)?;

    // Clobber guard: refuse if a loop is already in progress.
    let status = memory::parse_loops(repo_path);
    if let Some(current) = status.current {
        return Err(AppError::Conflict(format!(
            "A loop is already in progress (\"{}\"). Complete or abandon it before promoting another.",
            current.goal
        )));
    }

    // PRD back-reference is the filename without the `.md` extension.
    let prd_ref = prd_filename
        .strip_suffix(".md")
        .unwrap_or(prd_filename)
        .to_string();
    let today = Utc::now().format("%Y-%m-%d").to_string();

    let new_current = format!(
        "## Current\n\
         \n\
         - **Started**: {today}\n\
         - **Goal**: {title}\n\
         - **Status**: in_progress\n\
         - **Epic**: {epic}\n\
         - **PRD**: {prd}\n",
        today = today,
        title = loop_title,
        epic = epic_slug,
        prd = prd_ref,
    );

    let rewritten = replace_current_section(&content, &new_current);
    persist::atomic_write(&loops_file, &rewritten)?;
    Ok(())
}
/// Toggle a `- [ ]` / `- [x]` checklist item inside a PRD's `## Phases` body.
///
/// The manual-sync action from `prd-epics-view.md` (Phase 4, P2): lets the human
/// mark a planned loop done (or un-mark it) without waiting for a History match.
/// Matches by the item text (trimmed comparison) and toggles the first match.
///
/// Returns `Ok(true)` if toggled to checked, `Ok(false)` if toggled to unchecked.
pub fn toggle_prd_loop(
    repo_path: &Path,
    epic_slug: &str,
    prd_filename: &str,
    loop_title: &str,
) -> Result<bool, AppError> {
    // Containment: `epic_slug` and `prd_filename` are joined into a filesystem
    // path, so route them through the shared boundary helper under the
    // canonical `docs/epics/` root. This rejects `../` in either field (which
    // would otherwise escape `docs/epics/` and read/write an arbitrary file)
    // and symlink-redirected prefixes. `must_exist = false` lets the resolver
    // canonicalize the parent without failing on the not-yet-checked target;
    // the explicit `exists()` below then gives the friendly not-found error.
    let epics_root = repo_path
        .join("docs")
        .join("epics")
        .canonicalize()
        .map_err(|e| AppError::InvalidPath(format!("docs/epics not found: {e}")))?;
    let prd_path =
        paths::resolve_within(&epics_root, &format!("{epic_slug}/{prd_filename}"), false)?;

    if !prd_path.exists() {
        return Err(AppError::ProjectNotFound(format!(
            "PRD file not found: {epic_slug}/{prd_filename}"
        )));
    }

    let content = limits::read_bounded_to_string(&prd_path, limits::SPEC_MAX_BYTES)?;
    let needle = loop_title.trim();
    let mut found: Option<(usize, bool)> = None;

    for (i, line) in content.lines().enumerate() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix("- [ ] ") {
            if rest.trim() == needle {
                found = Some((i, false));
                break;
            }
        } else if let Some(rest) = trimmed
            .strip_prefix("- [x] ")
            .or_else(|| trimmed.strip_prefix("- [X] "))
        {
            if rest.trim() == needle {
                found = Some((i, true));
                break;
            }
        }
    }

    let (line_idx, currently_checked) = found.ok_or_else(|| {
        AppError::ProjectNotFound(format!(
            "checklist item not found in {}: {loop_title}",
            prd_path.display()
        ))
    })?;

    let mut lines: Vec<String> = content.split('\n').map(String::from).collect();
    let old_line = &lines[line_idx];
    let indent = old_line.len() - old_line.trim_start().len();
    let new_box = if currently_checked { "- [ ]" } else { "- [x]" };
    let item_text = old_line.trim()[6..].trim_start();
    lines[line_idx] = format!("{}{} {}", " ".repeat(indent), new_box, item_text);

    let new_content = lines.join("\n");
    persist::atomic_write(&prd_path, &new_content)?;
    Ok(!currently_checked)
}

// ── Spec file read/write (sandboxed to docs/epics/) ──────────────────

/// Resolve a relative path under `docs/epics/` to an absolute, sandbox-safe
/// path. The traversal / absolute / symlink-escape checks are delegated to the
/// shared [`crate::paths::resolve_within`] helper (PRD FR3); this wrapper only
/// adds the spec-specific `.md` extension requirement and canonicalizes the
/// `docs/epics/` root.
///
/// - `must_exist: true` (reads): the target is canonicalized — symlinks
///   pointing outside the root resolve out and are rejected.
/// - `must_exist: false` (writes): the target may not exist yet, so the
///   longest existing ancestor is canonicalized and the rest re-appended.
fn resolve_spec_path(
    repo_path: &Path,
    rel_path: &str,
    must_exist: bool,
) -> Result<PathBuf, AppError> {
    if !rel_path.ends_with(".md") {
        return Err(AppError::InvalidPath(format!(
            "spec file must be a .md file: {rel_path}"
        )));
    }

    let epics_root = repo_path
        .join("docs")
        .join("epics")
        .canonicalize()
        .map_err(|e| AppError::InvalidPath(format!("docs/epics not found: {e}")))?;

    paths::resolve_within(&epics_root, rel_path, must_exist)
}

/// Read a spec file under `docs/epics/`. The relative path is sandboxed —
/// `../` escapes and symlinks pointing outside the root are rejected.
pub fn read_spec_file(repo_path: &Path, rel_path: &str) -> Result<String, AppError> {
    let path = resolve_spec_path(repo_path, rel_path, true)?;
    // Bounded read (PRD FR4): spec Markdown is untrusted content under the
    // project; cap so a planted oversized file isn't loaded wholly into memory.
    let content = limits::read_bounded_to_string(&path, limits::SPEC_MAX_BYTES)?;
    Ok(content)
}

/// Write (create or overwrite) a spec file under `docs/epics/`. The relative
/// path is sandboxed. Does not re-parse the frontmatter — it's a raw write; a
/// broken file will be logged and skipped by the next `parse_epics` call.
pub fn write_spec_file(repo_path: &Path, rel_path: &str, content: &str) -> Result<(), AppError> {
    let path = resolve_spec_path(repo_path, rel_path, false)?;
    // atomic_write creates parent dirs itself, but keep the explicit
    // create_dir_all so a missing-parent failure surfaces with the resolver's
    // path semantics rather than the atomic_write temp path.
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    persist::atomic_write(&path, content)?;
    Ok(())
}

/// Replace the `## Current` section of `loops.md` with `new_current`,
/// preserving everything else (Next Steps, History, preamble).
///
/// If no `## Current` section exists, inserts one at the top (after the `# Loops`
/// H1) — mirroring the seed shape from `ensure_memory_files`.
fn replace_current_section(content: &str, new_current: &str) -> String {
    // Split into lines and walk section by section, mirroring memory.rs's
    // `\n## ` approach. We rebuild line-by-line so trailing whitespace and
    // section order are preserved exactly.
    let mut out = String::new();
    let mut in_current = false;
    let mut wrote_current = false;
    let mut first_section_seen = false;

    for line in content.lines() {
        if line.starts_with("## ") {
            // Leaving whatever section we were in.
            if in_current {
                // We just finished replacing Current; emit nothing for the
                // original Current lines (they were already skipped).
                in_current = false;
            }
            if line == "## Current" {
                // Emit the replacement block once, skip the original lines.
                if !wrote_current {
                    out.push_str(new_current);
                    wrote_current = true;
                }
                in_current = true;
                first_section_seen = true;
                continue;
            }
            first_section_seen = true;
        } else if in_current {
            // Still inside the original ## Current block — skip its lines.
            continue;
        }
        out.push_str(line);
        out.push('\n');
    }

    // If the file had no ## Current section at all, insert one after the H1.
    if !wrote_current {
        let mut with_current = String::new();
        let mut inserted = false;
        for line in content.lines() {
            with_current.push_str(line);
            with_current.push('\n');
            // Insert after the `# Loops` H1 (or at the top if there's no H1).
            if !inserted && (line.starts_with("# ") || (!first_section_seen && line.is_empty())) {
                with_current.push('\n');
                with_current.push_str(new_current);
                inserted = true;
            }
        }
        if !inserted {
            // File was empty or had no H1 — just prepend.
            return format!("{new_current}\n{out}");
        }
        return with_current;
    }

    out
}

// ── Per-file parsing (strict frontmatter, lenient body) ─────────────

/// Parse a single epic README into an `Epic` (no PRDs attached).
///
/// Strict on frontmatter: a missing required field surfaces as
/// `AppError::Yaml` (carrying the serde_yaml error). Missing frontmatter
/// entirely is `AppError::Config` naming the file.
fn parse_epic_readme(path: &Path) -> Result<Epic, AppError> {
    let content = limits::read_bounded_to_string(path, limits::SPEC_MAX_BYTES)?;
    let (fm_str, _body) = split_frontmatter(&content).ok_or_else(|| {
        AppError::Config(format!(
            "epic README missing YAML frontmatter: {}",
            path.display()
        ))
    })?;
    let fm: EpicFrontmatter = serde_yaml::from_str(fm_str)?;

    let dir = path
        .parent()
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_default();

    Ok(Epic {
        slug: fm.slug,
        title: fm.title,
        milestone: fm.milestone,
        status: fm.status,
        description: fm.description,
        started: fm.started,
        completed: fm.completed,
        owner: fm.owner,
        dir,
        prds: Vec::new(),
    })
}

/// Parse a single PRD file into a `Prd` with its phase checklists.
fn parse_prd(path: &Path) -> Result<Prd, AppError> {
    let content = limits::read_bounded_to_string(path, limits::SPEC_MAX_BYTES)?;
    let (fm_str, body) = split_frontmatter(&content).ok_or_else(|| {
        AppError::Config(format!("PRD missing YAML frontmatter: {}", path.display()))
    })?;
    let fm: PrdFrontmatter = serde_yaml::from_str(fm_str)?;

    let file = path
        .file_name()
        .map(|f| f.to_string_lossy().into_owned())
        .unwrap_or_default();

    Ok(Prd {
        slug: fm.prd,
        epic: fm.epic,
        status: fm.status,
        description: fm.description,
        milestone: fm.milestone,
        file,
        phases: parse_prd_phases(body),
    })
}

/// Read and parse every `prd-*.md` co-located with an epic README.
///
/// Malformed PRDs are logged and skipped. Returns PRDs sorted by filename.
fn read_prds(epic_dir: &Path) -> Vec<Prd> {
    let mut prds: Vec<Prd> = Vec::new();
    let entries = match std::fs::read_dir(epic_dir) {
        Ok(e) => e,
        Err(_) => return Vec::new(),
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let Some(name) = path.file_name().and_then(|f| f.to_str()) else {
            continue;
        };
        if !name.starts_with("prd-") || !name.ends_with(".md") {
            continue;
        }
        match parse_prd(&path) {
            Ok(prd) => prds.push(prd),
            Err(e) => tracing::warn!("failed to parse PRD {}: {e}", path.display()),
        }
    }
    prds.sort_by(|a, b| a.file.cmp(&b.file));
    prds
}

// ── Body parsing internals ───────────────────────────────────────────

/// Split a `---\n…\n---` YAML frontmatter block from the prose body.
///
/// Returns `(frontmatter, body)`. The opening fence must be the first line;
/// the closing fence must be a line equal to `---`. Returns `None` when there
/// is no frontmatter (e.g. a runtime-style file). Tolerates CRLF line endings.
fn split_frontmatter(content: &str) -> Option<(&str, &str)> {
    let after_open = content
        .strip_prefix("---\n")
        .or_else(|| content.strip_prefix("---\r\n"))?;

    // Find the closing fence: a line equal to "---".
    let bytes = after_open.as_bytes();
    let mut search_from = 0;
    while search_from < after_open.len() {
        let rel = after_open[search_from..].find("\n---")?;
        let abs = search_from + rel;
        // Byte index just past the `\n---`.
        let after_fence = abs + 4;
        let is_line_end = after_fence == after_open.len()
            || after_open[after_fence..].starts_with('\n')
            || after_open[after_fence..].starts_with("\r\n");
        if is_line_end {
            let frontmatter = &after_open[..abs];
            let mut body_start = after_fence;
            // Consume the trailing line terminator so the body starts clean.
            if after_open[body_start..].starts_with("\r\n") {
                body_start += 2;
            } else if bytes.get(body_start) == Some(&b'\n') {
                body_start += 1;
            }
            return Some((frontmatter, &after_open[body_start..]));
        }
        search_from = abs + 4;
    }
    None
}

/// Parse the `## Phases` section of a PRD body into ordered phases.
///
/// Mirrors `memory.rs`'s section splitting: prepend a newline, split on
/// `\n## `, locate the `Phases` block, then split that on `\n### ` for each
/// phase. Missing `## Phases` → empty Vec (lenient, no panic).
fn parse_prd_phases(body: &str) -> Vec<PrdPhase> {
    let normalized = format!("\n{body}");
    for section in normalized.split("\n## ") {
        if let Some(rest) = section.strip_prefix("Phases") {
            return parse_phases_section(rest);
        }
    }
    Vec::new()
}

/// Parse the body of a `## Phases` section into phases.
fn parse_phases_section(section: &str) -> Vec<PrdPhase> {
    let mut phases: Vec<PrdPhase> = Vec::new();
    let mut segments = section.split("\n### ");
    segments.next(); // preamble before the first `### Phase` heading

    for segment in segments {
        let mut lines = segment.lines();
        let name = lines.next().unwrap_or("").trim().to_string();
        if name.is_empty() {
            continue;
        }
        let loops = parse_phase_loops(lines);
        phases.push(PrdPhase { name, loops });
    }
    phases
}

/// Collect `- [ ]` / `- [x]` checklist items from a phase body, preserving
/// the checked state and a stable loop ID when present.
///
/// A leading `` `<prd-short-slug>/<loop-slug>` `` backtick token is the loop's
/// stable ID; the trimmed remainder is the display title. A token that is not a
/// well-formed ID (see [`is_valid_loop_id_shape`]) is left in the title and the
/// item parses as ID-less — [`validate_loop_ids`] flags it separately. Legacy
/// items without any backtick token parse unchanged (`id: None`).
fn parse_phase_loops<'a, I: Iterator<Item = &'a str>>(lines: I) -> Vec<PrdLoop> {
    let mut loops: Vec<PrdLoop> = Vec::new();
    for line in lines {
        let trimmed = line.trim();
        let (rest, checked) = if let Some(r) = trimmed.strip_prefix("- [ ] ") {
            (r, false)
        } else if let Some(r) = trimmed
            .strip_prefix("- [x] ")
            .or_else(|| trimmed.strip_prefix("- [X] "))
        {
            (r, true)
        } else {
            continue;
        };
        let (id, title) = split_loop_id(rest);
        loops.push(PrdLoop {
            title,
            checked,
            done_in_history: false,
            id,
        });
    }
    loops
}

/// If `rest` begins with a backtick-quoted token, return `(token, text_after)`,
/// where `text_after` is everything past the closing backtick (untrimmed).
/// Returns `None` when `rest` does not start with a complete `` `...` `` pair.
fn split_leading_backtick(rest: &str) -> Option<(&str, &str)> {
    let after = rest.strip_prefix('`')?;
    let end = after.find('`')?;
    Some((&after[..end], &after[end + 1..]))
}

/// Split a leading `` `<kebab>/<kebab>` `` stable-ID token from a checklist
/// item body. Returns `(Some(id), title)` when a well-formed ID leads;
/// otherwise `(None, rest)` (legacy item, or a malformed token left in place).
fn split_loop_id(rest: &str) -> (Option<String>, String) {
    match split_leading_backtick(rest) {
        Some((id, after)) if is_valid_loop_id_shape(id) => {
            (Some(id.to_string()), after.trim().to_string())
        }
        _ => (None, rest.to_string()),
    }
}

/// A stable loop ID is `<lowercase-kebab>/<lowercase-kebab>` — exactly one `/`,
/// non-empty segments, ASCII lowercase letters / digits / hyphens only.
fn is_valid_loop_id_shape(id: &str) -> bool {
    let Some((namespace, slug)) = id.split_once('/') else {
        return false;
    };
    !namespace.is_empty()
        && !slug.is_empty()
        && !slug.contains('/')
        && namespace.bytes().all(is_id_byte)
        && slug.bytes().all(is_id_byte)
}

fn is_id_byte(b: u8) -> bool {
    b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-'
}

// ── Loop-ID validation ──────────────────────────────────────────────

/// One stable-loop-ID validation finding, with the source file + 1-based line
/// for remediation. Produced by [`validate_loop_ids`]; surfaced as `warn!`
/// from [`parse_epics`] and reserved for a future reconciliation UI (Phase 5).
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct LoopIdDiagnostic {
    /// PRD path relative to `docs/epics/`, e.g.
    /// `support-project-management/prd-structured-execution-state.md`.
    pub file: String,
    /// 1-based line number of the offending checklist item.
    pub line: usize,
    /// The ID token as authored.
    pub id: String,
    pub kind: LoopIdDiagKind,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum LoopIdDiagKind {
    /// A backtick token is present but is not a well-formed `<kebab>/<kebab>` ID.
    Malformed,
    /// This well-formed ID is also defined on another checklist item.
    Duplicate,
}

/// Validate stable loop IDs across every `prd-*.md` under `<repo>/docs/epics/`.
///
/// Considers **only checklist items inside a `## Phases` section and outside
/// fenced blocks**, so prose and ```markdown``` examples that document the ID
/// format are never flagged. Reports:
/// - [`LoopIdDiagKind::Malformed`] — a leading backtick token that is not a
///   well-formed `<kebab>/<kebab>` ID.
/// - [`LoopIdDiagKind::Duplicate`] — a well-formed ID reused on 2+ items
///   project-wide; one diagnostic per occurrence so every site is listed.
///
/// Legacy ID-less items are valid (not reported). Pure: reads files, returns
/// diagnostics, never writes. Lenient: an unreadable `docs/epics/` returns `[]`.
pub fn validate_loop_ids(repo_path: &Path) -> Vec<LoopIdDiagnostic> {
    let mut diags: Vec<LoopIdDiagnostic> = Vec::new();
    let mut occurrences: Vec<(String, usize, String)> = Vec::new(); // (file, line, id)

    let epics_dir = repo_path.join("docs").join("epics");
    let epic_entries = match std::fs::read_dir(&epics_dir) {
        Ok(e) => e,
        Err(_) => return diags,
    };
    for epic_entry in epic_entries.flatten() {
        let epic_dir = epic_entry.path();
        if !epic_dir.is_dir() {
            continue;
        }
        let Some(epic_slug) = epic_dir.file_name().and_then(|f| f.to_str()) else {
            continue;
        };
        let prd_entries = match std::fs::read_dir(&epic_dir) {
            Ok(e) => e,
            Err(_) => continue,
        };
        for prd_entry in prd_entries.flatten() {
            let path = prd_entry.path();
            let Some(name) = path.file_name().and_then(|f| f.to_str()) else {
                continue;
            };
            if !name.starts_with("prd-") || !name.ends_with(".md") {
                continue;
            }
            let Ok(content) = limits::read_bounded_to_string(&path, limits::SPEC_MAX_BYTES) else {
                continue;
            };
            let rel_file = format!("{epic_slug}/{name}");
            scan_prd_loop_ids(&content, &rel_file, &mut diags, &mut occurrences);
        }
    }

    // Duplicate pass: emit one Duplicate diagnostic per occurrence of any
    // well-formed ID used on 2+ items project-wide.
    let mut count_by_id: HashMap<&str, usize> = HashMap::new();
    for (_, _, id) in &occurrences {
        *count_by_id.entry(id.as_str()).or_default() += 1;
    }
    for (file, line, id) in &occurrences {
        if count_by_id[id.as_str()] >= 2 {
            diags.push(LoopIdDiagnostic {
                file: file.clone(),
                line: *line,
                id: id.clone(),
                kind: LoopIdDiagKind::Duplicate,
            });
        }
    }

    diags
}

/// Scan one PRD body's `## Phases` checklist items for loop-ID findings.
///
/// State machine over lines: skip the YAML frontmatter, skip fenced blocks,
/// and only consider checklist items while inside the `## Phases` section.
/// Appends malformed findings to `diags` and well-formed `(file, line, id)`
/// occurrences to `occurrences` (the caller resolves duplicates).
fn scan_prd_loop_ids(
    content: &str,
    rel_file: &str,
    diags: &mut Vec<LoopIdDiagnostic>,
    occurrences: &mut Vec<(String, usize, String)>,
) {
    let starts_with_fence = content
        .lines()
        .next()
        .map(|l| l.trim() == "---")
        .unwrap_or(false);
    let mut after_frontmatter = !starts_with_fence;
    let mut fence: Option<&'static str> = None;
    let mut in_phases = false;

    for (i, raw) in content.lines().enumerate() {
        let line_no = i + 1;
        let trimmed = raw.trim();
        if !after_frontmatter {
            if i > 0 && trimmed == "---" {
                after_frontmatter = true;
            }
            continue;
        }
        // Fenced-block tracking (toggle on ``` / ~~~ openers; close on a line
        // starting with the same fence).
        if fence.is_none() && (trimmed.starts_with("```") || trimmed.starts_with("~~~")) {
            fence = Some(if trimmed.starts_with("```") {
                "```"
            } else {
                "~~~"
            });
            continue;
        }
        if let Some(f) = fence {
            if trimmed.starts_with(f) {
                fence = None;
            }
            continue;
        }
        // `## ` heading — track whether we are inside the Phases section.
        if let Some(heading) = trimmed.strip_prefix("## ") {
            in_phases = heading.starts_with("Phases");
            continue;
        }
        if !in_phases {
            continue;
        }
        let Some(rest) = trimmed
            .strip_prefix("- [ ]")
            .or_else(|| trimmed.strip_prefix("- [x]"))
            .or_else(|| trimmed.strip_prefix("- [X]"))
        else {
            continue;
        };
        let rest = rest.trim_start();
        let Some((token, _after)) = split_leading_backtick(rest) else {
            continue; // no backtick token — legacy ID-less item, valid.
        };
        if is_valid_loop_id_shape(token) {
            occurrences.push((rel_file.to_string(), line_no, token.to_string()));
        } else if token.contains('/') {
            // A `/`-bearing token is an ID attempt with the wrong shape
            // (uppercase, underscore, dot, extra slashes). Flag it. A token
            // with no `/` is an ordinary code identifier in prose
            // (`get_epics`, `Bash`, `EpicsView.tsx`), not an ID attempt.
            diags.push(LoopIdDiagnostic {
                file: rel_file.to_string(),
                line: line_no,
                id: token.to_string(),
                kind: LoopIdDiagKind::Malformed,
            });
        }
    }
}

// ── Tests ────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn create_temp_repo() -> PathBuf {
        let dir = std::env::temp_dir().join(format!("loopdeck-epic-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(dir.join("docs").join("epics")).unwrap();
        dir
    }

    fn write_epic(repo: &Path, slug: &str, readme: &str) {
        let dir = repo.join("docs").join("epics").join(slug);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("README.md"), readme).unwrap();
    }

    fn write_prd(repo: &Path, epic_slug: &str, filename: &str, body: &str) {
        let dir = repo.join("docs").join("epics").join(epic_slug);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join(filename), body).unwrap();
    }

    const VALID_EPIC_README: &str = "\
---
title: Support Project Management
slug: support-project-management
milestone: \"0.2.0\"
status: in_progress
started: 2026-07-08
owner: Suprie
description: >
  Introduce an Epic hierarchy so a human can structure a large feature as a
  reviewable spec in docs/epics/.
---

# Epic — Support Project Management

## Motivation

Some prose about why.
";

    const VALID_PRD: &str = "\
---
prd: prd-spec-layer
epic: support-project-management
milestone: \"0.2.0\"
status: proposed
description: >
  Define the on-disk format for the spec layer and build the parser.
---

# PRD — Spec Layer

## Overview

Prose here.

## Phases

### Phase 1 — Core structs and parser
- [ ] Define Epic and Prd structs
- [x] Implement frontmatter extractor
- [ ] Write unit tests

### Phase 2 — Bootstrap and integration
- [ ] Add docs/epics/ to bootstrap
- [ ] Register Tauri commands
";

    // ── Frontmatter extraction tests ─────────────────────────────

    #[test]
    fn test_split_frontmatter_present() {
        let content = "---\nfoo: bar\n---\n\n# Body\n";
        let (fm, body) = split_frontmatter(content).unwrap();
        assert_eq!(fm, "foo: bar");
        assert_eq!(body, "\n# Body\n");
    }

    #[test]
    fn test_split_frontmatter_absent() {
        assert!(split_frontmatter("# No frontmatter\n\nbody").is_none());
    }

    #[test]
    fn test_split_frontmatter_crlf() {
        let content = "---\r\nfoo: bar\r\n---\r\nbody line\r\n";
        let (fm, body) = split_frontmatter(content).unwrap();
        assert_eq!(fm, "foo: bar\r");
        assert!(body.starts_with("body line"));
    }

    #[test]
    fn test_split_frontmatter_closing_at_eof() {
        let content = "---\nfoo: bar\n---";
        let (fm, body) = split_frontmatter(content).unwrap();
        assert_eq!(fm, "foo: bar");
        assert_eq!(body, "");
    }

    // ── Epic README parsing tests ────────────────────────────────

    #[test]
    fn test_parse_epic_readme_valid() {
        let dir = create_temp_repo();
        let readme_path = dir
            .join("docs")
            .join("epics")
            .join("support-project-management")
            .join("README.md");
        std::fs::create_dir_all(readme_path.parent().unwrap()).unwrap();
        std::fs::write(&readme_path, VALID_EPIC_README).unwrap();

        let epic = parse_epic_readme(&readme_path).unwrap();
        assert_eq!(epic.slug, "support-project-management");
        assert_eq!(epic.title, "Support Project Management");
        assert_eq!(epic.milestone, "0.2.0");
        assert_eq!(epic.status, "in_progress");
        assert!(epic.description.contains("Epic hierarchy"));
        assert_eq!(epic.started.as_deref(), Some("2026-07-08"));
        assert_eq!(epic.owner.as_deref(), Some("Suprie"));
        assert_eq!(epic.completed, None);
        assert!(epic.prds.is_empty());

        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn test_parse_epic_readme_milestone_stays_string() {
        let dir = create_temp_repo();
        let readme = "\
---
title: T
slug: t
milestone: \"0.2.0\"
status: proposed
description: d
---
# T
";
        let readme_path = dir.join("docs").join("epics").join("t").join("README.md");
        std::fs::create_dir_all(readme_path.parent().unwrap()).unwrap();
        std::fs::write(&readme_path, readme).unwrap();

        let epic = parse_epic_readme(&readme_path).unwrap();
        assert_eq!(epic.milestone, "0.2.0");

        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn test_parse_epic_readme_missing_required_field_errors() {
        let dir = create_temp_repo();
        // No `status` field — required.
        let readme = "\
---
title: T
slug: t
milestone: \"0.2.0\"
description: d
---
# T
";
        let readme_path = dir.join("docs").join("epics").join("t").join("README.md");
        std::fs::create_dir_all(readme_path.parent().unwrap()).unwrap();
        std::fs::write(&readme_path, readme).unwrap();

        let err = parse_epic_readme(&readme_path);
        assert!(err.is_err(), "missing required field should error");

        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn test_parse_epic_readme_missing_frontmatter_errors() {
        let dir = create_temp_repo();
        let readme_path = dir.join("docs").join("epics").join("t").join("README.md");
        std::fs::create_dir_all(readme_path.parent().unwrap()).unwrap();
        std::fs::write(&readme_path, "# T\n\nno frontmatter at all\n").unwrap();

        let err = parse_epic_readme(&readme_path);
        assert!(err.is_err());

        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn test_parse_epic_readme_ignores_extras() {
        let dir = create_temp_repo();
        let readme = "\
---
title: T
slug: t
milestone: \"0.2.0\"
status: proposed
description: d
custom_field: whatever
another: 123
---
# T
";
        let readme_path = dir.join("docs").join("epics").join("t").join("README.md");
        std::fs::create_dir_all(readme_path.parent().unwrap()).unwrap();
        std::fs::write(&readme_path, readme).unwrap();

        // Extra fields are ignored, not rejected.
        let epic = parse_epic_readme(&readme_path).unwrap();
        assert_eq!(epic.slug, "t");

        std::fs::remove_dir_all(&dir).unwrap();
    }

    // ── PRD parsing tests ────────────────────────────────────────

    #[test]
    fn test_parse_prd_valid_multi_phase() {
        let dir = create_temp_repo();
        let prd_path = dir
            .join("docs")
            .join("epics")
            .join("support-project-management")
            .join("prd-spec-layer.md");
        std::fs::create_dir_all(prd_path.parent().unwrap()).unwrap();
        std::fs::write(&prd_path, VALID_PRD).unwrap();

        let prd = parse_prd(&prd_path).unwrap();
        assert_eq!(prd.slug, "prd-spec-layer");
        assert_eq!(prd.epic, "support-project-management");
        assert_eq!(prd.status, "proposed");
        assert_eq!(prd.file, "prd-spec-layer.md");
        assert_eq!(prd.milestone.as_deref(), Some("0.2.0"));
        assert_eq!(prd.phases.len(), 2);

        assert_eq!(prd.phases[0].name, "Phase 1 — Core structs and parser");
        assert_eq!(prd.phases[0].loops.len(), 3);
        assert_eq!(prd.phases[0].loops[0].title, "Define Epic and Prd structs");
        assert!(!prd.phases[0].loops[0].checked);
        assert_eq!(
            prd.phases[0].loops[1].title,
            "Implement frontmatter extractor"
        );
        assert!(prd.phases[0].loops[1].checked);

        assert_eq!(prd.phases[1].name, "Phase 2 — Bootstrap and integration");
        assert_eq!(prd.phases[1].loops.len(), 2);
        assert!(!prd.phases[1].loops[0].checked);

        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn test_parse_prd_missing_phases_section_is_lenient() {
        let dir = create_temp_repo();
        let body = "\
---
prd: prd-x
epic: e
status: proposed
description: d
---

# PRD — X

## Overview

No phases section at all.
";
        let prd_path = dir.join("docs").join("epics").join("e").join("prd-x.md");
        std::fs::create_dir_all(prd_path.parent().unwrap()).unwrap();
        std::fs::write(&prd_path, body).unwrap();

        let prd = parse_prd(&prd_path).unwrap();
        assert!(prd.phases.is_empty());

        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn test_parse_prd_missing_required_field_errors() {
        let dir = create_temp_repo();
        let body = "\
---
prd: prd-x
epic: e
description: d
---
# PRD — X
";
        let prd_path = dir.join("docs").join("epics").join("e").join("prd-x.md");
        std::fs::create_dir_all(prd_path.parent().unwrap()).unwrap();
        std::fs::write(&prd_path, body).unwrap();

        assert!(parse_prd(&prd_path).is_err());

        std::fs::remove_dir_all(&dir).unwrap();
    }

    // ── parse_epics (aggregate) tests ────────────────────────────

    #[test]
    fn test_parse_epics_missing_dir_returns_empty() {
        let dir = std::env::temp_dir().join(format!("loopdeck-epic-none-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        // No docs/epics/.
        assert!(parse_epics(&dir).is_empty());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn test_parse_epics_empty_dir_returns_empty() {
        let dir = create_temp_repo();
        // docs/epics/ exists but is empty.
        assert!(parse_epics(&dir).is_empty());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn test_parse_epics_nests_prds_under_epic() {
        let dir = create_temp_repo();
        write_epic(&dir, "support-project-management", VALID_EPIC_README);
        write_prd(
            &dir,
            "support-project-management",
            "prd-spec-layer.md",
            VALID_PRD,
        );
        write_prd(
            &dir,
            "support-project-management",
            "prd-epics-view.md",
            &VALID_PRD.replace("prd-spec-layer", "prd-epics-view"),
        );

        let epics = parse_epics(&dir);
        assert_eq!(epics.len(), 1);
        let epic = &epics[0];
        assert_eq!(epic.slug, "support-project-management");
        assert_eq!(epic.prds.len(), 2);
        // Sorted by filename.
        assert_eq!(epic.prds[0].file, "prd-epics-view.md");
        assert_eq!(epic.prds[1].file, "prd-spec-layer.md");

        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn test_parse_epics_skips_malformed_and_keeps_good() {
        let dir = create_temp_repo();
        // Good epic.
        write_epic(
            &dir,
            "good-epic",
            &VALID_EPIC_README.replace("support-project-management", "good-epic"),
        );
        // Bad epic: no frontmatter.
        write_epic(&dir, "bad-epic", "# Bad\n\nno frontmatter\n");

        let epics = parse_epics(&dir);
        assert_eq!(epics.len(), 1);
        assert_eq!(epics[0].slug, "good-epic");

        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn test_parse_epics_skips_dir_without_readme() {
        let dir = create_temp_repo();
        write_epic(
            &dir,
            "has-readme",
            &VALID_EPIC_README.replace("support-project-management", "has-readme"),
        );
        // A directory with no README.md.
        std::fs::create_dir_all(dir.join("docs").join("epics").join("no-readme")).unwrap();

        let epics = parse_epics(&dir);
        assert_eq!(epics.len(), 1);
        assert_eq!(epics[0].slug, "has-readme");

        std::fs::remove_dir_all(&dir).unwrap();
    }

    // ── epics_by_milestone tests ─────────────────────────────────

    #[test]
    fn test_epics_by_milestone_groups_and_orders() {
        let dir = create_temp_repo();
        let mk = |slug: &str, milestone: &str| {
            write_epic(
                &dir,
                slug,
                &format!(
                    "\
---
title: {t}
slug: {slug}
milestone: \"{ms}\"
status: proposed
description: d
---
# {t}
",
                    t = slug,
                    ms = milestone
                ),
            );
        };
        mk("beta", "0.3.0");
        mk("alpha", "0.2.0");
        mk("gamma", "0.2.0");

        let map = epics_by_milestone(&dir);
        // BTreeMap orders keys: "0.2.0" < "0.3.0".
        let keys: Vec<&String> = map.keys().collect();
        assert_eq!(keys, vec!["0.2.0", "0.3.0"]);
        assert_eq!(map["0.2.0"].len(), 2);
        assert_eq!(map["0.3.0"].len(), 1);

        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn test_epics_by_milestone_unmilestoned_bucket() {
        let dir = create_temp_repo();
        // Empty milestone string.
        let readme = "\
---
title: T
slug: t
milestone: \"\"
status: proposed
description: d
---
# T
";
        write_epic(&dir, "t", readme);

        let map = epics_by_milestone(&dir);
        assert!(map.contains_key("Unmilestoned"));
        assert_eq!(map["Unmilestoned"].len(), 1);

        std::fs::remove_dir_all(&dir).unwrap();
    }

    // ── Dogfood: parse this repo's own epic ──────────────────────

    #[test]
    fn test_dogfood_parses_this_repos_epic() {
        // CARGO_MANIFEST_DIR = <repo>/src-tauri; the repo root is its parent.
        let repo_root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("CARGO_MANIFEST_DIR should have a parent");
        let epics = parse_epics(repo_root);

        let epic = epics
            .iter()
            .find(|e| e.slug == "support-project-management")
            .unwrap_or_else(|| {
                panic!(
                    "expected support-project-management epic, found slugs: {:?}",
                    epics.iter().map(|e| &e.slug).collect::<Vec<_>>()
                )
            });

        assert_eq!(epic.milestone, "0.2.0");
        assert_eq!(epic.prds.len(), 5, "expected the five PRDs to parse");
        for prd in &epic.prds {
            assert!(
                !prd.phases.is_empty(),
                "PRD {} should have at least one phase",
                prd.slug
            );
            assert_eq!(prd.epic, "support-project-management");
        }

        // Phase 1 dogfood — stable loop IDs parse on real repo content: the
        // 0.2.1 PRD's loops carry IDs; the 0.2.0 PRDs' items are legacy (None).
        let structured_state = epic
            .prds
            .iter()
            .find(|p| p.slug == "prd-structured-execution-state")
            .expect("0.2.1 structured-execution-state PRD should parse");
        let all_ids: Vec<&str> = structured_state
            .phases
            .iter()
            .flat_map(|ph| ph.loops.iter())
            .filter_map(|l| l.id.as_deref())
            .collect();
        assert!(
            all_ids
                .iter()
                .any(|id| *id == "structured-state/parse-loop-id"),
            "0.2.1 PRD loops should expose parsed IDs, got {all_ids:?}"
        );
        assert!(
            !all_ids.is_empty() && all_ids.iter().all(|id| !id.is_empty()),
            "ID'd loops should have non-empty IDs, got {all_ids:?}"
        );

        // 0.2.0 PRDs predate stable IDs: none of their loops carry one.
        for prd in &epic.prds {
            if prd.slug == "prd-structured-execution-state" {
                continue;
            }
            for ph in &prd.phases {
                for l in &ph.loops {
                    assert!(
                        l.id.is_none(),
                        "0.2.0 PRD {} loop should be legacy (no ID): {:?}",
                        prd.slug,
                        l.title
                    );
                }
            }
        }
    }

    // ── done_in_history enrichment tests ─────────────────────────

    #[test]
    fn test_done_in_history_marks_matching_loop() {
        let dir = create_temp_repo();
        write_epic(
            &dir,
            "e",
            &VALID_EPIC_README.replace("support-project-management", "e"),
        );
        write_prd(
            &dir,
            "e",
            "prd-x.md",
            &VALID_PRD.replace("prd-spec-layer", "prd-x"),
        );

        // Write a loops.md whose History goal matches one checklist item.
        let loops_md = "\
# Loops

## Current

_No active loop._

## Next Steps

## History

### 2026-07-01 — Implement frontmatter extractor
- **Status**: completed
- **Completed**: 2026-07-02
";
        std::fs::create_dir_all(dir.join(".loopdeck")).unwrap();
        std::fs::write(dir.join(".loopdeck").join("loops.md"), loops_md).unwrap();

        let epics = parse_epics(&dir);
        let prd = &epics[0].prds[0];
        let phase1 = &prd.phases[0];
        // "Implement frontmatter extractor" is the 2nd item in VALID_PRD Phase 1.
        assert!(
            phase1.loops[1].done_in_history,
            "matching title should be done"
        );
        assert!(
            !phase1.loops[0].done_in_history,
            "non-matching title should not be done"
        );

        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn test_done_in_history_no_history_leaves_all_false() {
        let dir = create_temp_repo();
        write_epic(
            &dir,
            "e",
            &VALID_EPIC_README.replace("support-project-management", "e"),
        );
        write_prd(
            &dir,
            "e",
            "prd-x.md",
            &VALID_PRD.replace("prd-spec-layer", "prd-x"),
        );
        // No .loopdeck/loops.md at all.
        let epics = parse_epics(&dir);
        let prd = &epics[0].prds[0];
        assert!(prd
            .phases
            .iter()
            .all(|p| p.loops.iter().all(|l| !l.done_in_history)));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    // ── promote_epic_loop tests ──────────────────────────────────

    fn write_loops_md(dir: &Path, body: &str) {
        std::fs::create_dir_all(dir.join(".loopdeck")).unwrap();
        std::fs::write(dir.join(".loopdeck").join("loops.md"), body).unwrap();
    }

    #[test]
    fn test_promote_into_empty_current_succeeds() {
        let dir = create_temp_repo();
        write_loops_md(
            &dir,
            "# Loops\n\n## Current\n\n_No active loop._\n\n## Next Steps\n- [ ] Keep this\n\n## History\n\n### 2026-06-01 — Old\n- **Status**: completed\n",
        );

        promote_epic_loop(&dir, "my-epic", "prd-spec-layer.md", "Define structs").unwrap();

        let written = std::fs::read_to_string(dir.join(".loopdeck").join("loops.md")).unwrap();
        // Back-reference bullets present.
        assert!(written.contains("- **Epic**: my-epic"));
        assert!(written.contains("- **PRD**: prd-spec-layer"));
        assert!(written.contains("- **Goal**: Define structs"));
        assert!(written.contains("- **Status**: in_progress"));
        // PRD ref is the filename without .md.
        assert!(!written.contains("prd-spec-layer.md"));
        // Rest of the file preserved.
        assert!(written.contains("## Next Steps"), "Next Steps preserved");
        assert!(
            written.contains("- [ ] Keep this"),
            "Next Steps content preserved"
        );
        assert!(written.contains("## History"), "History preserved");
        assert!(
            written.contains("### 2026-06-01 — Old"),
            "History entry preserved"
        );
        // The old "No active loop" sentinel is gone.
        assert!(!written.contains("_No active loop._"));

        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn test_promote_refuses_when_current_occupied() {
        let dir = create_temp_repo();
        write_loops_md(
            &dir,
            "# Loops\n\n## Current\n- **Started**: 2026-07-01\n- **Goal**: Existing work\n- **Status**: in_progress\n\n## Next Steps\n\n## History\n",
        );

        let err = promote_epic_loop(&dir, "epic", "prd.md", "New loop").unwrap_err();
        match err {
            AppError::Conflict(msg) => assert!(msg.contains("already in progress")),
            other => panic!("expected Conflict, got {other:?}"),
        }

        // File untouched.
        let written = std::fs::read_to_string(dir.join(".loopdeck").join("loops.md")).unwrap();
        assert!(written.contains("Existing work"));
        assert!(!written.contains("New loop"));

        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn test_promote_creates_loops_md_when_absent() {
        let dir = create_temp_repo();
        // No .loopdeck/loops.md at all.
        promote_epic_loop(&dir, "epic", "prd.md", "First loop").unwrap();

        let written = std::fs::read_to_string(dir.join(".loopdeck").join("loops.md")).unwrap();
        assert!(written.contains("- **Goal**: First loop"));
        assert!(written.contains("- **Epic**: epic"));
        assert!(written.contains("- **PRD**: prd"));

        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn test_promote_prd_ref_strips_md_suffix() {
        let dir = create_temp_repo();
        write_loops_md(
            &dir,
            "# Loops\n\n## Current\n\n_No active loop._\n\n## History\n",
        );

        promote_epic_loop(&dir, "e", "prd-epics-view.md", "Build UI").unwrap();

        let written = std::fs::read_to_string(dir.join(".loopdeck").join("loops.md")).unwrap();
        assert!(written.contains("- **PRD**: prd-epics-view\n"));
        assert!(!written.contains("prd-epics-view.md"));

        std::fs::remove_dir_all(&dir).unwrap();
    }

    // ── toggle_prd_loop tests ────────────────────────────────────

    #[test]
    fn test_toggle_prd_loop_unchecked_to_checked() {
        let dir = create_temp_repo();
        write_epic(
            &dir,
            "e",
            &VALID_EPIC_README.replace("support-project-management", "e"),
        );
        write_prd(
            &dir,
            "e",
            "prd-x.md",
            &VALID_PRD.replace("prd-spec-layer", "prd-x"),
        );

        let now_checked =
            toggle_prd_loop(&dir, "e", "prd-x.md", "Define Epic and Prd structs").unwrap();
        assert!(now_checked);

        let written =
            std::fs::read_to_string(dir.join("docs").join("epics").join("e").join("prd-x.md"))
                .unwrap();
        assert!(written.contains("- [x] Define Epic and Prd structs"));
        // Other items untouched.
        assert!(written.contains("- [ ] Write unit tests"));

        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn test_toggle_prd_loop_checked_to_unchecked() {
        let dir = create_temp_repo();
        write_epic(
            &dir,
            "e",
            &VALID_EPIC_README.replace("support-project-management", "e"),
        );
        write_prd(
            &dir,
            "e",
            "prd-x.md",
            &VALID_PRD.replace("prd-spec-layer", "prd-x"),
        );

        // "Implement frontmatter extractor" starts as `- [x]` in VALID_PRD.
        let now_checked =
            toggle_prd_loop(&dir, "e", "prd-x.md", "Implement frontmatter extractor").unwrap();
        assert!(!now_checked);

        let written =
            std::fs::read_to_string(dir.join("docs").join("epics").join("e").join("prd-x.md"))
                .unwrap();
        assert!(written.contains("- [ ] Implement frontmatter extractor"));

        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn test_toggle_prd_loop_missing_file_errors() {
        let dir = create_temp_repo();
        let err = toggle_prd_loop(&dir, "nope", "missing.md", "anything");
        assert!(err.is_err());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn test_toggle_prd_loop_item_not_found_errors() {
        let dir = create_temp_repo();
        write_epic(
            &dir,
            "e",
            &VALID_EPIC_README.replace("support-project-management", "e"),
        );
        write_prd(
            &dir,
            "e",
            "prd-x.md",
            &VALID_PRD.replace("prd-spec-layer", "prd-x"),
        );

        let err = toggle_prd_loop(&dir, "e", "prd-x.md", "nonexistent loop");
        assert!(err.is_err());

        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn test_toggle_prd_loop_rejects_traversal_in_slug() {
        // PRD FR3: `epic_slug` is joined into a filesystem path, so a `../`
        // in it must not escape `docs/epics/` and touch a file outside the
        // sandbox. Seed a real file outside docs/epics/ to prove the toggle
        // does NOT reach it.
        let dir = create_temp_repo();
        std::fs::write(dir.join("secret.md"), "- [ ] pwned\n").unwrap();

        let err = toggle_prd_loop(&dir, "../../.", "secret.md", "pwned");
        assert!(
            err.is_err(),
            "traversal in epic_slug must be rejected, not followed"
        );
        // The out-of-sandbox file is untouched.
        assert_eq!(
            std::fs::read_to_string(dir.join("secret.md")).unwrap(),
            "- [ ] pwned\n"
        );

        std::fs::remove_dir_all(&dir).unwrap();
    }

    // ── read_spec_file / write_spec_file tests ───────────────────

    #[test]
    fn test_read_spec_file_returns_raw_content() {
        let dir = create_temp_repo();
        write_epic(
            &dir,
            "e",
            &VALID_EPIC_README.replace("support-project-management", "e"),
        );

        let content = read_spec_file(&dir, "e/README.md").unwrap();
        assert!(content.starts_with("---"));
        assert!(content.contains("title: Support Project Management"));
    }

    #[test]
    fn test_write_spec_file_round_trip() {
        let dir = create_temp_repo();
        let new_content = "---\nprd: prd-new\nepic: e\nstatus: proposed\ndescription: d\n---\n\n# PRD — New\n\n## Phases\n\n### Phase 1 — Do it\n- [ ] Task\n";

        write_spec_file(&dir, "e/prd-new.md", new_content).unwrap();

        let read_back = read_spec_file(&dir, "e/prd-new.md").unwrap();
        assert_eq!(read_back, new_content);

        // And it parses cleanly as a PRD.
        let prd_path = dir.join("docs").join("epics").join("e").join("prd-new.md");
        let prd = parse_prd(&prd_path).unwrap();
        assert_eq!(prd.slug, "prd-new");
        assert_eq!(prd.phases.len(), 1);
    }

    #[test]
    fn test_read_spec_file_missing_errors() {
        let dir = create_temp_repo();
        assert!(read_spec_file(&dir, "nope/missing.md").is_err());
    }

    #[test]
    fn test_write_spec_file_rejects_non_md() {
        let dir = create_temp_repo();
        let err = write_spec_file(&dir, "e/script.sh", "#!/bin/sh");
        assert!(err.is_err());
    }

    #[test]
    fn test_read_spec_file_rejects_traversal() {
        let dir = create_temp_repo();
        // Create a file outside docs/epics/ to attempt reading.
        std::fs::write(dir.join("secret.md"), "secret").unwrap();
        // `../secret.md` resolves outside docs/epics/.
        let err = read_spec_file(&dir, "../secret.md");
        assert!(err.is_err());
    }

    #[test]
    fn test_write_spec_file_rejects_traversal() {
        let dir = create_temp_repo();
        let err = write_spec_file(&dir, "../../etc/evil.md", "pwned");
        assert!(err.is_err());
    }

    // ── Stable loop-ID parsing tests ───────────────────────────────

    #[test]
    fn test_is_valid_loop_id_shape() {
        for valid in ["a/b", "epics-view/promote-loop", "ns1/slug-2", "x/y"] {
            assert!(is_valid_loop_id_shape(valid), "{valid} should be valid");
        }
        for invalid in [
            "nodashnoslash", // no '/'
            "/slug",         // empty namespace
            "ns/",           // empty slug
            "a/b/c",         // two slashes
            "Upper/lower",   // uppercase
            "ns/slu_g",      // underscore
            "ns/slu.g",      // dot
            "",
        ] {
            assert!(
                !is_valid_loop_id_shape(invalid),
                "{invalid} should be invalid"
            );
        }
    }

    #[test]
    fn test_parse_phase_loops_extracts_well_formed_id() {
        let loops = parse_phase_loops(
            "- [ ] `epics-view/promote-loop` Add the Promote button\n\
             - [x] `epics-view/clobber-guard` Prevent replacing an active loop\n\
             - [ ] Legacy item with no id\n"
                .lines(),
        );
        assert_eq!(loops.len(), 3);
        assert_eq!(loops[0].id.as_deref(), Some("epics-view/promote-loop"));
        assert_eq!(loops[0].title, "Add the Promote button");
        assert!(!loops[0].checked);
        assert_eq!(loops[1].id.as_deref(), Some("epics-view/clobber-guard"));
        assert_eq!(loops[1].title, "Prevent replacing an active loop");
        assert!(loops[1].checked);
        assert!(loops[2].id.is_none());
        assert_eq!(loops[2].title, "Legacy item with no id");
    }

    #[test]
    fn test_parse_phase_loops_malformed_id_falls_back_to_legacy() {
        // Uppercase + underscore aren't valid ID bytes → the token stays in the
        // title and the item parses as ID-less (validate_loop_ids flags it).
        let loops = parse_phase_loops("- [ ] `Bad_ID/x` Do thing".lines());
        assert_eq!(loops.len(), 1);
        assert!(loops[0].id.is_none());
        assert_eq!(loops[0].title, "`Bad_ID/x` Do thing");
    }

    // ── validate_loop_ids tests ────────────────────────────────────

    #[test]
    fn test_validate_loop_ids_clean() {
        let dir = create_temp_repo();
        write_prd(
            &dir,
            "e",
            "prd-a.md",
            "---\nprd: prd-a\nepic: e\nstatus: proposed\ndescription: >\n  d\n---\n\n\
             ## Phases\n\n### Phase 1 — P\n\
             - [ ] `prd-a/one` One\n\
             - [x] `prd-a/two` Two\n",
        );
        let diags = validate_loop_ids(&dir);
        assert!(diags.is_empty(), "expected no diagnostics, got {diags:?}");
    }

    #[test]
    fn test_validate_loop_ids_flags_malformed_with_file_and_line() {
        let dir = create_temp_repo();
        write_prd(
            &dir,
            "e",
            "prd-a.md",
            "---\nprd: prd-a\nepic: e\nstatus: proposed\ndescription: >\n  d\n---\n\n\
             ## Phases\n\n### Phase 1 — P\n\
             - [ ] `Bad_ID/x` One\n\
             - [ ] `prd-a/two` Two\n",
        );
        let diags = validate_loop_ids(&dir);
        assert_eq!(diags.len(), 1, "got {diags:?}");
        assert_eq!(diags[0].kind, LoopIdDiagKind::Malformed);
        assert_eq!(diags[0].id, "Bad_ID/x");
        assert_eq!(diags[0].file, "e/prd-a.md");
        assert_eq!(diags[0].line, 12);
    }

    #[test]
    fn test_validate_loop_ids_flags_duplicate_across_prds() {
        let dir = create_temp_repo();
        for filename in ["prd-a.md", "prd-b.md"] {
            write_prd(
                &dir,
                "e",
                filename,
                "---\nprd: x\nepic: e\nstatus: proposed\ndescription: >\n  d\n---\n\n\
                 ## Phases\n\n### Phase 1 — P\n- [ ] `shared/loop` Do it\n",
            );
        }
        let diags = validate_loop_ids(&dir);
        let dups: Vec<_> = diags
            .iter()
            .filter(|d| d.kind == LoopIdDiagKind::Duplicate)
            .collect();
        assert_eq!(dups.len(), 2, "both occurrences should be reported");
        let files: Vec<_> = dups.iter().map(|d| d.file.as_str()).collect();
        assert!(files.contains(&"e/prd-a.md"));
        assert!(files.contains(&"e/prd-b.md"));
        assert!(dups.iter().all(|d| d.id == "shared/loop"));
    }

    #[test]
    fn test_validate_loop_ids_ignores_prose_code_identifier() {
        // A backtick token with no `/` is an ordinary code identifier in prose
        // (`get_epics`, `Bash`, `EpicsView.tsx`), not an ID attempt — 0.2.0 PRDs
        // legitimately write items like `` - [ ] `get_epics` enriches … ``.
        let dir = create_temp_repo();
        write_prd(
            &dir,
            "e",
            "prd-a.md",
            "---\nprd: prd-a\nepic: e\nstatus: proposed\ndescription: >\n  d\n---\n\n\
             ## Phases\n\n### Phase 1 — P\n\
             - [ ] `get_epics` enriches each item\n\
             - [ ] `EpicsView.tsx` renders the view\n",
        );
        let diags = validate_loop_ids(&dir);
        assert!(
            diags.is_empty(),
            "prose code identifiers must not be flagged, got {diags:?}"
        );
    }

    #[test]
    fn test_validate_loop_ids_ignores_legacy_and_fenced_examples() {
        let dir = create_temp_repo();
        write_prd(
            &dir,
            "e",
            "prd-a.md",
            "---\nprd: prd-a\nepic: e\nstatus: proposed\ndescription: >\n  d\n---\n\n\
             ## Authoring format\n\n\
             ```markdown\n\
             - [ ] `fenced/example` not a real loop\n\
             ```\n\n\
             ## Phases\n\n### Phase 1 — P\n\
             - [ ] Legacy item with no id\n\
             - [ ] `prd-a/real` Real\n",
        );
        let diags = validate_loop_ids(&dir);
        assert!(
            diags.is_empty(),
            "fenced example + legacy item must be ignored, got {diags:?}"
        );
    }
}
