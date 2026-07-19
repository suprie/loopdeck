//! Conversation persistence for agent sessions.
//!
//! Each project keeps its conversation transcript in
//! `<project>/.loopdeck/sessions/active.jsonl` — one JSON object per line,
//! append-only. On reset, `active.jsonl` is rotated to
//! `archive-<timestamp>.jsonl` so history is never lost.
//!
//! The transcript serves two purposes:
//! 1. **Display** — the Agent tab shows prior turns even before the live
//!    process is spawned.
//! 2. **Resume** — `last_session_id` reads the most recent assistant
//!    `session_id` so a re-spawned `claude --resume <id>` restores the model's
//!    own context across app restarts.
//!
//! All reads are lenient: a missing file or a malformed line degrades to an
//! empty/partial result rather than an error, so a corrupted append never
//! bricks the UI.

use crate::agents::{ContentBlockRecord, UsageInfo};
use crate::limits;
use crate::persist;
use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::fs::OpenOptions;
use std::io::Write;
use std::path::Path;

// ── Types ──────────────────────────────────────────────────────────────────

/// The on-disk identifier for one conversation.
///
/// `"active"` is the live transcript (`active.jsonl`); any other value is an
/// archive stem (e.g. `"archive-20260703T101811Z"`) corresponding to a file
/// named `<id>.jsonl` in the sessions dir. This is the handle the history UI
/// passes back to `agent_get_conversation_by_id`.
pub type ConversationId = String;

/// One row in the conversation history list.
///
/// Built by `list_conversations` from a filesystem scan of the sessions dir:
/// one entry for `active.jsonl` (when present) plus one per `archive-*.jsonl`.
/// Each carries enough info for the history UI to show a meaningful row
/// (relative time, first-message excerpt, turn count) without reading the file
/// again. Frontend-facing — mirrors `ConversationSummary` in `types/index.ts`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ConversationSummary {
    /// `"active"` or the archive stem — pass this to load the turns.
    pub id: ConversationId,
    /// `"active"` for the live transcript, `"archived"` otherwise.
    pub kind: String,
    /// ISO-8601 timestamp of the first turn (conversation start). Empty when
    /// the transcript has no turns (an empty active file, or a corrupt one).
    pub started_ts: String,
    /// ISO-8601 timestamp of the last turn. Drives newest-first sort order.
    pub last_ts: String,
    /// Turn count for the row badge. Counts ALL turns incl. user turns.
    pub turn_count: usize,
    /// First user prompt, truncated for the row preview. Empty when the
    /// transcript has no user turns (e.g. all orphans filtered out).
    pub first_user_excerpt: String,
}

/// A tool call made by the assistant during a turn, persisted in the transcript.
///
/// Captured so the transcript records not just the model's final summary text
/// but also *how* it got there (which files it read, what it edited, which
/// commands it ran). Mirrors the live `ClaudeEvent::ToolUse` payload shape.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ToolCallRecord {
    /// Tool name, e.g. "Read", "Edit", "Bash".
    pub name: String,
    /// The raw tool input object, serialized to a JSON string.
    pub input: String,
}

/// One task lifecycle event captured from a `tool_use_result.task` line.
///
/// Claude's TodoWrite/Task tool returns structured task state as a tool result
/// (carried on a `type: "user"` event under `tool_use_result.task`). We persist
/// one `TaskRecord` per such event so the transcript records task creates /
/// updates the agent made — the same "record how the turn unfolded" principle
/// as `ToolCallRecord`, and the foundation for a future live Tasks panel.
///
/// `id` is the stable task identifier Claude assigns; correlating updates to
/// creates across turns is keyed on it. `subject` is the human-readable title;
/// `status` is our best-effort read of the action ("created"/"updated"/"…")
/// since the wire shape carries it in the surrounding text content rather than
/// a structured field.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TaskRecord {
    /// Claude's stable task identifier, e.g. `"10"`. Correlates updates to
    /// their originating create across turns.
    pub id: String,
    /// The task title / subject text.
    pub subject: String,
    /// What happened to the task in this event — `"created"`, `"updated"`, or
    /// the raw verb when we can't classify it. Best-effort, derived from the
    /// tool-result text content.
    pub status: String,
}

/// A single turn in the persisted conversation transcript.
///
/// Serialized as one JSON object per line in `active.jsonl`. The shape is
/// stable — adding fields should be `#[serde(default)]` so old transcripts
/// keep loading.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ConversationTurn {
    /// ISO-8601 timestamp of when the turn was recorded.
    pub ts: String,
    /// `"user"` for prompts sent to the agent, `"assistant"` for replies.
    pub role: String,
    /// Who originated a user turn:
    /// - `"user"` (default): the human typed it into the composer.
    /// - `"loop"`: the backend auto-built it from `.loopdeck/loops.md` when the
    ///   user clicked "Start next loop". These prompts are long boilerplate
    ///   ("You are working on this LoopDeck project… The next unchecked step
    ///   is: …") that the human never wrote — without a marker they're
    ///   indistinguishable from real messages and drown out typed ones.
    ///   Absent on assistant turns and on user turns written before this
    ///   field existed (old transcripts load as `"user"` via the default).
    #[serde(default)]
    pub source: String,
    /// The turn body: user prompt text or assistant result text.
    pub text: String,
    /// Claude session id, present on assistant turns (drives `--resume`).
    /// Absent on user turns.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    /// Whether the assistant turn was an error. Always `false` for user turns.
    #[serde(default)]
    pub is_error: bool,
    /// Token usage + cost from the assistant turn's `result` event.
    /// Absent on user turns and on assistant turns that didn't report usage.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub usage: Option<UsageInfo>,
    /// Wall-clock duration of the assistant turn in milliseconds.
    /// `0` for user turns (they're recorded instantly).
    #[serde(default)]
    pub duration_ms: u64,
    /// The model's extended-thinking chain for this assistant turn, if any.
    /// Absent on user turns and on assistant turns that produced no thinking.
    /// Added for transcript richness — `#[serde(default)]` keeps old lines loadable.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thinking: Option<String>,
    /// Tool calls the assistant made during this turn (name + input), in order.
    /// Empty/absent on user turns and on assistant turns that used no tools.
    /// Added for transcript richness — `#[serde(default)]` keeps old lines loadable.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tool_calls: Vec<ToolCallRecord>,
    /// The assistant content blocks for the turn, in **arrival order**. This is
    /// the canonical order-preserving view used for rendering; `text` /
    /// `thinking` / `tool_calls` are the flattened legacy view. Lines written
    /// before this field existed load as an empty vec and the UI falls back to
    /// the flattened fields — `#[serde(default)]` keeps old transcripts working.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub blocks: Vec<ContentBlockRecord>,
    /// Task lifecycle events (`tool_use_result.task`) observed during the turn,
    /// in arrival order. Captured so the transcript records task creates /
    /// updates alongside the tool calls that produced them. Empty/absent on
    /// turns with no task activity; `#[serde(default)]` keeps old lines loadable.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tasks: Vec<TaskRecord>,
}

impl ConversationTurn {
    /// Build a user turn at the current time (typed by the human).
    pub fn user(text: impl Into<String>) -> Self {
        Self {
            ts: Utc::now().to_rfc3339(),
            role: String::from("user"),
            source: String::from("user"),
            text: text.into(),
            session_id: None,
            is_error: false,
            usage: None,
            duration_ms: 0,
            thinking: None,
            tool_calls: Vec::new(),
            blocks: Vec::new(),
            tasks: Vec::new(),
        }
    }

    /// Build a user turn that was auto-generated by the backend (e.g. the
    /// boilerplate "next unchecked step is: …" prompt built from
    /// `.loopdeck/loops.md`). Distinct from `user` so the UI can render these
    /// as compact system rows instead of human chat bubbles.
    pub fn user_loop(text: impl Into<String>) -> Self {
        Self {
            ts: Utc::now().to_rfc3339(),
            role: String::from("user"),
            source: String::from("loop"),
            text: text.into(),
            session_id: None,
            is_error: false,
            usage: None,
            duration_ms: 0,
            thinking: None,
            tool_calls: Vec::new(),
            blocks: Vec::new(),
            tasks: Vec::new(),
        }
    }

    /// Build an assistant turn from an `AgentResponse`-shaped result, including
    /// the model's thinking chain, the tool calls it made during the turn, and
    /// any task lifecycle events observed.
    pub fn assistant(
        text: impl Into<String>,
        session_id: String,
        is_error: bool,
        usage: Option<UsageInfo>,
        duration_ms: u64,
        thinking: Option<String>,
        tool_calls: Vec<ToolCallRecord>,
        blocks: Vec<ContentBlockRecord>,
        tasks: Vec<TaskRecord>,
    ) -> Self {
        Self {
            ts: Utc::now().to_rfc3339(),
            role: String::from("assistant"),
            // Assistant turns are never auto-generated. Empty string + serde
            // default keeps the wire shape stable for old transcripts.
            source: String::new(),
            text: text.into(),
            session_id: Some(session_id).filter(|s| !s.is_empty()),
            is_error,
            usage,
            duration_ms,
            // Don't persist empty thinking — keeps the transcript tidy.
            thinking: thinking.filter(|s| !s.is_empty()),
            tool_calls,
            blocks,
            tasks,
        }
    }
}

// ── Path helpers ───────────────────────────────────────────────────────────

/// `<repo>/.loopdeck/sessions/`
fn sessions_dir(repo_path: &Path) -> std::path::PathBuf {
    repo_path.join(".loopdeck").join("sessions")
}

/// `<repo>/.loopdeck/sessions/active.jsonl`
fn active_file(repo_path: &Path) -> std::path::PathBuf {
    sessions_dir(repo_path).join("active.jsonl")
}

/// Resolve a conversation id to its file path.
///
/// `"active"` → `active.jsonl`; any other id → `<sessions>/<id>.jsonl`. This
/// is the load side of the id-from-stem convention used by `list_conversations`
/// — the id is always the filename stem, so resolution is just re-joining.
fn file_for_id(repo_path: &Path, id: &str) -> std::path::PathBuf {
    if id == "active" {
        active_file(repo_path)
    } else {
        sessions_dir(repo_path).join(format!("{id}.jsonl"))
    }
}

// ── Public API ─────────────────────────────────────────────────────────────

/// Load the active conversation transcript.
///
/// Lenient: a missing file returns an empty vec; lines that fail to parse
/// are skipped (a truncated/corrupt append shouldn't hide the turns before it).
///
/// **Orphan filtering:** because the user turn is appended *before* sending
/// (crash-safety), a turn killed mid-flight (app crash, dev rebuild, force-quit)
/// leaves a `user` turn with no following `assistant` reply. Such orphaned user
/// turns are dropped here so the transcript only shows completed exchanges —
/// the raw file keeps them for forensic purposes, but the UI never sees an
/// unanswered prompt. A `user` turn is kept only if some `assistant` turn
/// follows it anywhere later in the file.
pub fn load_conversation(repo_path: &Path) -> Vec<ConversationTurn> {
    load_conversation_by_id(repo_path, "active")
}

/// Load a conversation by id (`"active"` or an archive stem).
///
/// Same lenient parsing + orphan filtering as `load_conversation`. Unknown id
/// / missing file / unreadable file all degrade to an empty vec — a stale id
/// in the UI (e.g. an archive deleted out from under us) just shows nothing.
pub fn load_conversation_by_id(repo_path: &Path, id: &str) -> Vec<ConversationTurn> {
    let path = file_for_id(repo_path, id);
    let content = match limits::read_bounded_to_string(&path, limits::TRANSCRIPT_MAX_BYTES) {
        Ok(c) => c,
        Err(e) => {
            // Missing file is the common case (no turns yet, or stale id) —
            // debug, not warn.
            tracing::debug!("conversation {id} not readable at {}: {e}", path.display());
            return Vec::new();
        }
    };
    parse_turns(&content)
}

/// Parse raw JSONL transcript content into orphan-filtered turns.
///
/// Extracted from `load_conversation` so `load_conversation_by_id` reuses the
/// exact same lenient line parsing + orphan filter — one source of truth for
/// how a transcript file becomes a `Vec<ConversationTurn>`.
fn parse_turns(content: &str) -> Vec<ConversationTurn> {
    let mut turns: Vec<ConversationTurn> = content
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .filter_map(
            |line| match serde_json::from_str::<ConversationTurn>(line) {
                Ok(turn) => Some(turn),
                Err(e) => {
                    // A corrupt line mid-file shouldn't hide earlier valid turns.
                    tracing::warn!("skipping malformed conversation line: {e}");
                    None
                }
            },
        )
        .collect();

    filter_orphaned_user_turns(&mut turns);
    turns
}

/// Drop `user` turns that have no `assistant` reply anywhere after them.
///
/// See `load_conversation` for the rationale. Works from the end backward:
/// tracking whether any assistant turn has been seen yet, a user turn is kept
/// only if at least one assistant turn follows it. O(n) in transcript length.
fn filter_orphaned_user_turns(turns: &mut Vec<ConversationTurn>) {
    let mut saw_assistant_after = false;
    let mut keep = vec![false; turns.len()];

    // Iterate in reverse so `saw_assistant_after` means "an assistant turn
    // exists strictly later in the file than this position".
    for (i, turn) in turns.iter().enumerate().rev() {
        if turn.role == "assistant" {
            saw_assistant_after = true;
            keep[i] = true;
        } else {
            // user turn — keep only if it gets answered later
            keep[i] = saw_assistant_after;
        }
    }

    // Apply the mask: retain only kept indices, preserving order. Collecting
    // indices first avoids borrowing `turns` mutably while iterating it.
    let kept: Vec<usize> = keep
        .iter()
        .enumerate()
        .filter_map(|(i, &k)| k.then_some(i))
        .collect();
    let mut write = 0;
    for i in kept {
        turns.swap(write, i);
        write += 1;
    }
    turns.truncate(write);
}

/// Return the most recent non-empty assistant `session_id` in the active
/// transcript.
///
/// Used to re-spawn claude with `--resume <id>` after an app restart. Returns
/// `None` when there's no transcript yet (fresh conversation). Scans in file
/// order and takes the last non-empty id, so a provider that rotates session
/// ids mid-conversation is handled correctly.
pub fn last_session_id(repo_path: &Path) -> Option<String> {
    session_id_in(&load_conversation(repo_path))
}

/// The most recent non-empty assistant `session_id` in an arbitrary turn list.
///
/// Factored out of `last_session_id` so `agent_promote_to_active` can extract
/// the resume id from a *just-loaded archive* without re-reading the file.
pub fn session_id_in(turns: &[ConversationTurn]) -> Option<String> {
    turns
        .iter()
        .rev()
        .find_map(|t| t.session_id.clone().filter(|s| !s.is_empty()))
}

/// The `session_id` carried by the conversation with the given id, if any.
///
/// Used by `agent_promote_to_active` to pick the `--resume` target when the
/// user continues an archived conversation: we load its turns (already needed
/// to seed the new active transcript) and pull the resume id from them in one
/// pass. Returns `None` for an unknown id or a conversation with no assistant
/// turns (e.g. an empty or fully-orphaned transcript).
pub fn session_id_for_conversation(repo_path: &Path, id: &str) -> Option<String> {
    session_id_in(&load_conversation_by_id(repo_path, id))
}

/// Promote a conversation to active: archive the current `active.jsonl` aside
/// (so it survives in history), then copy the named conversation's turns into a
/// fresh `active.jsonl`.
///
/// This is the "continue an old conversation" primitive. The user picks an
/// archive from the history list and sends a follow-up; we make that archive
/// the live conversation so the new turn — and its `--resume`d context —
/// appends to it naturally. The previously-active conversation isn't lost:
/// it's rotated to `archive-<ts>.jsonl` first, exactly as `agent_reset_session`
/// would, so it stays browsable in history.
///
/// - `id == "active"` is a no-op (already active) — returns `Ok(())`.
/// - Unknown / empty source → returns `Ok(())` and leaves active untouched
///   rather than clobbering the current conversation with nothing. The caller
///   treats this as "nothing to promote, proceed with a fresh active."
/// - The source archive file is left in place — history is append-only; we
///   don't move or delete it. The active transcript becomes the canonical
///   going-forward copy.
pub fn promote_to_active(repo_path: &Path, id: &str) -> Result<(), std::io::Error> {
    if id == "active" {
        return Ok(());
    }

    let source = file_for_id(repo_path, id);
    let turns = if source.exists() {
        parse_turns(
            &limits::read_bounded_to_string(&source, limits::TRANSCRIPT_MAX_BYTES)
                .unwrap_or_default(),
        )
    } else {
        return Ok(()); // unknown id — nothing to promote
    };
    if turns.is_empty() {
        // Don't clobber active with an empty source — see the doc comment.
        return Ok(());
    }

    // Rotate the current active aside (if any) so it survives in history.
    archive_conversation(repo_path)?;

    // Seed a fresh active.jsonl with the source conversation's turns.
    // Atomic so a crash here can't leave a truncated transcript that breaks
    // the next agent resume — either the old active (now archived) or the
    // new complete one survives.
    let dir = sessions_dir(repo_path);
    std::fs::create_dir_all(&dir)?;
    let mut content = String::new();
    for turn in &turns {
        let mut line = serde_json::to_string(turn).map_err(|e| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("serialize turn: {e}"),
            )
        })?;
        line.push('\n');
        content.push_str(&line);
    }
    persist::atomic_write(&active_file(repo_path), &content)?;
    Ok(())
}

/// Append a single turn to `active.jsonl`, creating the sessions dir if needed.
///
/// Append-only, so this is O(1) regardless of transcript length. The user
/// turn is appended *before* `send_message` in the caller so a crash mid-turn
/// still records the user's intent.
pub fn append_turn(repo_path: &Path, turn: &ConversationTurn) -> Result<(), std::io::Error> {
    let dir = sessions_dir(repo_path);
    std::fs::create_dir_all(&dir)?;

    let mut line = serde_json::to_string(turn).map_err(|e| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("serialize turn: {e}"),
        )
    })?;
    line.push('\n');

    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(active_file(repo_path))?;
    file.write_all(line.as_bytes())?;
    // Flush to the OS page cache (no fsync — that would tank streaming
    // throughput). The PRD accepts line-atomic appends as sufficient for
    // transcripts; flush() is enough to make the append visible to a
    // subsequent read from the same process without waiting for the OS
    // writeback delay.
    file.flush()?;
    Ok(())
}

/// Rotate `active.jsonl` to `archive-<timestamp>.jsonl`.
///
/// Called on reset so the next Start is a fresh conversation (no `--resume`).
/// No-op when there's no active transcript. Uses a filesystem timestamp so
/// two resets in the same second don't collide (suffix counter).
pub fn archive_conversation(repo_path: &Path) -> Result<(), std::io::Error> {
    let active = active_file(repo_path);
    if !active.exists() {
        return Ok(());
    }

    let dir = sessions_dir(repo_path);
    let stem = Utc::now().format("%Y%m%dT%H%M%SZ").to_string();

    // Avoid clobbering an existing archive from the same second.
    let mut target = dir.join(format!("archive-{stem}.jsonl"));
    let mut i = 1;
    while target.exists() {
        target = dir.join(format!("archive-{stem}-{i}.jsonl"));
        i += 1;
    }

    std::fs::rename(&active, &target)?;
    Ok(())
}

/// The maximum length of the `first_user_excerpt` shown in the history list.
///
/// Capped so the row stays one line — the full prompt is available once the
/// user opens the conversation. Chosen to fit ~80 columns minus the prefix.
const EXCERPT_MAX: usize = 80;

/// Build the history list: one row per transcript file in the sessions dir.
///
/// Returns `active` (when present) plus every `archive-*.jsonl`, sorted
/// newest-first by the last turn's timestamp. Each row carries a one-line
/// preview (first user prompt, truncated) and a turn count, so the history UI
/// can render without re-reading files. Files we can't read are skipped
/// silently — a half-deleted state still yields the rows that survive.
///
/// The id (filename stem) is what `load_conversation_by_id` consumes, so the
/// UI can pass it straight back. Newest-first by `last_ts` (with empty
/// timestamps sorting last) keeps the most recent conversation at the top
/// where the user expects it.
pub fn list_conversations(repo_path: &Path) -> Vec<ConversationSummary> {
    let dir = sessions_dir(repo_path);
    let entries = match std::fs::read_dir(&dir) {
        Ok(e) => e,
        Err(_) => return Vec::new(), // no sessions dir yet — common case
    };

    let mut summaries: Vec<ConversationSummary> = entries
        .filter_map(Result::ok)
        .filter_map(|entry| {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            let (id, kind) = if name == "active.jsonl" {
                ("active".to_string(), "active")
            } else if let Some(stem) = name
                .strip_prefix("archive-")
                .and_then(|s| s.strip_suffix(".jsonl"))
            {
                (format!("archive-{stem}"), "archived")
            } else {
                // Not a transcript file — ignore (sessions dir may hold other
                // files in the future; don't assume exclusivity).
                return None;
            };

            let turns = parse_turns(
                &limits::read_bounded_to_string(entry.path(), limits::TRANSCRIPT_MAX_BYTES)
                    .unwrap_or_default(),
            );
            Some(summarize(&id, kind, &turns))
        })
        .collect();

    // Newest-first by last_ts. Empty timestamps sort last so an empty/corrupt
    // active file doesn't pin itself to the top.
    summaries.sort_by(|a, b| b.last_ts.cmp(&a.last_ts));
    summaries
}

/// Reduce a parsed transcript to a `ConversationSummary`.
///
/// Pulled out of `list_conversations` so the rules for deriving timestamps and
/// the excerpt live in one place: `started_ts`/`last_ts` come from the first /
/// last turn's `ts` field, and the excerpt is the first *user* turn's text,
/// trimmed and truncated.
fn summarize(id: &str, kind: &str, turns: &[ConversationTurn]) -> ConversationSummary {
    let started_ts = turns.first().map(|t| t.ts.clone()).unwrap_or_default();
    let last_ts = turns.last().map(|t| t.ts.clone()).unwrap_or_default();
    let first_user_excerpt = turns
        .iter()
        .find(|t| t.role == "user")
        .map(|t| truncate_preview(&t.text))
        .unwrap_or_default();
    ConversationSummary {
        id: id.to_string(),
        kind: kind.to_string(),
        started_ts,
        last_ts,
        turn_count: turns.len(),
        first_user_excerpt,
    }
}

/// Collapse a prompt to a one-line preview: collapse runs of whitespace and
/// cap at `EXCERPT_MAX` chars with an ellipsis. Used by `summarize` for the
/// history row's excerpt — preserves the look of "first thing the user said"
/// without letting multi-line prompts blow up the row height.
fn truncate_preview(text: &str) -> String {
    let collapsed: String = text.split_ascii_whitespace().collect::<Vec<_>>().join(" ");
    if collapsed.chars().count() <= EXCERPT_MAX {
        return collapsed;
    }
    let end = collapsed
        .char_indices()
        .nth(EXCERPT_MAX)
        .map(|(i, _)| i)
        .unwrap_or(collapsed.len());
    let mut s = collapsed[..end].to_string();
    s.push('…');
    s
}

// ── Tests ──────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn temp_repo() -> PathBuf {
        let dir = std::env::temp_dir().join(format!("loopdeck-conv-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(dir.join(".loopdeck")).unwrap();
        dir
    }

    fn usage() -> UsageInfo {
        UsageInfo {
            input_tokens: 100,
            output_tokens: 20,
            total_cost_usd: 0.005,
        }
    }

    // ── load_conversation ──────────────────────────────────────────────

    #[test]
    fn load_missing_file_returns_empty() {
        let dir = temp_repo();
        assert!(load_conversation(&dir).is_empty());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn load_roundtrips_appended_turns() {
        let dir = temp_repo();
        append_turn(&dir, &ConversationTurn::user("hello")).unwrap();
        append_turn(
            &dir,
            &ConversationTurn::assistant(
                "hi there",
                "sess-1".into(),
                false,
                Some(usage()),
                1500,
                None,
                Vec::new(),
                Vec::new(),
                Vec::new(),
            ),
        )
        .unwrap();

        let turns = load_conversation(&dir);
        assert_eq!(turns.len(), 2);
        assert_eq!(turns[0].role, "user");
        assert_eq!(turns[0].text, "hello");
        assert!(turns[0].session_id.is_none());

        assert_eq!(turns[1].role, "assistant");
        assert_eq!(turns[1].text, "hi there");
        assert_eq!(turns[1].session_id.as_deref(), Some("sess-1"));
        assert!(!turns[1].is_error);
        assert_eq!(turns[1].duration_ms, 1500);
        assert!(turns[1].usage.is_some());

        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn load_roundtrips_thinking_and_tool_calls() {
        // Verify the new richness fields survive an append → load cycle, so the
        // transcript records how the model reached its answer, not just the text.
        let dir = temp_repo();
        append_turn(
            &dir,
            &ConversationTurn::assistant(
                "done",
                "sess-9".into(),
                false,
                None,
                42,
                Some("considering the options…".into()),
                vec![
                    ToolCallRecord {
                        name: "Read".into(),
                        input: r#"{"file_path":"/a/b.rs"}"#.into(),
                    },
                    ToolCallRecord {
                        name: "Bash".into(),
                        input: r#"{"command":"cargo test"}"#.into(),
                    },
                ],
                Vec::new(),
                Vec::new(),
            ),
        )
        .unwrap();

        let turns = load_conversation(&dir);
        assert_eq!(turns.len(), 1);
        let t = &turns[0];
        assert_eq!(t.thinking.as_deref(), Some("considering the options…"));
        assert_eq!(t.tool_calls.len(), 2);
        assert_eq!(t.tool_calls[0].name, "Read");
        assert!(t.tool_calls[0].input.contains("/a/b.rs"));
        assert_eq!(t.tool_calls[1].name, "Bash");
        assert!(t.tool_calls[1].input.contains("cargo test"));

        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn load_roundtrips_tasks() {
        // Task lifecycle events must survive an append → load cycle so the
        // persisted transcript records task creates / updates the agent made.
        // Verifies the `tasks` field on `ConversationTurn` round-trips.
        let dir = temp_repo();
        append_turn(
            &dir,
            &ConversationTurn::assistant(
                "done",
                "sess-7".into(),
                false,
                None,
                1,
                None,
                Vec::new(),
                Vec::new(),
                vec![
                    TaskRecord {
                        id: "10".into(),
                        subject: "Test truncate helper".into(),
                        status: "created".into(),
                    },
                    TaskRecord {
                        id: "10".into(),
                        subject: "Test truncate helper".into(),
                        status: "updated".into(),
                    },
                ],
            ),
        )
        .unwrap();

        let turns = load_conversation(&dir);
        assert_eq!(turns.len(), 1);
        let tasks = &turns[0].tasks;
        assert_eq!(tasks.len(), 2, "both task records round-trip");
        assert_eq!(tasks[0].id, "10");
        assert_eq!(tasks[0].status, "created");
        assert_eq!(tasks[1].status, "updated");

        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn load_old_transcript_without_tasks_still_works() {
        // Backward compat: a line written before `tasks` existed must load
        // (default empty vec), so old transcripts don't break.
        let dir = temp_repo();
        let dir_sessions = dir.join(".loopdeck").join("sessions");
        std::fs::create_dir_all(&dir_sessions).unwrap();
        std::fs::write(
            active_file(&dir),
            r#"{"ts":"2026-07-03T12:00:00Z","role":"assistant","text":"hi","session_id":"s1","is_error":false,"duration_ms":5}
"#,
        )
        .unwrap();

        let turns = load_conversation(&dir);
        assert_eq!(turns.len(), 1);
        assert!(turns[0].tasks.is_empty(), "missing tasks → empty vec");

        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn load_roundtrips_blocks_in_arrival_order() {
        // The whole point of `blocks`: an interleaved sequence
        // (thinking → text → tool_use → thinking → text) must survive an
        // append → load cycle in that exact order, so the UI can render the
        // turn exactly as it unfolded instead of a fixed grouping.
        let dir = temp_repo();
        append_turn(
            &dir,
            &ConversationTurn::assistant(
                "let me checkdone",
                "sess-7".into(),
                false,
                None,
                99,
                Some("plannow edit".into()),
                vec![ToolCallRecord {
                    name: "Read".into(),
                    input: r#"{"file_path":"/a.rs"}"#.into(),
                }],
                vec![
                    ContentBlockRecord::Thinking {
                        thinking: "plan".into(),
                    },
                    ContentBlockRecord::Text {
                        text: "let me check".into(),
                    },
                    ContentBlockRecord::ToolUse {
                        name: "Read".into(),
                        input: r#"{"file_path":"/a.rs"}"#.into(),
                    },
                    ContentBlockRecord::Thinking {
                        thinking: "now edit".into(),
                    },
                    ContentBlockRecord::Text {
                        text: "done".into(),
                    },
                ],
                Vec::new(),
            ),
        )
        .unwrap();

        let turns = load_conversation(&dir);
        assert_eq!(turns.len(), 1);
        let blocks = &turns[0].blocks;
        assert_eq!(
            blocks.len(),
            5,
            "all five blocks should round-trip in order"
        );
        assert!(
            matches!(&blocks[0], ContentBlockRecord::Thinking { thinking } if thinking == "plan")
        );
        assert!(matches!(&blocks[1], ContentBlockRecord::Text { text } if text == "let me check"));
        assert!(matches!(
            &blocks[2],
            ContentBlockRecord::ToolUse { name, input } if name == "Read" && input.contains("/a.rs")
        ));
        assert!(
            matches!(&blocks[3], ContentBlockRecord::Thinking { thinking } if thinking == "now edit")
        );
        assert!(matches!(&blocks[4], ContentBlockRecord::Text { text } if text == "done"));

        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn load_old_transcript_without_new_fields_still_works() {
        // Backward compat: a line written before `thinking`/`tool_calls` existed
        // must still load (defaults applied), so old transcripts don't break.
        let dir = temp_repo();
        let dir_sessions = dir.join(".loopdeck").join("sessions");
        std::fs::create_dir_all(&dir_sessions).unwrap();
        // Hand-written line with NO thinking / tool_calls keys.
        std::fs::write(
            active_file(&dir),
            r#"{"ts":"2026-07-03T12:00:00Z","role":"assistant","text":"hi","session_id":"s1","is_error":false,"duration_ms":5}
"#,
        )
        .unwrap();

        let turns = load_conversation(&dir);
        assert_eq!(turns.len(), 1);
        assert!(turns[0].thinking.is_none(), "missing thinking → None");
        assert!(turns[0].tool_calls.is_empty(), "missing tool_calls → empty");

        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn load_skips_malformed_lines_keeps_valid() {
        let dir = temp_repo();
        // Hand-write a file with one corrupt line sandwiched between valid ones.
        let dir_sessions = dir.join(".loopdeck").join("sessions");
        std::fs::create_dir_all(&dir_sessions).unwrap();
        let valid_user = serde_json::to_string(&ConversationTurn::user("first")).unwrap();
        let valid_asst = serde_json::to_string(&ConversationTurn::assistant(
            "second",
            "s1".into(),
            false,
            None,
            10,
            None,
            Vec::new(),
            Vec::new(),
            Vec::new(),
        ))
        .unwrap();
        let content = format!("{valid_user}\nNOT JSON\n{valid_asst}\n");
        std::fs::write(active_file(&dir), content).unwrap();

        let turns = load_conversation(&dir);
        assert_eq!(
            turns.len(),
            2,
            "malformed line should be skipped, not fatal"
        );
        assert_eq!(turns[0].text, "first");
        assert_eq!(turns[1].text, "second");

        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn load_tolerates_partial_final_line_after_crash() {
        // PRD FR2: append-only transcript writes must tolerate a partial
        // final line during recovery — a crash mid-append leaves a
        // half-written JSON object at the tail. The prior valid turns must
        // still load.
        let dir = temp_repo();
        let dir_sessions = dir.join(".loopdeck").join("sessions");
        std::fs::create_dir_all(&dir_sessions).unwrap();

        // A complete user/assistant pair (so neither gets filtered as an
        // orphan), then a truncated final assistant line — what a crash
        // mid-write would leave behind. Half a JSON object, no terminating
        // brace, no newline.
        let valid_user = serde_json::to_string(&ConversationTurn::user("question")).unwrap();
        let valid_asst = serde_json::to_string(&ConversationTurn::assistant(
            "answer",
            "s1".into(),
            false,
            None,
            10,
            None,
            Vec::new(),
            Vec::new(),
            Vec::new(),
        ))
        .unwrap();
        let partial = r#"{"role":"assistant","text":"partia"#;
        let content = format!("{valid_user}\n{valid_asst}\n{partial}");
        std::fs::write(active_file(&dir), content).unwrap();

        let turns = load_conversation(&dir);
        assert_eq!(
            turns.len(),
            2,
            "valid pair loads; partial final line is skipped, not fatal"
        );
        assert_eq!(turns[0].text, "question");
        assert_eq!(turns[1].text, "answer");

        std::fs::remove_dir_all(&dir).unwrap();
    }

    // ── orphan filtering (crash-safety fallout) ───────────────────────

    #[test]
    fn load_drops_trailing_orphaned_user_turns() {
        // Simulates a turn killed mid-flight: user prompt appended, no reply.
        let dir = temp_repo();
        append_turn(&dir, &ConversationTurn::user("answered q")).unwrap();
        append_turn(
            &dir,
            &ConversationTurn::assistant(
                "a1",
                "s1".into(),
                false,
                None,
                10,
                None,
                Vec::new(),
                Vec::new(),
                Vec::new(),
            ),
        )
        .unwrap();
        // Orphan — never got a reply (process died mid-turn).
        append_turn(&dir, &ConversationTurn::user("orphan q")).unwrap();

        let turns = load_conversation(&dir);
        assert_eq!(
            turns.len(),
            2,
            "trailing orphaned user turn should be dropped"
        );
        assert_eq!(turns[0].text, "answered q");
        assert_eq!(turns[1].text, "a1");

        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn load_drops_multiple_trailing_orphans() {
        // Two unanswered prompts in a row (e.g. repeated killed starts).
        let dir = temp_repo();
        append_turn(&dir, &ConversationTurn::user("q1")).unwrap();
        append_turn(
            &dir,
            &ConversationTurn::assistant(
                "a1",
                "s1".into(),
                false,
                None,
                10,
                None,
                Vec::new(),
                Vec::new(),
                Vec::new(),
            ),
        )
        .unwrap();
        append_turn(&dir, &ConversationTurn::user("orphan1")).unwrap();
        append_turn(&dir, &ConversationTurn::user("orphan2")).unwrap();

        let turns = load_conversation(&dir);
        assert_eq!(turns.len(), 2, "both trailing orphans should be dropped");

        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn load_keeps_answered_user_turns_and_drops_only_unanswered() {
        // Mixed: a mid-file user turn IS answered (keep), a trailing one is not (drop).
        let dir = temp_repo();
        append_turn(&dir, &ConversationTurn::user("q1")).unwrap();
        append_turn(
            &dir,
            &ConversationTurn::assistant(
                "a1",
                "s1".into(),
                false,
                None,
                10,
                None,
                Vec::new(),
                Vec::new(),
                Vec::new(),
            ),
        )
        .unwrap();
        append_turn(&dir, &ConversationTurn::user("q2")).unwrap();
        append_turn(
            &dir,
            &ConversationTurn::assistant(
                "a2",
                "s2".into(),
                false,
                None,
                20,
                None,
                Vec::new(),
                Vec::new(),
                Vec::new(),
            ),
        )
        .unwrap();
        append_turn(&dir, &ConversationTurn::user("orphan")).unwrap();

        let turns = load_conversation(&dir);
        assert_eq!(turns.len(), 4);
        assert_eq!(turns[0].text, "q1");
        assert_eq!(turns[1].text, "a1");
        assert_eq!(turns[2].text, "q2");
        assert_eq!(turns[3].text, "a2");

        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn load_empty_when_only_orphaned_user_turns() {
        // No assistant turns at all → everything is an orphan.
        let dir = temp_repo();
        append_turn(&dir, &ConversationTurn::user("q1")).unwrap();
        append_turn(&dir, &ConversationTurn::user("q2")).unwrap();

        let turns = load_conversation(&dir);
        assert!(
            turns.is_empty(),
            "unanswered prompts should all be filtered"
        );

        std::fs::remove_dir_all(&dir).unwrap();
    }

    // ── last_session_id ────────────────────────────────────────────────

    #[test]
    fn last_session_id_none_when_empty() {
        let dir = temp_repo();
        assert!(last_session_id(&dir).is_none());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn last_session_id_returns_most_recent() {
        let dir = temp_repo();
        append_turn(
            &dir,
            &ConversationTurn::assistant(
                "a",
                "old".into(),
                false,
                None,
                1,
                None,
                Vec::new(),
                Vec::new(),
                Vec::new(),
            ),
        )
        .unwrap();
        append_turn(&dir, &ConversationTurn::user("follow up")).unwrap();
        append_turn(
            &dir,
            &ConversationTurn::assistant(
                "b",
                "new".into(),
                false,
                None,
                2,
                None,
                Vec::new(),
                Vec::new(),
                Vec::new(),
            ),
        )
        .unwrap();

        assert_eq!(last_session_id(&dir).as_deref(), Some("new"));

        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn last_session_id_ignores_empty_id() {
        let dir = temp_repo();
        // An assistant turn with an empty session_id shouldn't be picked.
        append_turn(
            &dir,
            &ConversationTurn::assistant(
                "a",
                String::new(),
                false,
                None,
                1,
                None,
                Vec::new(),
                Vec::new(),
                Vec::new(),
            ),
        )
        .unwrap();
        assert!(last_session_id(&dir).is_none());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    // ── append_turn ────────────────────────────────────────────────────

    #[test]
    fn append_creates_sessions_dir() {
        // Project with no .loopdeck/sessions at all.
        let dir = std::env::temp_dir().join(format!("loopdeck-conv-bare-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();

        append_turn(&dir, &ConversationTurn::user("hi")).unwrap();
        assert!(active_file(&dir).exists());

        std::fs::remove_dir_all(&dir).unwrap();
    }

    // ── archive_conversation ───────────────────────────────────────────

    #[test]
    fn archive_noop_when_no_active() {
        let dir = temp_repo();
        // No active.jsonl exists — archive should be a clean no-op.
        archive_conversation(&dir).unwrap();
        assert!(!active_file(&dir).exists());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn archive_rotates_active_to_archive_file() {
        let dir = temp_repo();
        append_turn(&dir, &ConversationTurn::user("hi")).unwrap();
        assert!(active_file(&dir).exists());

        archive_conversation(&dir).unwrap();

        // active.jsonl is gone; exactly one archive file exists.
        assert!(!active_file(&dir).exists());
        let archives: Vec<_> = std::fs::read_dir(sessions_dir(&dir))
            .unwrap()
            .filter_map(Result::ok)
            .filter(|e| e.file_name().to_string_lossy().starts_with("archive-"))
            .collect();
        assert_eq!(archives.len(), 1, "expected exactly one archive file");

        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn archive_then_load_is_empty() {
        let dir = temp_repo();
        append_turn(&dir, &ConversationTurn::user("hi")).unwrap();
        archive_conversation(&dir).unwrap();
        assert!(load_conversation(&dir).is_empty());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn archive_twice_same_second_does_not_clobber() {
        let dir = temp_repo();
        append_turn(&dir, &ConversationTurn::user("first")).unwrap();
        archive_conversation(&dir).unwrap();
        append_turn(&dir, &ConversationTurn::user("second")).unwrap();
        archive_conversation(&dir).unwrap();

        let archives: Vec<_> = std::fs::read_dir(sessions_dir(&dir))
            .unwrap()
            .filter_map(Result::ok)
            .filter(|e| e.file_name().to_string_lossy().starts_with("archive-"))
            .collect();
        assert_eq!(archives.len(), 2, "two archives should both survive");

        std::fs::remove_dir_all(&dir).unwrap();
    }

    // ── list_conversations / load_conversation_by_id ───────────────────

    #[test]
    fn list_returns_empty_when_no_sessions_dir() {
        // No `.loopdeck/sessions` at all — common pre-first-turn state.
        let dir = std::env::temp_dir().join(format!("loopdeck-conv-bare-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();

        assert!(list_conversations(&dir).is_empty());

        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn list_includes_active_and_archives_sorted_newest_first() {
        // Build three transcripts with controllable timestamps so the sort
        // is deterministic regardless of how fast the test runs. Each is a
        // user+assistant PAIR so the orphan filter doesn't drop the user turn
        // (a lone user turn would be filtered → empty timestamps → wrong sort).
        let dir = temp_repo();

        // "oldest" archive: 2026-01-01.
        write_active_pair_with_ts(&dir, "2026-01-01T00:00:00Z", "old q");
        std::fs::rename(
            active_file(&dir),
            sessions_dir(&dir).join("archive-20260101T000000Z.jsonl"),
        )
        .unwrap();

        // "middle" archive: 2026-06-01.
        write_active_pair_with_ts(&dir, "2026-06-01T00:00:00Z", "mid q");
        std::fs::rename(
            active_file(&dir),
            sessions_dir(&dir).join("archive-20260601T000000Z.jsonl"),
        )
        .unwrap();

        // "active" (newest content): 2026-07-01.
        write_active_pair_with_ts(&dir, "2026-07-01T00:00:00Z", "live q");

        let list = list_conversations(&dir);

        // Three rows: active + two archives. Active sorts first because its
        // last_ts (2026-07-01) is the newest.
        assert_eq!(list.len(), 3, "active + both archives should appear");
        assert_eq!(list[0].id, "active");
        assert_eq!(list[0].kind, "active");
        assert_eq!(list[0].last_ts, "2026-07-01T00:00:00Z");
        assert_eq!(list[1].id, "archive-20260601T000000Z");
        assert_eq!(list[1].kind, "archived");
        assert_eq!(list[2].id, "archive-20260101T000000Z");
        assert!(list[1].last_ts > list[2].last_ts, "newest-first ordering");

        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn list_summarizes_turn_count_and_excerpt() {
        // A user+assistant pair survives the orphan filter, so the summary
        // reports both turns and the user prompt as the excerpt.
        let dir = temp_repo();
        append_turn(&dir, &ConversationTurn::user("hello world prompt")).unwrap();
        append_turn(
            &dir,
            &ConversationTurn::assistant(
                "hi",
                "s1".into(),
                false,
                None,
                10,
                None,
                Vec::new(),
                Vec::new(),
                Vec::new(),
            ),
        )
        .unwrap();

        let list = list_conversations(&dir);
        assert_eq!(list.len(), 1);
        let row = &list[0];
        assert_eq!(row.id, "active");
        assert_eq!(row.turn_count, 2, "user + assistant");
        assert_eq!(row.first_user_excerpt, "hello world prompt");
        assert!(!row.started_ts.is_empty());
        assert!(!row.last_ts.is_empty());

        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn list_excerpt_truncates_long_prompts() {
        // The excerpt is capped at EXCERPT_MAX chars + ellipsis. Use a
        // user+assistant pair so the user turn survives orphan filtering.
        let dir = temp_repo();
        let long = "x".repeat(EXCERPT_MAX * 2);
        append_turn(&dir, &ConversationTurn::user(&long)).unwrap();
        append_turn(
            &dir,
            &ConversationTurn::assistant(
                "ok",
                "s1".into(),
                false,
                None,
                1,
                None,
                Vec::new(),
                Vec::new(),
                Vec::new(),
            ),
        )
        .unwrap();

        let row = &list_conversations(&dir)[0];
        // Truncation kicks in beyond EXCERPT_MAX chars and adds the ellipsis.
        assert!(row.first_user_excerpt.chars().count() <= EXCERPT_MAX + 1);
        assert!(row.first_user_excerpt.ends_with('…'));

        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn list_collapse_multiline_prompt_into_one_line_excerpt() {
        // Multi-line prompts collapse to one space-joined line in the excerpt.
        // User+assistant pair so the user turn isn't orphan-filtered.
        let dir = temp_repo();
        append_turn(
            &dir,
            &ConversationTurn::user("line one\nline two\n  spaced"),
        )
        .unwrap();
        append_turn(
            &dir,
            &ConversationTurn::assistant(
                "ok",
                "s1".into(),
                false,
                None,
                1,
                None,
                Vec::new(),
                Vec::new(),
                Vec::new(),
            ),
        )
        .unwrap();

        let row = &list_conversations(&dir)[0];
        assert_eq!(row.first_user_excerpt, "line one line two spaced");

        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn load_by_id_returns_correct_turns() {
        // Each conversation is a user+assistant pair so orphan filtering
        // doesn't drop the user turn we're asserting against.
        let dir = temp_repo();
        // Archive first: write a pair to active, then rename it to an archive.
        write_active_pair_with_ts(&dir, "2026-03-01T00:00:00Z", "archived prompt");
        std::fs::rename(
            active_file(&dir),
            sessions_dir(&dir).join("archive-20260301T000000Z.jsonl"),
        )
        .unwrap();
        // Now active: a different pair (live). write_active_pair_with_ts
        // overwrites active.jsonl fresh.
        write_active_pair_with_ts(&dir, "2026-07-01T00:00:00Z", "live prompt");

        let live = load_conversation_by_id(&dir, "active");
        assert_eq!(live.len(), 2, "user + assistant");
        assert_eq!(live[0].text, "live prompt");
        assert_eq!(live[1].role, "assistant");

        let archived = load_conversation_by_id(&dir, "archive-20260301T000000Z");
        assert_eq!(archived.len(), 2, "user + assistant");
        assert_eq!(archived[0].text, "archived prompt");
        assert_eq!(archived[1].role, "assistant");

        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn load_by_id_unknown_returns_empty() {
        // Stale id (e.g. archive deleted out of band) → empty, not an error.
        let dir = temp_repo();
        assert!(load_conversation_by_id(&dir, "archive-doesnotexist").is_empty());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn load_conversation_equivalent_to_load_by_id_active() {
        // The public active loader must remain a thin wrapper over the by-id
        // loader — same parsing, same orphan filter.
        let dir = temp_repo();
        append_turn(&dir, &ConversationTurn::user("q")).unwrap();
        append_turn(
            &dir,
            &ConversationTurn::assistant(
                "a",
                "s1".into(),
                false,
                None,
                1,
                None,
                Vec::new(),
                Vec::new(),
                Vec::new(),
            ),
        )
        .unwrap();

        assert_eq!(
            load_conversation(&dir),
            load_conversation_by_id(&dir, "active")
        );

        std::fs::remove_dir_all(&dir).unwrap();
    }

    // ── promote_to_active / session_id_for_conversation ────────────────

    #[test]
    fn promote_to_active_is_noop_for_active() {
        // Promoting the already-active id must be a true no-op — the current
        // active turns must survive untouched, and no archive should be created.
        let dir = temp_repo();
        append_turn(&dir, &ConversationTurn::user("live")).unwrap();
        append_turn(
            &dir,
            &ConversationTurn::assistant(
                "reply",
                "s1".into(),
                false,
                None,
                1,
                None,
                Vec::new(),
                Vec::new(),
                Vec::new(),
            ),
        )
        .unwrap();

        promote_to_active(&dir, "active").unwrap();

        assert_eq!(load_conversation(&dir).len(), 2, "active turns unchanged");
        let archives: Vec<_> = std::fs::read_dir(sessions_dir(&dir))
            .unwrap()
            .filter_map(Result::ok)
            .filter(|e| e.file_name().to_string_lossy().starts_with("archive-"))
            .collect();
        assert!(archives.is_empty(), "no archive should be created");

        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn promote_to_active_noop_for_unknown_or_empty() {
        // Unknown id or empty source must NOT clobber the current active —
        // the user's current conversation survives a bad promote request.
        let dir = temp_repo();
        append_turn(&dir, &ConversationTurn::user("live")).unwrap();
        append_turn(
            &dir,
            &ConversationTurn::assistant(
                "reply",
                "s1".into(),
                false,
                None,
                1,
                None,
                Vec::new(),
                Vec::new(),
                Vec::new(),
            ),
        )
        .unwrap();

        // Unknown id.
        promote_to_active(&dir, "archive-doesnotexist").unwrap();
        assert_eq!(
            load_conversation(&dir).len(),
            2,
            "active untouched by unknown id"
        );

        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn promote_to_active_seeds_active_from_archive_and_preserves_old_active() {
        // The headline "continue old conversation" behavior: promote an archive
        // → its turns become active, and the previously-active conversation is
        // rotated aside as a new archive (so it stays in history).
        let dir = temp_repo();
        // Current active: "current" conversation.
        write_active_pair_with_ts(&dir, "2026-07-01T00:00:00Z", "current q");
        // An older archive with a known session_id we can later resume from.
        let mut user = ConversationTurn::user("old q");
        user.ts = "2026-01-01T00:00:00Z".to_string();
        let mut asst = ConversationTurn::assistant(
            "old a",
            "sess-old".into(),
            false,
            None,
            1,
            None,
            Vec::new(),
            Vec::new(),
            Vec::new(),
        );
        asst.ts = "2026-01-01T00:00:01Z".to_string();
        let old_path = sessions_dir(&dir).join("archive-20260101T000000Z.jsonl");
        std::fs::write(
            &old_path,
            format!(
                "{}\n{}\n",
                serde_json::to_string(&user).unwrap(),
                serde_json::to_string(&asst).unwrap()
            ),
        )
        .unwrap();

        promote_to_active(&dir, "archive-20260101T000000Z").unwrap();

        // active.jsonl now carries the OLD conversation's turns.
        let active = load_conversation(&dir);
        assert_eq!(active.len(), 2);
        assert_eq!(active[0].text, "old q");
        assert_eq!(active[1].text, "old a");
        assert_eq!(active[1].session_id.as_deref(), Some("sess-old"));

        // The previously-active "current" conversation survives as exactly one
        // new archive (plus the source archive we promoted from).
        let archives: Vec<String> = std::fs::read_dir(sessions_dir(&dir))
            .unwrap()
            .filter_map(Result::ok)
            .map(|e| e.file_name().to_string_lossy().to_string())
            .filter(|n| n.starts_with("archive-"))
            .collect();
        assert_eq!(archives.len(), 2, "source archive + rotated active");
        // The source archive we promoted FROM is still on disk (we don't move it).
        assert!(
            archives
                .iter()
                .any(|n| n == "archive-20260101T000000Z.jsonl"),
            "source archive preserved"
        );

        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn promote_to_active_appends_into_existing_active_when_no_prior() {
        // Edge: promoting when there's NO current active (fresh project). Must
        // not error and must seed active from the archive without archiving
        // anything (nothing to archive).
        let dir = temp_repo();
        let mut user = ConversationTurn::user("only q");
        user.ts = "2026-02-01T00:00:00Z".to_string();
        let mut asst = ConversationTurn::assistant(
            "only a",
            "s9".into(),
            false,
            None,
            1,
            None,
            Vec::new(),
            Vec::new(),
            Vec::new(),
        );
        asst.ts = "2026-02-01T00:00:01Z".to_string();
        std::fs::create_dir_all(sessions_dir(&dir)).unwrap();
        std::fs::write(
            sessions_dir(&dir).join("archive-20260201T000000Z.jsonl"),
            format!(
                "{}\n{}\n",
                serde_json::to_string(&user).unwrap(),
                serde_json::to_string(&asst).unwrap()
            ),
        )
        .unwrap();

        promote_to_active(&dir, "archive-20260201T000000Z").unwrap();

        let active = load_conversation(&dir);
        assert_eq!(active.len(), 2);
        assert_eq!(active[0].text, "only q");

        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn session_id_for_conversation_returns_resume_target() {
        // The resume id for the promote path comes from the (just-loaded)
        // archive's most recent assistant session_id.
        let dir = temp_repo();
        let mut user = ConversationTurn::user("q");
        user.ts = "2026-01-01T00:00:00Z".to_string();
        let mut asst = ConversationTurn::assistant(
            "a",
            "sess-resume".into(),
            false,
            None,
            1,
            None,
            Vec::new(),
            Vec::new(),
            Vec::new(),
        );
        asst.ts = "2026-01-01T00:00:01Z".to_string();
        std::fs::create_dir_all(sessions_dir(&dir)).unwrap();
        std::fs::write(
            sessions_dir(&dir).join("archive-20260101T000000Z.jsonl"),
            format!(
                "{}\n{}\n",
                serde_json::to_string(&user).unwrap(),
                serde_json::to_string(&asst).unwrap()
            ),
        )
        .unwrap();

        assert_eq!(
            session_id_for_conversation(&dir, "archive-20260101T000000Z").as_deref(),
            Some("sess-resume")
        );
        // Unknown id / empty conversation → None.
        assert!(session_id_for_conversation(&dir, "active").is_none());
        assert!(session_id_for_conversation(&dir, "archive-missing").is_none());

        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// Helper: overwrite `active.jsonl` with a user+assistant pair sharing the
    /// given timestamp. Used by the list/load-by-id tests to control ordering
    /// without depending on `Utc::now`. The pair survives orphan filtering
    /// (a lone user turn would be dropped — see `filter_orphaned_user_turns`).
    fn write_active_pair_with_ts(dir: &Path, ts: &str, user_text: &str) {
        let mut user = ConversationTurn::user(user_text);
        user.ts = ts.to_string();
        let mut asst = ConversationTurn::assistant(
            "ok",
            "s-test".into(),
            false,
            None,
            1,
            None,
            Vec::new(),
            Vec::new(),
            Vec::new(),
        );
        asst.ts = ts.to_string();
        std::fs::create_dir_all(sessions_dir(dir)).unwrap();
        let u = serde_json::to_string(&user).unwrap();
        let a = serde_json::to_string(&asst).unwrap();
        std::fs::write(active_file(dir), format!("{u}\n{a}\n")).unwrap();
    }
}
