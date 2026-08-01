//! Central resource budgets for untrusted work (PRD FR4).
//!
//! Everything that processes untrusted input — a repository's filesystem
//! (discovery scans, `@`-mention search, README/spec/transcript reads), the
//! `claude` CLI's NDJSON stream, and agent/model output accumulated into a
//! response — is bounded by a named constant in this one module. Centralizing
//! them makes the budgets visible, tunable, and easy to reason about as a set,
//! rather than scattered as magic numbers at each call site.
//!
//! ## Design stance
//!
//! The budgets are DoS / exhaustion guards, not correctness limits. When a
//! budget is hit the application stays responsive and usable:
//!
//! - **Filesystem walks** (scan, search) stop and return whatever they found so
//!   far, with a `warn!` log — discovery still shows results, just fewer.
//! - **Single-object loads** (an oversized NDJSON line) can't degrade to a
//!   partial result, so they surface as a structured [`crate::error::AppError::Limit`]
//!   and end the operation cleanly.
//! - **Accumulation** (the agent response) caps memory and stamps a truncation
//!   marker rather than aborting the turn.
//!
//! See `docs/PRD-trust-boundary-hardening.md` FR4, and the loops.md history
//! entry for the same date.

use std::path::Path;
use std::time::Duration;

// ── Recursive discovery scan (`scanner::scan_directory`) ───────────────────

/// Max directory entries visited during a single discovery scan.
///
/// Bounds the `walkdir` traversal against a pathologically wide/deep tree
/// (e.g. pointing the scanner at `$HOME`). The depth budget
/// (`Settings::scan_depth`) already caps how deep we go; this caps how *much*
/// we visit regardless of depth.
pub const SCAN_MAX_ENTRIES: usize = 200_000;

/// Max repositories returned from one discovery scan.
///
/// The discovery UI is browsable; beyond this many hits a repo is unlikely to
/// be the one the user is looking for, and an unbounded `Vec` is itself an
/// exhaustion vector.
pub const SCAN_MAX_RESULTS: usize = 5_000;

/// Wall-clock budget for a single discovery scan.
///
/// A scan that exceeds this is aborted (returning partial results) rather than
/// freezing the UI on a giant tree. Generous enough for a real monorepo at the
/// default scan depth; only an adversarially large root hits it.
pub const SCAN_MAX_DURATION: Duration = Duration::from_secs(30);

// ── `@`-mention search walk (`commands::walk_root`) ───────────────────────

/// Max depth the `@`-mention search descends.
///
/// `walk_root` recurses with no inherent depth bound; a deeply-nested project
/// could recurse arbitrarily deep (and overflow the call stack). This caps both.
pub const SEARCH_MAX_DEPTH: u8 = 15;

/// Max entries visited during one search walk.
///
/// The result cap (`search_project_files`'s `max_results`, default 50) bounds
/// what's *returned*; this bounds what's *visited* to find those hits, so a
/// huge tree can't make one keystroke expensive.
pub const SEARCH_MAX_ENTRIES: usize = 100_000;

/// Wall-clock budget for one search walk.
///
/// The autocomplete popup is expected to feel instant; a walk that exceeds
/// this returns what it has and stops.
pub const SEARCH_MAX_DURATION: Duration = Duration::from_secs(10);

// ── File-size budgets (loaded wholly into memory) ──────────────────────────

/// Max bytes read from a README for description generation.
///
/// We only extract the first paragraph, so a cap keeps a hostile giant README
/// out of memory without changing the extracted result in practice.
pub const README_MAX_BYTES: u64 = 64 * 1024;

/// Max bytes of `.loopdeck/execution.yaml` read wholesale.
///
/// App-managed structured state (current loop, queue, completion history). 1
/// MiB is far beyond any real project history at schema v1; history is
/// checkpointed to an append-only archive only in a later schema version if it
/// ever grows material.
pub const EXECUTION_MAX_BYTES: u64 = 1024 * 1024;

/// Max bytes of `.loopdeck/run-plan.yaml` read wholesale.
///
/// A queued run plus its pinned interview answers and park payloads; 1 MiB
/// matches `EXECUTION_MAX_BYTES` and is far beyond any real overnight run.
pub const RUN_PLAN_MAX_BYTES: u64 = 1024 * 1024;

/// Max bytes of a spec / PRD / epic Markdown file read wholesale.
///
/// These live under `docs/epics/` (human-authored) and `.loopdeck/` (agent-
/// written); both are untrusted content. 1 MiB is far beyond any legitimate
/// spec file.
pub const SPEC_MAX_BYTES: u64 = 1024 * 1024;

/// Max bytes of a `SKILL.md` read for the skill-discovery menu.
///
/// Only the YAML frontmatter is parsed; the body is never inspected, so a cap
/// well above any real frontmatter is harmless and bounds a planted huge file.
pub const SKILL_MAX_BYTES: u64 = 256 * 1024;

/// Max bytes of a transcript file read wholesale for display.
///
/// Transcripts are append-only and turn-sized in practice; 16 MiB is hundreds
/// of turns. Beyond that the file is truncated at a line boundary by the
/// lenient loader (a partial final line is skipped, as on crash recovery).
pub const TRANSCRIPT_MAX_BYTES: u64 = 16 * 1024 * 1024;

// ── NDJSON stream line budget (`claude` CLI) ───────────────────────────────

/// Max bytes of a single NDJSON line from the `claude` CLI stream.
///
/// A tool result that embeds a huge file (or a runaway `result` event) could
/// otherwise be buffered wholly into memory by an unbounded `read_line`. A line
/// exceeding this is discarded and surfaced as a [`crate::error::AppError::Limit`].
/// 4 MiB is well above any legitimate single stream-json line.
pub const STREAM_LINE_MAX_BYTES: usize = 4 * 1024 * 1024;

// ── Composer image attachments ─────────────────────────────────────────────
//
// Attachments are stored inline as base64 in the transcript AND written inline
// into the single NDJSON line that carries the user turn to the CLI's stdin.
// Both of those are why these are bounded here rather than left to the
// frontend: the browser downscales before encoding (keeping a typical
// screenshot in the low hundreds of KB), but the backend must not *trust*
// that — a hand-crafted IPC call could otherwise write an unbounded line to
// the child process and blow up `TRANSCRIPT_MAX_BYTES` in one turn.

/// Max base64 length of a single attached image.
///
/// Base64 inflates by ~4/3, so this is ~3 MiB of raw image — far above
/// anything the frontend's 1568px downscale produces, while still refusing a
/// multi-hundred-megabyte paste.
pub const ATTACHMENT_MAX_BYTES: usize = 4 * 1024 * 1024;

/// Max combined base64 length of all attachments on one turn.
///
/// Kept in the same order of magnitude as [`STREAM_LINE_MAX_BYTES`] because
/// the turn is written as one NDJSON line: what we refuse to *read* at 4 MiB
/// we should not casually *write* at 100.
pub const ATTACHMENTS_MAX_TOTAL_BYTES: usize = 8 * 1024 * 1024;

/// Max number of images on a single turn. A guard against pathological counts
/// of tiny images, which the byte budgets alone would happily allow.
pub const ATTACHMENTS_MAX_COUNT: usize = 8;

// ── Agent response accumulation (`agents::ResponseAccumulator`) ────────────

/// Max content blocks recorded in one accumulated turn.
///
/// Bounds the persisted `blocks` / `tool_calls` vecs against a runaway agent
/// that emits thousands of blocks. Past this, further blocks are dropped and
/// the response is stamped with a truncation marker.
pub const ACCUMULATOR_MAX_BLOCKS: usize = 5_000;

/// Max bytes accumulated in one turn across text, thinking, tool-call input,
/// and task payloads.
///
/// The dominant memory of a turn is its content strings; this caps their total.
/// `blocks` mirror the same content and are bounded separately by
/// [`ACCUMULATOR_MAX_BLOCKS`]. Past this, further content is dropped and the
/// response is stamped with a truncation marker.
pub const ACCUMULATOR_MAX_BYTES: usize = 16 * 1024 * 1024;

// ── Bounded file-read helper ───────────────────────────────────────────────

/// Read up to `max_bytes` from `path` into a `String`, decoding lossily.
///
/// A drop-in for `std::fs::read_to_string` that bounds peak memory: at most
/// `max_bytes` are read. Bytes beyond the cap are ignored. Decoding is lossy
/// (`String::from_utf8_lossy`) so a truncation that splits a multi-byte UTF-8
/// sequence degrades to replacement characters instead of erroring — these are
/// display strings (README excerpts, spec Markdown, transcripts), and the
/// existing lenient parsers skip anything malformed anyway.
///
/// Accepts `AsRef<Path>` so callers can pass `&Path`, `&PathBuf`, or an owned
/// `PathBuf` (e.g. `DirEntry::path()`) uniformly.
///
/// Returns the underlying [`std::io::Error`] (open/read failures) so callers
/// using `?` convert via the existing `#[from] io::Error` on `AppError`.
pub fn read_bounded_to_string<P: AsRef<Path>>(
    path: P,
    max_bytes: u64,
) -> Result<String, std::io::Error> {
    use std::io::Read;
    let file = std::fs::File::open(path)?;
    let mut buf = Vec::new();
    file.take(max_bytes).read_to_end(&mut buf)?;
    Ok(String::from_utf8_lossy(&buf).into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn read_bounded_returns_full_small_file() {
        let dir = std::env::temp_dir().join(format!("loopdeck-lim-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("small.txt");
        std::fs::write(&path, "hello world").unwrap();

        let s = read_bounded_to_string(&path, 1024).unwrap();
        assert_eq!(s, "hello world");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn read_bounded_truncates_at_cap() {
        let dir = std::env::temp_dir().join(format!("loopdeck-lim-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("big.txt");
        // 100 bytes of ASCII; cap at 10 → only first 10 read.
        std::fs::write(&path, "a".repeat(100)).unwrap();

        let s = read_bounded_to_string(&path, 10).unwrap();
        assert_eq!(s.len(), 10, "cap should bound the read");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn read_bounded_lossy_on_split_multibyte() {
        // A 3-byte UTF-8 char (€) truncated after 1 byte should decode lossily
        // to a replacement char, not error. This is the realistic "cap landed
        // mid-codepoint" case for a giant README/transcript.
        let dir = std::env::temp_dir().join(format!("loopdeck-lim-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("utf8.txt");
        // "€" is 0xE2 0x82 0xAC. Take 1 byte → invalid UTF-8 on its own.
        let bytes: Vec<u8> = vec![0xE2, b'A', b'B', b'C'];
        std::fs::write(&path, &bytes).unwrap();

        let s = read_bounded_to_string(&path, 1).unwrap();
        // Lossy decode of 0xE2 alone → U+FFFD replacement char.
        assert_eq!(s, "\u{FFFD}");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn read_bounded_missing_file_errors() {
        let path =
            std::env::temp_dir().join(format!("loopdeck-lim-missing-{}", uuid::Uuid::new_v4()));
        assert!(read_bounded_to_string(&path, 1024).is_err());
    }
}
