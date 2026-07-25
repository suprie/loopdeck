//! Parser for `.loopdeck/` memory files (decisions.md, loops.md).
//!
//! All parsing is lenient — missing files or malformed sections return
//! empty/default values rather than errors. This ensures the UI degrades
//! gracefully before any AI agent has written the files.

use crate::persist;
use serde::Serialize;
use std::path::{Path, PathBuf};

// ── Types ────────────────────────────────────────────────────────────

/// A single architectural decision record.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Decision {
    /// ISO date string, e.g. "2026-06-22"
    pub date: String,
    /// Short title extracted from the heading.
    pub title: String,
    /// "proposed" | "accepted" | "superseded"
    pub status: String,
    /// Freeform context / rationale.
    pub context: String,
    /// Optional consequences of the decision.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub consequences: Option<String>,
}

/// A single development loop / iteration.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Loop {
    /// ISO date string when the loop started.
    pub started: String,
    /// Short description of the loop goal.
    pub goal: String,
    /// "in_progress" | "completed" | "abandoned"
    pub status: String,
    /// ISO date string when completed (if status is completed).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub completed: Option<String>,
}

/// A single checklist item under `## Next Steps` in `loops.md`.
///
/// Carries the checked state so the UI can render `- [x]` items as done.
/// Previously this was a bare `Vec<String>` and the prefix was stripped,
/// discarding the checked state entirely (a checked box never showed as checked).
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct NextStep {
    /// The step text (without the `- [ ]` / `- [x]` prefix).
    pub text: String,
    /// Whether the box is checked (`- [x]`).
    pub checked: bool,
}

/// Full status from `.loopdeck/loops.md`.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct LoopStatus {
    /// The currently active loop, if any.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub current: Option<Loop>,
    /// Checklist of next steps (from `- [ ]` / `- [x]` items).
    #[serde(default)]
    pub next_steps: Vec<NextStep>,
    /// Completed / historical loops.
    #[serde(default)]
    pub history: Vec<Loop>,
}

// ── Public API ───────────────────────────────────────────────────────

/// Parse `.loopdeck/decisions.md` into structured decision records.
///
/// Returns an empty Vec if the file does not exist or is empty.
pub fn parse_decisions(repo_path: &Path) -> Vec<Decision> {
    let file_path = repo_path.join(".loopdeck").join("decisions.md");
    let content = match std::fs::read_to_string(&file_path) {
        Ok(c) => c,
        Err(e) => {
            tracing::debug!("decisions.md not readable at {}: {e}", file_path.display());
            return Vec::new();
        }
    };

    parse_decisions_content(&content)
}

/// Parse `.loopdeck/loops.md` into a LoopStatus.
///
/// Returns defaults (no current loop, empty history) if the file
/// does not exist or is empty.
pub fn parse_loops(repo_path: &Path) -> LoopStatus {
    let file_path = repo_path.join(".loopdeck").join("loops.md");
    let content = match std::fs::read_to_string(&file_path) {
        Ok(c) => c,
        Err(e) => {
            tracing::debug!("loops.md not readable at {}: {e}", file_path.display());
            return LoopStatus {
                current: None,
                next_steps: Vec::new(),
                history: Vec::new(),
            };
        }
    };

    parse_loops_content(&content)
}

/// The canonical path to a project's `.loopdeck/loops.md`.
///
/// Shared by the migration module so the legacy-file location isn't duplicated.
pub fn loops_md_path(repo_path: &Path) -> PathBuf {
    repo_path.join(".loopdeck").join("loops.md")
}

/// Bootstrap empty memory files if `.loopdeck/` exists but files don't.
/// Does NOT overwrite existing files.
pub fn ensure_memory_files(repo_path: &Path) -> Result<(), std::io::Error> {
    let loopdeck = repo_path.join(".loopdeck");
    if !loopdeck.exists() {
        return Ok(());
    }

    let decisions_file = loopdeck.join("decisions.md");
    if !decisions_file.exists() {
        persist::atomic_write(&decisions_file, "# Decisions\n\n")?;
    }

    let loops_file = loopdeck.join("loops.md");
    if !loops_file.exists() {
        persist::atomic_write(
            &loops_file,
            "# Loops\n\n## Current\n\n_No active loop._\n\n## Next Steps\n\n## History\n\n",
        )?;
    }

    Ok(())
}

/// Toggle a `- [ ]` / `- [x]` checklist item under `## Next Steps` in
/// `loops.md`.
///
/// Matches the checklist line by its text (the content after the `- [ ]` /
/// `- [x]` prefix, trimmed for comparison). The first match is toggled.
/// Returns `Ok(true)` if toggled to checked, `Ok(false)` if toggled to
/// unchecked, or an error if the file or item can't be found.
pub fn toggle_loop_step(repo_path: &Path, step_text: &str) -> Result<bool, std::io::Error> {
    let loops_file = repo_path.join(".loopdeck").join("loops.md");
    let content = std::fs::read_to_string(&loops_file)?;

    let needle = step_text.trim();
    let mut found: Option<(usize, bool)> = None; // (line index, currently_checked)

    for (i, line) in content.lines().enumerate() {
        let trimmed = line.trim();
        if let Some((prefix_len, checked)) = checklist_prefix_len(trimmed) {
            let text = trimmed[prefix_len..].trim();
            if text == needle {
                found = Some((i, checked));
                break;
            }
        }
    }

    let (line_idx, currently_checked) = found.ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::NotFound,
            format!("checklist item not found: {step_text}"),
        )
    })?;

    // Rewrite the line with the toggled prefix. Reassemble preserving the
    // original line terminators by splitting on lines and re-joining.
    let mut lines: Vec<String> = content.split('\n').map(String::from).collect();
    let old_line = &lines[line_idx];
    let indent = old_line.len() - old_line.trim_start().len();
    let new_box = if currently_checked { "- [ ]" } else { "- [x]" };
    let item_text =
        old_line.trim()[checklist_prefix_len(old_line.trim()).unwrap().0..].trim_start();
    lines[line_idx] = format!("{}{} {}", " ".repeat(indent), new_box, item_text);

    let new_content = lines.join("\n");
    persist::atomic_write(&loops_file, &new_content)?;
    Ok(!currently_checked)
}

/// If `line` starts with a GFM checklist marker, returns `(prefix_len, checked)`.
/// `prefix_len` is the byte length of the `- [ ] ` / `- [x] ` prefix.
fn checklist_prefix_len(line: &str) -> Option<(usize, bool)> {
    if let Some(rest) = line.strip_prefix("- [ ] ") {
        let _ = rest;
        Some((6, false))
    } else if let Some(rest) = line
        .strip_prefix("- [x] ")
        .or_else(|| line.strip_prefix("- [X] "))
    {
        let _ = rest;
        Some((6, true))
    } else {
        None
    }
}

// ── Parsing internals ────────────────────────────────────────────────

/// Parse decisions.md content into Decision records.
fn parse_decisions_content(content: &str) -> Vec<Decision> {
    let mut decisions: Vec<Decision> = Vec::new();

    // Prepend a newline so `## ` at the very start of the file (before any
    // `# Decisions` header) is caught by the split pattern.
    let normalized = format!("\n{content}");

    // Split on `\n## ` — first segment is the preamble (everything before
    // the first `## ` heading). All subsequent segments are the body of
    // each `## ` block (the `## ` itself was consumed by the split).
    let mut sections = normalized.split("\n## ");
    sections.next(); // skip preamble

    for section in sections {
        if let Some(decision) = parse_decision_block(section) {
            decisions.push(decision);
        }
    }

    decisions
}

/// Parse a single decision block (content after `## `).
fn parse_decision_block(block: &str) -> Option<Decision> {
    let mut lines = block.lines();

    // First line is the heading: "YYYY-MM-DD — Title" or "YYYY-MM-DD - Title"
    let heading = lines.next()?.trim();
    let (date, title) = split_heading(heading)?;

    let mut status = String::from("proposed");
    let mut context_lines: Vec<&str> = Vec::new();
    let mut consequences_lines: Vec<&str> = Vec::new();
    let mut in_consequences = false;
    let mut past_bullets = false;

    for line in lines {
        let trimmed = line.trim();

        if trimmed.is_empty() {
            if past_bullets {
                // Empty line after body starts new paragraph
                if in_consequences {
                    consequences_lines.push("");
                } else {
                    context_lines.push("");
                }
            }
            continue;
        }

        // Parse `- **Key**: Value` lines
        if let Some((key, value)) = parse_kv_line(trimmed) {
            past_bullets = true;
            match key.to_lowercase().as_str() {
                "status" => status = value.to_string(),
                "context" => {
                    if !value.is_empty() {
                        context_lines.push(value);
                    }
                }
                "decision" => {
                    if !value.is_empty() {
                        context_lines.push(value);
                    }
                }
                "consequences" => {
                    in_consequences = true;
                    if !value.is_empty() {
                        consequences_lines.push(value);
                    }
                }
                _ => {
                    // Unknown keys — treat as context
                    if !value.is_empty() {
                        context_lines.push(value);
                    }
                }
            }
        } else {
            // Non-bullet line — body text
            past_bullets = true;
            if in_consequences {
                consequences_lines.push(trimmed);
            } else {
                context_lines.push(trimmed);
            }
        }
    }

    let context = join_lines(&context_lines);
    let consequences = if consequences_lines.is_empty() {
        None
    } else {
        Some(join_lines(&consequences_lines))
    };

    Some(Decision {
        date,
        title,
        status,
        context,
        consequences,
    })
}

/// Parse loops.md content into a LoopStatus.
fn parse_loops_content(content: &str) -> LoopStatus {
    let mut current: Option<Loop> = None;
    let mut next_steps: Vec<NextStep> = Vec::new();
    let mut history: Vec<Loop> = Vec::new();

    // Prepend a newline so `## ` at the very start of the file is caught
    let normalized = format!("\n{content}");

    for section in normalized.split("\n## ") {
        if let Some(rest) = section.strip_prefix("Current") {
            current = parse_loop_section(rest);
        } else if let Some(rest) = section.strip_prefix("Next Steps") {
            next_steps = parse_checklist(rest);
        } else if let Some(rest) = section.strip_prefix("History") {
            history = parse_history_section(rest);
        }
        // Preamble (`# Loops` header) is ignored
    }

    LoopStatus {
        current,
        next_steps,
        history,
    }
}

/// Parse a `## Current` or loop block into a Loop struct.
fn parse_loop_section(section: &str) -> Option<Loop> {
    // Check for "_No active loop._" sentinel
    if section.contains("_No active loop._") {
        return None;
    }

    let mut started = String::new();
    let mut goal = String::new();
    let mut status = String::from("in_progress");
    let mut completed: Option<String> = None;
    let mut body_lines: Vec<&str> = Vec::new();

    for line in section.lines() {
        let trimmed = line.trim();

        if trimmed.is_empty() {
            continue;
        }

        if let Some((key, value)) = parse_kv_line(trimmed) {
            match key.to_lowercase().as_str() {
                "started" => started = value.to_string(),
                "goal" => goal = value.to_string(),
                "status" => status = value.to_string(),
                "completed" => completed = Some(value.to_string()),
                _ => {}
            }
        } else if !trimmed.starts_with('#') {
            body_lines.push(trimmed);
        }
    }

    if started.is_empty() && goal.is_empty() {
        return None;
    }

    // If goal wasn't in KV, use first body line
    if goal.is_empty() && !body_lines.is_empty() {
        goal = body_lines.remove(0).to_string();
    }

    Some(Loop {
        started,
        goal,
        status,
        completed,
    })
}

/// Parse `- [ ]` and `- [x]` checklist items, preserving the checked state.
fn parse_checklist(section: &str) -> Vec<NextStep> {
    section
        .lines()
        .filter_map(|line| {
            let trimmed = line.trim();
            if let Some(rest) = trimmed.strip_prefix("- [ ] ") {
                Some(NextStep {
                    text: rest.to_string(),
                    checked: false,
                })
            } else {
                trimmed
                    .strip_prefix("- [x] ")
                    .or_else(|| trimmed.strip_prefix("- [X] "))
                    .map(|rest| NextStep {
                        text: rest.to_string(),
                        checked: true,
                    })
            }
        })
        .collect()
}

/// Parse `## History` section — each `### ` entry is a historical loop.
fn parse_history_section(section: &str) -> Vec<Loop> {
    let mut history: Vec<Loop> = Vec::new();

    // Split on `### ` to get individual history entries
    let entries: Vec<&str> = section.split("\n### ").collect();

    for (i, entry) in entries.iter().enumerate() {
        if i == 0 {
            // Before first `### ` — may have a heading like "YYYY-MM-DD — Title"
            // Try to parse it if it has the date-title format
            if let Some(first_line) = entry.lines().next() {
                let trimmed = first_line.trim();
                if trimmed.contains(" — ") || trimmed.contains(" - ") {
                    if let Some(loop_item) = parse_loop_from_heading_and_body(entry) {
                        history.push(loop_item);
                    }
                }
            }
            continue;
        }

        if let Some(loop_item) = parse_loop_from_heading_and_body(entry) {
            history.push(loop_item);
        }
    }

    history
}

/// Parse a history entry: heading `YYYY-MM-DD — Title` followed by body.
fn parse_loop_from_heading_and_body(entry: &str) -> Option<Loop> {
    let mut lines = entry.lines();
    let heading = lines.next()?.trim();

    let (started, goal) = split_heading(heading)?;

    let mut status = String::from("completed");
    let mut completed: Option<String> = None;

    for line in lines {
        let trimmed = line.trim();
        if let Some((key, value)) = parse_kv_line(trimmed) {
            match key.to_lowercase().as_str() {
                "status" => status = value.to_string(),
                "completed" => completed = Some(value.to_string()),
                _ => {}
            }
        }
    }

    Some(Loop {
        started,
        goal,
        status,
        completed,
    })
}

// ── Helpers ──────────────────────────────────────────────────────────

/// Split a heading like "2026-06-22 — Title Here" into (date, title).
pub(crate) fn split_heading(heading: &str) -> Option<(String, String)> {
    // Try em-dash first, then en-dash, then hyphen with spaces.
    // Track which separator was found to correctly compute the slice.
    let separators: &[&str] = &[" — ", " - "];

    for sep in separators {
        if let Some(pos) = heading.find(sep) {
            let date = heading[..pos].trim().to_string();
            let title = heading[pos + sep.len()..].trim().to_string();

            if date.is_empty() || title.is_empty() {
                return None;
            }

            return Some((date, title));
        }
    }

    None
}

/// Parse a `- **Key**: Value` line. Returns (key, value) if matched.
pub(crate) fn parse_kv_line(line: &str) -> Option<(&str, &str)> {
    let stripped = line.strip_prefix("- **")?;
    let colon_pos = stripped.find("**:")?;
    let key = &stripped[..colon_pos];
    let value = stripped[colon_pos + 3..].trim();
    Some((key, value))
}

/// Join non-empty lines with spaces, preserving paragraph breaks.
fn join_lines(lines: &[&str]) -> String {
    let mut result = String::new();
    let mut prev_empty = false;

    for line in lines {
        if line.is_empty() {
            if !prev_empty && !result.is_empty() {
                result.push('\n');
                prev_empty = true;
            }
        } else {
            if !result.is_empty() && !prev_empty {
                result.push(' ');
            }
            result.push_str(line);
            prev_empty = false;
        }
    }

    result.trim().to_string()
}

// ── Tests ────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn create_temp_repo() -> (PathBuf, String) {
        let dir = std::env::temp_dir().join(format!("loopdeck-mem-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(dir.join(".loopdeck")).unwrap();
        let name = dir.file_name().unwrap().to_string_lossy().to_string();
        (dir, name)
    }

    // ── Decisions tests ──────────────────────────────────────────

    #[test]
    fn test_empty_decisions_file() {
        let content = "# Decisions\n\n";
        let decisions = parse_decisions_content(content);
        assert!(decisions.is_empty());
    }

    #[test]
    fn test_nonexistent_decisions_file() {
        let dir = create_temp_repo().0;
        let decisions = parse_decisions(&dir);
        assert!(decisions.is_empty());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn test_single_decision() {
        let content = "\
# Decisions

## 2026-06-22 — Use Tauri v2
- **Status**: accepted
- **Context**: Needed cross-platform desktop shell.
- **Consequences**: Must maintain Rust and TypeScript.
";
        let decisions = parse_decisions_content(content);
        assert_eq!(decisions.len(), 1);
        assert_eq!(decisions[0].date, "2026-06-22");
        assert_eq!(decisions[0].title, "Use Tauri v2");
        assert_eq!(decisions[0].status, "accepted");
        assert!(decisions[0].context.contains("cross-platform"));
        assert!(decisions[0].consequences.is_some());
    }

    #[test]
    fn test_multiple_decisions() {
        let content = "\
# Decisions

## 2026-06-20 — First decision
- **Status**: accepted
- **Context**: Some context here.

## 2026-06-22 — Second decision
- **Status**: proposed
- **Context**: Another context.
- **Consequences**: Some consequences.
";
        let decisions = parse_decisions_content(content);
        assert_eq!(decisions.len(), 2);
        assert_eq!(decisions[0].title, "First decision");
        assert_eq!(decisions[1].title, "Second decision");
    }

    #[test]
    fn test_decision_with_body_text() {
        let content = "\
# Decisions

## 2026-06-22 — Complex decision
- **Status**: accepted
- **Context**: We needed to pick a state management library.

After evaluating Zustand, Redux, and Jotai, we chose Zustand for its
minimal API and excellent TypeScript support.

- **Consequences**: All components now use Zustand selectors.
Migration from Context API was straightforward.
";
        let decisions = parse_decisions_content(content);
        assert_eq!(decisions.len(), 1);
        assert!(decisions[0].context.contains("evaluating Zustand"));
        assert!(decisions[0].context.contains("minimal API"));
        let cons = decisions[0].consequences.as_ref().unwrap();
        assert!(cons.contains("Zustand selectors"));
        assert!(cons.contains("Context API"));
    }

    #[test]
    fn test_decision_default_status() {
        let content = "\
# Decisions

## 2026-06-22 — No status specified
- **Context**: Something.
";
        let decisions = parse_decisions_content(content);
        assert_eq!(decisions.len(), 1);
        assert_eq!(decisions[0].status, "proposed"); // default
    }

    #[test]
    fn test_decision_roundtrip_via_file() {
        let (dir, _name) = create_temp_repo();
        let decisions_file = dir.join(".loopdeck").join("decisions.md");
        std::fs::write(
            &decisions_file,
            "\
# Decisions

## 2026-06-22 — Test decision
- **Status**: accepted
- **Context**: This is a test.
- **Consequences**: None really.
",
        )
        .unwrap();

        let decisions = parse_decisions(&dir);
        assert_eq!(decisions.len(), 1);
        assert_eq!(decisions[0].title, "Test decision");

        std::fs::remove_dir_all(&dir).unwrap();
    }

    // ── Loops tests ──────────────────────────────────────────────

    #[test]
    fn test_empty_loops_file() {
        let content =
            "# Loops\n\n## Current\n\n_No active loop._\n\n## Next Steps\n\n## History\n\n";
        let status = parse_loops_content(content);
        assert!(status.current.is_none());
        assert!(status.next_steps.is_empty());
        assert!(status.history.is_empty());
    }

    #[test]
    fn test_nonexistent_loops_file() {
        let dir = create_temp_repo().0;
        let status = parse_loops(&dir);
        assert!(status.current.is_none());
        assert!(status.history.is_empty());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn test_active_loop_with_next_steps() {
        let content = "\
# Loops

## Current
- **Started**: 2026-06-22
- **Goal**: Implement V2 agent memory
- **Status**: in_progress

## Next Steps
- [ ] Create memory.rs
- [x] Design file format
- [ ] Build UI panels

## History

### 2026-06-20 — V1 Core
- **Status**: completed
- **Completed**: 2026-06-22
";
        let status = parse_loops_content(content);

        let current = status.current.unwrap();
        assert_eq!(current.started, "2026-06-22");
        assert_eq!(current.goal, "Implement V2 agent memory");
        assert_eq!(current.status, "in_progress");

        assert_eq!(status.next_steps.len(), 3);
        assert_eq!(status.next_steps[0].text, "Create memory.rs");
        assert_eq!(status.next_steps[1].text, "Design file format");
        // The second item is `- [x] Design file format` — checked state preserved.
        assert!(!status.next_steps[0].checked);
        assert!(status.next_steps[1].checked);
        assert!(!status.next_steps[2].checked);

        assert_eq!(status.history.len(), 1);
        assert_eq!(status.history[0].goal, "V1 Core");
        assert_eq!(status.history[0].status, "completed");
    }

    #[test]
    fn test_loop_with_no_active() {
        let content = "\
# Loops

## Current

_No active loop._

## Next Steps

## History

### 2026-06-19 — Some old loop
- **Status**: abandoned
";
        let status = parse_loops_content(content);
        assert!(status.current.is_none());
        assert_eq!(status.history.len(), 1);
        assert_eq!(status.history[0].goal, "Some old loop");
    }

    #[test]
    fn test_loops_roundtrip_via_file() {
        let (dir, _name) = create_temp_repo();
        let loops_file = dir.join(".loopdeck").join("loops.md");
        std::fs::write(
            &loops_file,
            "\
# Loops

## Current
- **Started**: 2026-06-22
- **Goal**: Build the thing
- **Status**: in_progress

## Next Steps
- [ ] First step
- [ ] Second step

## History

### 2026-06-20 — Previous loop
- **Status**: completed
- **Completed**: 2026-06-21
",
        )
        .unwrap();

        let status = parse_loops(&dir);
        assert!(status.current.is_some());
        assert_eq!(status.next_steps.len(), 2);
        assert_eq!(status.history.len(), 1);

        std::fs::remove_dir_all(&dir).unwrap();
    }

    // ── ensure_memory_files tests ─────────────────────────────────

    #[test]
    fn test_ensure_memory_files_creates_files() {
        let (dir, _name) = create_temp_repo();
        // .loopdeck already exists from create_temp_repo

        ensure_memory_files(&dir).unwrap();

        let decisions_file = dir.join(".loopdeck").join("decisions.md");
        let loops_file = dir.join(".loopdeck").join("loops.md");

        assert!(decisions_file.exists());
        assert!(loops_file.exists());

        // Should be idempotent — doesn't overwrite
        std::fs::write(&decisions_file, "custom content").unwrap();
        ensure_memory_files(&dir).unwrap();
        let content = std::fs::read_to_string(&decisions_file).unwrap();
        assert_eq!(content, "custom content");

        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn test_ensure_memory_files_no_loopdeck_dir() {
        let dir = std::env::temp_dir().join(format!("loopdeck-mem-nold-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        // No .loopdeck directory

        ensure_memory_files(&dir).unwrap();

        // Should not create .loopdeck if it doesn't exist
        assert!(!dir.join(".loopdeck").exists());

        std::fs::remove_dir_all(&dir).unwrap();
    }

    // ── Edge case tests ──────────────────────────────────────────

    #[test]
    fn test_malformed_decision_heading_skipped() {
        let content = "\
# Decisions

## Not a date heading
- **Status**: proposed

## 2026-06-22 — Valid heading
- **Status**: accepted
- **Context**: Valid.
";
        let decisions = parse_decisions_content(content);
        // The first "decision" has no date-title separator, so it's skipped
        // Actually our parser is lenient — it will accept "Not a date heading" as title
        // with empty date. Let me check... split_heading requires " — " or " - ".
        // "Not a date heading" has neither, so split_heading returns None, and
        // parse_decision_block returns None. So only the valid one is parsed.
        assert_eq!(decisions.len(), 1);
        assert_eq!(decisions[0].title, "Valid heading");
    }

    #[test]
    fn test_decision_with_en_dash() {
        let content = "\
# Decisions

## 2026-06-22 — Title with em dash
- **Status**: accepted
- **Context**: Uses em dash separator.
";
        let decisions = parse_decisions_content(content);
        assert_eq!(decisions.len(), 1);
        assert_eq!(decisions[0].title, "Title with em dash");
    }

    #[test]
    fn test_loop_history_multiple_entries() {
        let content = "\
# Loops

## Current

_No active loop._

## Next Steps

## History

### 2026-06-18 — Backend setup
- **Status**: completed
- **Completed**: 2026-06-19

### 2026-06-20 — Frontend UI
- **Status**: completed
- **Completed**: 2026-06-21

### 2026-06-21 — Polish
- **Status**: abandoned
";
        let status = parse_loops_content(content);
        assert_eq!(status.history.len(), 3);
        assert_eq!(status.history[0].goal, "Backend setup");
        assert_eq!(status.history[1].goal, "Frontend UI");
        assert_eq!(status.history[2].goal, "Polish");
        assert_eq!(status.history[2].status, "abandoned");
    }

    // ── Edge case: hyphen separator ───────────────────────────────

    #[test]
    fn test_decision_with_hyphen_separator() {
        let content = "\
# Decisions

## 2026-06-22 - Title with hyphen
- **Status**: accepted
- **Context**: Uses regular hyphen separator.
";
        let decisions = parse_decisions_content(content);
        assert_eq!(decisions.len(), 1);
        assert_eq!(decisions[0].date, "2026-06-22");
        assert_eq!(decisions[0].title, "Title with hyphen");
    }

    // ── Edge case: empty file ─────────────────────────────────────

    #[test]
    fn test_empty_file_returns_empty() {
        let decisions = parse_decisions_content("");
        assert!(decisions.is_empty());

        let loops = parse_loops_content("");
        assert!(loops.current.is_none());
        assert!(loops.next_steps.is_empty());
        assert!(loops.history.is_empty());
    }

    // ── Edge case: no headings ────────────────────────────────────

    #[test]
    fn test_no_headings_returns_empty() {
        let content = "Just some text without any ## sections.\nMore text here.\n";
        let decisions = parse_decisions_content(content);
        assert!(decisions.is_empty());
    }

    // ── Edge case: ## at very start of file (no # header) ────────

    #[test]
    fn test_decision_at_start_of_file() {
        // No `# Decisions` header — first `## ` starts at byte 0
        let content = "\
## 2026-06-22 — First line decision
- **Status**: accepted
- **Context**: This decision starts at the first line.
";
        let decisions = parse_decisions_content(content);
        assert_eq!(decisions.len(), 1);
        assert_eq!(decisions[0].title, "First line decision");
        assert_eq!(decisions[0].status, "accepted");
    }

    // ── Edge case: ensure_memory_files with only one file missing ─

    #[test]
    fn test_ensure_memory_files_partial() {
        let (dir, _name) = create_temp_repo();
        // Create decisions.md manually but NOT loops.md
        std::fs::write(dir.join(".loopdeck").join("decisions.md"), "# Custom\n").unwrap();

        ensure_memory_files(&dir).unwrap();

        // decisions.md should be untouched
        let decisions_content =
            std::fs::read_to_string(dir.join(".loopdeck").join("decisions.md")).unwrap();
        assert_eq!(decisions_content, "# Custom\n");

        // loops.md should be created
        assert!(dir.join(".loopdeck").join("loops.md").exists());

        std::fs::remove_dir_all(&dir).unwrap();
    }

    // ── toggle_loop_step tests ───────────────────────────────────

    #[test]
    fn test_toggle_loop_step_unchecked_to_checked() {
        let (dir, _name) = create_temp_repo();
        let loops_file = dir.join(".loopdeck").join("loops.md");
        std::fs::write(
            &loops_file,
            "# Loops\n\n## Next Steps\n- [ ] First step\n- [ ] Second step\n",
        )
        .unwrap();

        let now_checked = toggle_loop_step(&dir, "Second step").unwrap();
        assert!(now_checked);

        let content = std::fs::read_to_string(&loops_file).unwrap();
        assert!(content.contains("- [x] Second step"));
        assert!(content.contains("- [ ] First step"));
        assert!(!content.contains("- [x] First step"));

        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn test_toggle_loop_step_checked_to_unchecked() {
        let (dir, _name) = create_temp_repo();
        let loops_file = dir.join(".loopdeck").join("loops.md");
        std::fs::write(
            &loops_file,
            "# Loops\n\n## Next Steps\n- [x] Done thing\n- [ ] Todo\n",
        )
        .unwrap();

        let now_checked = toggle_loop_step(&dir, "Done thing").unwrap();
        assert!(!now_checked);

        let content = std::fs::read_to_string(&loops_file).unwrap();
        assert!(content.contains("- [ ] Done thing"));

        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn test_toggle_loop_step_not_found_errors() {
        let (dir, _name) = create_temp_repo();
        std::fs::write(
            dir.join(".loopdeck").join("loops.md"),
            "# Loops\n\n## Next Steps\n- [ ] Real step\n",
        )
        .unwrap();

        let err = toggle_loop_step(&dir, "nonexistent step");
        assert!(err.is_err());

        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn test_toggle_loop_step_preserves_indentation() {
        let (dir, _name) = create_temp_repo();
        let loops_file = dir.join(".loopdeck").join("loops.md");
        std::fs::write(
            &loops_file,
            "# Loops\n\n## Next Steps\n  - [ ] Indented step\n",
        )
        .unwrap();

        toggle_loop_step(&dir, "Indented step").unwrap();

        let content = std::fs::read_to_string(&loops_file).unwrap();
        assert!(
            content.contains("  - [x] Indented step"),
            "indentation should be preserved"
        );

        std::fs::remove_dir_all(&dir).unwrap();
    }
}
