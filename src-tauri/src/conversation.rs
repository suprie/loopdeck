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

use crate::agents::UsageInfo;
use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::fs::OpenOptions;
use std::io::Write;
use std::path::Path;

// ── Types ──────────────────────────────────────────────────────────────────

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
}

impl ConversationTurn {
    /// Build a user turn at the current time.
    pub fn user(text: impl Into<String>) -> Self {
        Self {
            ts: Utc::now().to_rfc3339(),
            role: String::from("user"),
            text: text.into(),
            session_id: None,
            is_error: false,
            usage: None,
            duration_ms: 0,
        }
    }

    /// Build an assistant turn from an `AgentResponse`-shaped result.
    pub fn assistant(
        text: impl Into<String>,
        session_id: String,
        is_error: bool,
        usage: Option<UsageInfo>,
        duration_ms: u64,
    ) -> Self {
        Self {
            ts: Utc::now().to_rfc3339(),
            role: String::from("assistant"),
            text: text.into(),
            session_id: Some(session_id).filter(|s| !s.is_empty()),
            is_error,
            usage,
            duration_ms,
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
    let content = match std::fs::read_to_string(active_file(repo_path)) {
        Ok(c) => c,
        Err(e) => {
            // Missing file is the common case (no turns yet) — debug, not warn.
            tracing::debug!(
                "active.jsonl not readable at {}: {e}",
                active_file(repo_path).display()
            );
            return Vec::new();
        }
    };

    let mut turns: Vec<ConversationTurn> = content
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .filter_map(|line| match serde_json::from_str::<ConversationTurn>(line) {
            Ok(turn) => Some(turn),
            Err(e) => {
                // A corrupt line mid-file shouldn't hide earlier valid turns.
                tracing::warn!("skipping malformed conversation line: {e}");
                None
            }
        })
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
    let kept: Vec<usize> = keep.iter().enumerate().filter_map(|(i, &k)| k.then_some(i)).collect();
    let mut write = 0;
    for i in kept {
        turns.swap(write, i);
        write += 1;
    }
    turns.truncate(write);
}

/// Return the most recent non-empty assistant `session_id` in the transcript.
///
/// Used to re-spawn claude with `--resume <id>` after an app restart. Returns
/// `None` when there's no transcript yet (fresh conversation). Scans in file
/// order and takes the last non-empty id, so a provider that rotates session
/// ids mid-conversation is handled correctly.
pub fn last_session_id(repo_path: &Path) -> Option<String> {
    load_conversation(repo_path)
        .iter()
        .rev()
        .find_map(|t| t.session_id.clone().filter(|s| !s.is_empty()))
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
        std::io::Error::new(std::io::ErrorKind::InvalidData, format!("serialize turn: {e}"))
    })?;
    line.push('\n');

    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(active_file(repo_path))?;
    file.write_all(line.as_bytes())?;
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
            &ConversationTurn::assistant("hi there", "sess-1".into(), false, Some(usage()), 1500),
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
        ))
        .unwrap();
        let content = format!("{valid_user}\nNOT JSON\n{valid_asst}\n");
        std::fs::write(active_file(&dir), content).unwrap();

        let turns = load_conversation(&dir);
        assert_eq!(turns.len(), 2, "malformed line should be skipped, not fatal");
        assert_eq!(turns[0].text, "first");
        assert_eq!(turns[1].text, "second");

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
            &ConversationTurn::assistant("a1", "s1".into(), false, None, 10),
        )
        .unwrap();
        // Orphan — never got a reply (process died mid-turn).
        append_turn(&dir, &ConversationTurn::user("orphan q")).unwrap();

        let turns = load_conversation(&dir);
        assert_eq!(turns.len(), 2, "trailing orphaned user turn should be dropped");
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
            &ConversationTurn::assistant("a1", "s1".into(), false, None, 10),
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
            &ConversationTurn::assistant("a1", "s1".into(), false, None, 10),
        )
        .unwrap();
        append_turn(&dir, &ConversationTurn::user("q2")).unwrap();
        append_turn(
            &dir,
            &ConversationTurn::assistant("a2", "s2".into(), false, None, 20),
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
        assert!(turns.is_empty(), "unanswered prompts should all be filtered");

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
            &ConversationTurn::assistant("a", "old".into(), false, None, 1),
        )
        .unwrap();
        append_turn(&dir, &ConversationTurn::user("follow up")).unwrap();
        append_turn(
            &dir,
            &ConversationTurn::assistant("b", "new".into(), false, None, 2),
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
            &ConversationTurn::assistant("a", String::new(), false, None, 1),
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
            .filter(|e| {
                e.file_name()
                    .to_string_lossy()
                    .starts_with("archive-")
            })
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
            .filter(|e| {
                e.file_name()
                    .to_string_lossy()
                    .starts_with("archive-")
            })
            .collect();
        assert_eq!(archives.len(), 2, "two archives should both survive");

        std::fs::remove_dir_all(&dir).unwrap();
    }
}
