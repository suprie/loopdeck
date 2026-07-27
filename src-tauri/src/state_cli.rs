//! The `loopdeck state` subcommand — Phase 3 `state-command`.
//!
//! A narrow CLI that lets bundled skills/hooks complete, abandon, and promote
//! loops through the **same validated Rust domain** as the UI commands
//! (`commands::execution`) — never a free-form `.loopdeck/execution.yaml` edit.
//! It reuses `execution::load` / the `ExecutionState` transitions / `execution::save`
//! and `epic::find_loop_by_id`, so no parsing or transition rule is duplicated.
//!
//! Invoked when the binary's first argument is `state` (see `main.rs`):
//!
//! ```text
//! loopdeck state show [--path <dir>]
//! loopdeck state promote <loop-id> [--path <dir>]
//! loopdeck state complete [--commit <sha>] [--no-promote-next] [--path <dir>]
//! loopdeck state abandon --reason <text> [--no-promote-next] [--path <dir>]
//! loopdeck state migrate [--yes] [--path <dir>]
//! ```
//!
//! `migrate` (Phase 4) prints a read-only legacy→structured migration preview;
//! `--yes` performs it (writes `execution.yaml`, renames `loops.md` →
//! `loops.legacy.md`). It reuses the `migration` domain — no duplicated logic.
//!
//! `--path` defaults to the current directory (skills/hooks run from the
//! project root). Errors print to stderr; the process exits non-zero.

use crate::epic;
use crate::error::AppError;
use crate::execution::{self, GitEvidence, LoadedExecution, LoopOrigin};
use crate::migration::{self, MigrationPreview};
use chrono::Utc;
use std::path::PathBuf;

const USAGE: &str = "\
usage:
  loopdeck state show    [--path <dir>]
  loopdeck state promote <loop-id> [--path <dir>]
  loopdeck state complete [--commit <sha>] [--no-promote-next] [--path <dir>]
  loopdeck state abandon --reason <text> [--no-promote-next] [--path <dir>]
  loopdeck state migrate [--yes] [--path <dir>]

--path defaults to the current directory.";

/// Entry point for the `loopdeck state` subcommand. `args` is everything after
/// the literal `state` token. Returns the process exit code (0 = ok, 1 =
/// domain/IO error, 2 = bad usage).
pub fn run_state_cli(args: Vec<String>) -> i32 {
    match parse_state_cli(&args) {
        Ok(cmd) => match execute(&cmd) {
            Ok(()) => 0,
            Err(e) => {
                eprintln!("loopdeck state: {e}");
                1
            }
        },
        Err(usage) => {
            eprintln!("loopdeck state: {usage}");
            eprintln!("{USAGE}");
            2
        }
    }
}

/// A parsed `loopdeck state` subcommand.
#[derive(Debug, Clone, PartialEq, Eq)]
enum StateCli {
    Show {
        path: PathBuf,
    },
    Promote {
        id: String,
        path: PathBuf,
    },
    Complete {
        commit: Option<String>,
        promote_next: bool,
        path: PathBuf,
    },
    Abandon {
        reason: String,
        promote_next: bool,
        path: PathBuf,
    },
    /// Phase 4: migrate legacy `loops.md` → structured `execution.yaml`.
    /// `yes == false` prints a read-only preview; `true` performs the migration.
    Migrate {
        yes: bool,
        path: PathBuf,
    },
}

/// Parse the args that follow `state` into a [`StateCli`]. Pure (no I/O) so it
/// can be unit-tested directly. Flags accept both `--name value` and
/// `--name=value`.
fn parse_state_cli(args: &[String]) -> Result<StateCli, String> {
    let sub = args
        .first()
        .ok_or("missing subcommand (show|promote|complete|abandon|migrate)")?;
    let mut path: Option<PathBuf> = None;
    let mut commit: Option<String> = None;
    let mut reason: Option<String> = None;
    let mut promote_next = true;
    let mut yes = false;
    let mut positionals: Vec<String> = Vec::new();

    let mut i = 1;
    while i < args.len() {
        let arg = &args[i];
        let (name, inline) = match arg.split_once('=') {
            Some((n, v)) => (n, Some(v.to_string())),
            None => (arg.as_str(), None),
        };
        match name {
            "--path" => path = Some(PathBuf::from(take_value(name, inline, args, &mut i)?)),
            "--commit" => commit = Some(take_value(name, inline, args, &mut i)?),
            "--reason" => reason = Some(take_value(name, inline, args, &mut i)?),
            "--no-promote-next" => promote_next = false,
            "--promote-next" => promote_next = true,
            "--yes" | "-y" => yes = true,
            _ => positionals.push(arg.clone()),
        }
        i += 1;
    }

    let path = path.unwrap_or_else(|| PathBuf::from("."));
    match sub.as_str() {
        "show" => Ok(StateCli::Show { path }),
        "promote" => {
            let id = positionals
                .into_iter()
                .next()
                .ok_or("promote requires a <loop-id>")?;
            Ok(StateCli::Promote { id, path })
        }
        "complete" => Ok(StateCli::Complete {
            commit,
            promote_next,
            path,
        }),
        "abandon" => {
            let reason = reason.ok_or("abandon requires --reason <text>")?;
            Ok(StateCli::Abandon {
                reason,
                promote_next,
                path,
            })
        }
        "migrate" => Ok(StateCli::Migrate { yes, path }),
        other => Err(format!(
            "unknown subcommand \"{other}\" (expected show|promote|complete|abandon|migrate)"
        )),
    }
}

/// Resolve a flag's value: an inline `--name=value`, else consume the next arg.
fn take_value(
    name: &str,
    inline: Option<String>,
    args: &[String],
    i: &mut usize,
) -> Result<String, String> {
    if let Some(v) = inline {
        Ok(v)
    } else {
        *i += 1;
        args.get(*i)
            .cloned()
            .ok_or_else(|| format!("{name} requires a value"))
    }
}

/// Run a parsed command against the filesystem, reusing the execution domain.
fn execute(cmd: &StateCli) -> Result<(), String> {
    match cmd {
        StateCli::Show { path } => {
            let loaded = execution::load(path).map_err(fmt_err)?;
            print_summary(&loaded);
            for w in &loaded.warnings {
                eprintln!("warning: {w}");
            }
            Ok(())
        }
        StateCli::Promote { id, path } => {
            let loc = epic::find_loop_by_id(path, id).ok_or_else(|| {
                format!("no PRD checklist loop with id \"{id}\" under docs/epics/")
            })?;
            let loaded = execution::load(path).map_err(fmt_err)?;
            let origin = LoopOrigin {
                epic: loc.epic,
                prd: loc.prd,
                phase: loc.phase,
            };
            let next = loaded
                .state
                .promote_loop_into_current(id, &loc.title, origin, Utc::now())
                .map_err(fmt_err)?;
            let attempt = next.current.as_ref().map(|c| c.attempt).unwrap_or(0);
            execution::save(path, &next, loaded.state.revision).map_err(fmt_err)?;
            println!("promoted \"{id}\" (attempt {attempt})");
            Ok(())
        }
        StateCli::Complete {
            commit,
            promote_next,
            path,
        } => {
            let loaded = execution::load(path).map_err(fmt_err)?;
            let git = commit
                .as_deref()
                .map(|sha| {
                    crate::git::verified_commit(path, sha)
                        .map(|commit| GitEvidence { commit })
                        .map_err(AppError::ExecutionState)
                })
                .transpose()?;
            let next = loaded
                .state
                .complete_current(Utc::now(), git, *promote_next)
                .map_err(fmt_err)?;
            execution::save(path, &next, loaded.state.revision).map_err(fmt_err)?;
            println!("completed current loop (revision {})", next.revision);
            Ok(())
        }
        StateCli::Abandon {
            reason,
            promote_next,
            path,
        } => {
            let loaded = execution::load(path).map_err(fmt_err)?;
            let next = loaded
                .state
                .abandon_current(reason, Utc::now(), *promote_next)
                .map_err(fmt_err)?;
            execution::save(path, &next, loaded.state.revision).map_err(fmt_err)?;
            println!("abandoned current loop (revision {})", next.revision);
            Ok(())
        }
        StateCli::Migrate { yes, path } => {
            // Always compute + print the read-only preview first; --yes applies.
            let preview = migration::preview(path, Utc::now()).map_err(fmt_err)?;
            print_migration_preview(&preview);
            if !preview.available() {
                return Ok(());
            }
            if *yes {
                let state = migration::apply(path, Utc::now()).map_err(fmt_err)?;
                println!(
                    "migrated: execution.yaml written at revision {} \
                     (loops.md → loops.legacy.md)",
                    state.revision
                );
            } else {
                println!(
                    "\nre-run with --yes to apply. The original loops.md is preserved \
                     verbatim as loops.legacy.md."
                );
            }
            Ok(())
        }
    }
}

fn fmt_err(e: AppError) -> String {
    e.to_string()
}

/// Print a human-readable migration preview to stdout. Unmatched records are
/// listed so the user sees exactly what will NOT be carried into execution.yaml
/// (it stays in loops.legacy.md). Read-only — never writes.
fn print_migration_preview(p: &MigrationPreview) {
    if p.execution_yaml_present {
        println!("migration not available: execution.yaml already exists (already structured).");
        return;
    }
    if !p.loops_md_present {
        println!("migration not available: no .loopdeck/loops.md found.");
        return;
    }
    println!(
        "migration preview: current {} · {} history record(s) matched · {} unmatched · {} next-step(s) unconverted",
        if p.current_matched { "matched" } else { "unmatched or none" },
        p.matched_history,
        p.unmatched.len(),
        p.unconverted_next_steps.len(),
    );
    if !p.unmatched.is_empty() {
        println!("unmatched (preserved in loops.legacy.md, NOT written to execution.yaml):");
        for u in &p.unmatched {
            println!("  [{}] {} — {}", u.section, u.title, u.reason);
        }
    }
}

fn print_summary(loaded: &LoadedExecution) {
    let s = &loaded.state;
    println!(
        "execution.yaml @ revision {} ({:?})",
        s.revision, loaded.source
    );
    match &s.current {
        Some(c) => println!(
            "current: \"{}\" — {} (attempt {})",
            c.id, c.title, c.attempt
        ),
        None => println!("current: (none)"),
    }
    let done = s
        .history
        .iter()
        .filter(|h| h.outcome == crate::execution::Outcome::Completed)
        .count();
    println!(
        "queue: {} planned · history: {} finished ({} completed)",
        s.queue.len(),
        s.history.len(),
        done
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(x: &str) -> String {
        x.to_string()
    }

    #[test]
    fn parse_show_default_path() {
        assert_eq!(
            parse_state_cli(&[s("show")]).unwrap(),
            StateCli::Show {
                path: PathBuf::from(".")
            }
        );
    }

    #[test]
    fn parse_show_explicit_path() {
        assert_eq!(
            parse_state_cli(&[s("show"), s("--path"), s("/tmp/repo")]).unwrap(),
            StateCli::Show {
                path: PathBuf::from("/tmp/repo")
            }
        );
    }

    #[test]
    fn parse_path_inline_equals() {
        assert_eq!(
            parse_state_cli(&[s("show"), s("--path=/tmp/repo")]).unwrap(),
            StateCli::Show {
                path: PathBuf::from("/tmp/repo")
            }
        );
    }

    #[test]
    fn parse_promote_with_id() {
        assert_eq!(
            parse_state_cli(&[s("promote"), s("ns/loop-a"), s("--path"), s("/r")]).unwrap(),
            StateCli::Promote {
                id: s("ns/loop-a"),
                path: PathBuf::from("/r")
            }
        );
    }

    #[test]
    fn parse_complete_defaults_and_flags() {
        // Default: promote_next true, no commit.
        let (commit, promote_next, path) = match parse_state_cli(&[s("complete")]).unwrap() {
            StateCli::Complete {
                commit,
                promote_next,
                path,
            } => (commit, promote_next, path),
            _ => panic!("expected Complete"),
        };
        assert!(commit.is_none());
        assert!(promote_next);
        assert_eq!(path, PathBuf::from("."));

        // --commit + --no-promote-next.
        let (commit, promote_next) = match parse_state_cli(&[
            s("complete"),
            s("--commit"),
            s("abc123"),
            s("--no-promote-next"),
        ])
        .unwrap()
        {
            StateCli::Complete {
                commit,
                promote_next,
                path: _,
            } => (commit, promote_next),
            _ => panic!("expected Complete"),
        };
        assert_eq!(commit.as_deref(), Some("abc123"));
        assert!(!promote_next);
    }

    #[test]
    fn parse_abandon_requires_reason() {
        let err = parse_state_cli(&[s("abandon")]).unwrap_err();
        assert!(err.contains("--reason"));
        let parsed = parse_state_cli(&[s("abandon"), s("--reason"), s("blocked")]).unwrap();
        match parsed {
            StateCli::Abandon {
                reason,
                promote_next,
                path,
            } => {
                assert_eq!(reason, "blocked");
                assert!(promote_next);
                assert_eq!(path, PathBuf::from("."));
            }
            _ => panic!("expected Abandon"),
        }
    }

    #[test]
    fn parse_migrate_preview_and_yes() {
        // Preview (no --yes).
        match parse_state_cli(&[s("migrate"), s("--path"), s("/r")]).unwrap() {
            StateCli::Migrate { yes, path } => {
                assert!(!yes);
                assert_eq!(path, PathBuf::from("/r"));
            }
            _ => panic!("expected Migrate"),
        }
        // Apply (--yes), inline path.
        match parse_state_cli(&[s("migrate"), s("--yes"), s("--path=/r")]).unwrap() {
            StateCli::Migrate { yes, path } => {
                assert!(yes);
                assert_eq!(path, PathBuf::from("/r"));
            }
            _ => panic!("expected Migrate"),
        }
        // -y short form.
        match parse_state_cli(&[s("migrate"), s("-y")]).unwrap() {
            StateCli::Migrate { yes, path } => {
                assert!(yes);
                assert_eq!(path, PathBuf::from("."));
            }
            _ => panic!("expected Migrate"),
        }
    }

    #[test]
    fn parse_errors() {
        // No subcommand.
        assert!(parse_state_cli(&[]).is_err());
        // Unknown subcommand.
        let err = parse_state_cli(&[s("frobnicate")]).unwrap_err();
        assert!(err.contains("unknown subcommand"));
        // promote without an id.
        assert!(parse_state_cli(&[s("promote")]).is_err());
        // --path with no value.
        assert!(parse_state_cli(&[s("show"), s("--path")]).is_err());
        // --commit with no value.
        assert!(parse_state_cli(&[s("complete"), s("--commit")]).is_err());
    }

    #[test]
    fn run_state_cli_bad_usage_exits_2() {
        assert_eq!(run_state_cli(vec![s("frobnicate")]), 2);
        assert_eq!(run_state_cli(vec![]), 2);
    }

    /// `show` against a temp dir with no `execution.yaml` succeeds (loads the
    /// default) and exits 0 — the end-to-end CLI happy path through `execute`.
    #[test]
    fn run_state_cli_show_on_empty_dir_exits_0() {
        let dir = std::env::temp_dir().join(format!(
            "loopdeck-statecli-show-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let code = run_state_cli(vec![
            s("show"),
            s("--path"),
            dir.to_string_lossy().to_string(),
        ]);
        assert_eq!(
            code, 0,
            "show on a dir with no execution.yaml should load the default"
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
