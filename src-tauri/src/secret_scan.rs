//! Staged-diff secret scan — `prd-unattended-ship.md` P1 ("pre-push secret
//! scan of the staged diff; a hit aborts the PR, parks the phase, flags the
//! report").
//!
//! The pattern set previously lived only inside `loopdeck-open-pr`'s
//! markdown as an inline `rg` pipeline run **after** `git push` — by the time
//! it ran, a real secret would already be on the remote, and nothing in CI
//! ever exercised it. It now has one real definition here (with an automated
//! test) and a `loopdeck secret-scan` CLI entry point the skill invokes
//! **before** staging is pushed, mirroring the `loopdeck state` convention of
//! routing skill-driven checks through validated Rust rather than
//! markdown-embedded shell.
//!
//! v1 deliberately keeps the same small, hand-rolled pattern set the skill
//! carried inline — broader detection (entropy scoring, a maintained
//! provider ruleset) is future work, not this pass's job.

use regex::Regex;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;

struct Pattern {
    name: &'static str,
    re: Regex,
}

/// Compiled once per process — building five patterns is not free and this
/// may run on every unattended push.
fn patterns() -> &'static [Pattern] {
    static PATTERNS: OnceLock<Vec<Pattern>> = OnceLock::new();
    PATTERNS.get_or_init(|| {
        vec![
            Pattern {
                name: "AWS access key ID",
                re: Regex::new(r"AKIA[0-9A-Z]{16}").unwrap(),
            },
            Pattern {
                name: "PEM private key",
                re: Regex::new(r"-----BEGIN (RSA |EC |OPENSSH )?PRIVATE KEY-----").unwrap(),
            },
            Pattern {
                name: "GitHub token",
                re: Regex::new(r"gh[pousr]_[A-Za-z0-9_]{20,}").unwrap(),
            },
            Pattern {
                name: "OpenAI-shaped secret key",
                re: Regex::new(r"sk-[A-Za-z0-9_-]{20,}").unwrap(),
            },
            Pattern {
                name: "generic key/secret/token assignment",
                re: Regex::new(r#"(?i)(api[_-]?key|secret|token)\s*[:=]\s*["'][^"']{12,}"#)
                    .unwrap(),
            },
        ]
    })
}

/// Return the first matching pattern's name and offending line, if any.
/// Scanned line-by-line so an abort report can point at a specific line
/// instead of a byte offset into a multi-file diff.
pub fn find_secret(diff: &str) -> Option<(&'static str, &str)> {
    diff.lines().find_map(|line| {
        patterns()
            .iter()
            .find(|pattern| pattern.re.is_match(line))
            .map(|pattern| (pattern.name, line))
    })
}

/// `git diff --cached` in `repo_path` via the vetted, resolved `git` binary
/// (same cwd-hijack defenses as every other git spawn in this codebase —
/// see `binary.rs`).
fn staged_diff(repo_path: &Path) -> Result<String, String> {
    let git = crate::binary::git().map_err(|e| e.to_string())?;
    let output = Command::new(git)
        .args(["-C"])
        .arg(repo_path)
        .args(["diff", "--cached"])
        .output()
        .map_err(|e| format!("failed to spawn git diff --cached: {e}"))?;
    if !output.status.success() {
        return Err(format!(
            "git diff --cached failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

fn parse_secret_scan_cli(args: &[String]) -> Result<PathBuf, String> {
    let mut path = PathBuf::from(".");
    let mut i = 0;
    while i < args.len() {
        let arg = &args[i];
        let (name, inline) = match arg.split_once('=') {
            Some((n, v)) => (n, Some(v.to_string())),
            None => (arg.as_str(), None),
        };
        match name {
            "--path" => {
                let value = if let Some(v) = inline {
                    v
                } else {
                    i += 1;
                    args.get(i)
                        .cloned()
                        .ok_or_else(|| "--path requires a value".to_string())?
                };
                path = PathBuf::from(value);
            }
            other => return Err(format!("unknown flag \"{other}\"")),
        }
        i += 1;
    }
    Ok(path)
}

/// Entry point for the `loopdeck secret-scan` subcommand (see `main.rs`).
/// `args` is everything after the literal `secret-scan` token.
///
/// Exit codes: `0` clean (safe to push), `1` a pattern matched — abort, the
/// offending line is on stderr, `2` the scan itself could not run (bad usage
/// or a git/IO error) — distinct from `1` so a caller can tell "found a
/// secret" from "couldn't check at all".
pub fn run_secret_scan_cli(args: Vec<String>) -> i32 {
    let path = match parse_secret_scan_cli(&args) {
        Ok(p) => p,
        Err(usage) => {
            eprintln!("loopdeck secret-scan: {usage}");
            eprintln!("usage: loopdeck secret-scan [--path <dir>]");
            return 2;
        }
    };
    let diff = match staged_diff(&path) {
        Ok(d) => d,
        Err(e) => {
            eprintln!("loopdeck secret-scan: {e}");
            return 2;
        }
    };
    match find_secret(&diff) {
        Some((name, line)) => {
            eprintln!("loopdeck secret-scan: possible {name} in staged diff: {line}");
            1
        }
        None => 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command as StdCommand;

    #[test]
    fn detects_aws_access_key() {
        let diff = "+aws_key = \"AKIAABCDEFGHIJKLMNOP\"";
        let (name, line) = find_secret(diff).expect("must match AWS key");
        assert_eq!(name, "AWS access key ID");
        assert_eq!(line, diff);
    }

    #[test]
    fn detects_pem_private_key_header() {
        let diff = "+-----BEGIN RSA PRIVATE KEY-----";
        let (name, _) = find_secret(diff).expect("must match PEM header");
        assert_eq!(name, "PEM private key");
    }

    #[test]
    fn detects_github_token() {
        let diff = "+token = \"ghp_1234567890abcdefghijklmnopqrstuvwx\"";
        let (name, _) = find_secret(diff).expect("must match GitHub token");
        // The generic assignment pattern also matches this line; either
        // named hit is a correct abort — accept both known matchers.
        assert!(name == "GitHub token" || name == "generic key/secret/token assignment");
    }

    #[test]
    fn detects_openai_shaped_key() {
        let diff = "+OPENAI_API_KEY=sk-abcdefghijklmnopqrstuvwx";
        assert!(find_secret(diff).is_some());
    }

    #[test]
    fn detects_generic_assignment() {
        let diff = "+const secret: \"correct-horse-battery-staple\"";
        let (name, _) = find_secret(diff).expect("must match generic assignment");
        assert_eq!(name, "generic key/secret/token assignment");
    }

    #[test]
    fn ignores_short_generic_value() {
        // Under the 12-char floor — not confident enough to be a real secret.
        let diff = "+let token = \"short\"";
        assert!(find_secret(diff).is_none());
    }

    #[test]
    fn clean_diff_has_no_match() {
        let diff = "+fn main() {\n+    println!(\"hello\");\n+}\n";
        assert!(find_secret(diff).is_none());
    }

    fn init_repo(dir: &Path) {
        for args in [
            vec!["init"],
            vec!["config", "user.email", "test@loopdeck.dev"],
            vec!["config", "user.name", "LoopDeck Test"],
        ] {
            StdCommand::new("git")
                .args(["-C"])
                .arg(dir)
                .args(args)
                .output()
                .unwrap();
        }
    }

    fn temp_repo(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "loopdeck-secretscan-{name}-{}-{}",
            std::process::id(),
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        init_repo(&dir);
        dir
    }

    /// The end-to-end abort path: a staged file containing a credential-shaped
    /// string makes `run_secret_scan_cli` exit `1` and name the pattern that
    /// tripped it — this is what the unattended `open-pr` path checks before
    /// it pushes or opens a draft PR.
    #[test]
    fn run_secret_scan_cli_aborts_on_staged_secret() {
        let dir = temp_repo("hit");
        std::fs::write(dir.join("config.env"), "AWS_KEY=AKIAABCDEFGHIJKLMNOP\n").unwrap();
        StdCommand::new("git")
            .args(["-C"])
            .arg(&dir)
            .args(["add", "."])
            .output()
            .unwrap();

        let code = run_secret_scan_cli(vec![
            "--path".to_string(),
            dir.to_string_lossy().to_string(),
        ]);
        assert_eq!(code, 1, "a staged secret must abort with exit code 1");

        std::fs::remove_dir_all(&dir).ok();
    }

    /// The safe path: a staged diff with no credential-shaped content exits
    /// `0`, so the unattended flow proceeds to commit + push.
    #[test]
    fn run_secret_scan_cli_clean_on_ordinary_staged_diff() {
        let dir = temp_repo("clean");
        std::fs::write(dir.join("README.md"), "# hello\n").unwrap();
        StdCommand::new("git")
            .args(["-C"])
            .arg(&dir)
            .args(["add", "."])
            .output()
            .unwrap();

        let code = run_secret_scan_cli(vec![
            "--path".to_string(),
            dir.to_string_lossy().to_string(),
        ]);
        assert_eq!(code, 0, "an ordinary staged diff must not abort");

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn run_secret_scan_cli_rejects_unknown_flag() {
        assert_eq!(run_secret_scan_cli(vec!["--bogus".to_string()]), 2);
    }
}
