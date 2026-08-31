//! Recoverable delivery records + the idempotent retry —
//! `prd-verified-delivery-reconciliation.md` Phase 4, loop
//! `delivery-bookkeeping/retry-recovery`.
//!
//! When a draft-PR-authorized turn passes verification but the delivery
//! operation (commit / push / PR creation) fails, the executor persists a
//! [`DeliveryRetryRecord`] describing how far the delivery got and why it
//! stopped. One idempotent command ([`run_retry`]) resumes from the recorded
//! stage: re-detect the live stage from Git (the record may be stale), then
//! push → create the draft PR → finish the checklist/delivery bookkeeping.
//! Per the run's pre-answered clarification there is **no automatic retry** —
//! the record surfaces the reason and the next safe action in the UI, and a
//! human decides.
//!
//! `gh` is invoked via plain `Command::new("gh")` (PATH lookup, like
//! `git::open_pr_for_branch`) so tests stub it with a script dir on `PATH`.

use crate::delivery::{DeliveryLinks, RubricResult};
use crate::error::AppError;
use crate::execution;
use crate::git;
use crate::runplan::{self, RunPhaseStatus};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// How far a failed delivery got. Mirrors the PRD's three reportable states:
/// nothing mutated / committed / pushed.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DeliveryStage {
    /// The worktree still holds uncommitted changes — nothing was mutated in
    /// Git; the safe resume is a fresh agent turn (requeue).
    NothingMutated,
    /// A commit exists locally but the branch was never pushed.
    Committed,
    /// The branch is on the remote; only the PR (or the bookkeeping) is
    /// missing.
    Pushed,
}

impl DeliveryStage {
    /// The next safe action, as the UI states it (PRD: "offers the idempotent
    /// next action" — this is the sentence next to the Retry button).
    pub fn next_action(self) -> &'static str {
        match self {
            Self::NothingMutated => {
                "Work was never committed — retry requeues the phase for a fresh run; the worktree is untouched."
            }
            Self::Committed => {
                "Commit exists locally — retry pushes the branch to origin, opens the draft PR, and completes the bookkeeping."
            }
            Self::Pushed => {
                "Branch is already pushed — retry opens the draft PR (or adopts the existing one) and completes the bookkeeping."
            }
        }
    }
}

impl std::fmt::Display for DeliveryStage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::NothingMutated => "nothing mutated",
            Self::Committed => "committed",
            Self::Pushed => "pushed",
        })
    }
}

/// Detect the live stage of a delivery from the worktree's Git state. Pure
/// decision over observable facts: dirty tree → nothing was committed; clean
/// tree with HEAD absent from every remote ref → committed; HEAD on a remote
/// ref → pushed.
pub fn detect_stage(worktree: &Path) -> DeliveryStage {
    if !git::worktree_is_pristine(worktree) {
        return DeliveryStage::NothingMutated;
    }
    if git::head_on_remote(worktree) {
        DeliveryStage::Pushed
    } else {
        DeliveryStage::Committed
    }
}

/// The persisted recoverable delivery record (`.loopdeck/delivery-retry.yaml`).
/// Written by the executor at the failure site; cleared by a successful
/// retry. One record per project — the latest failed delivery.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DeliveryRetryRecord {
    /// The loops whose delivery stopped (execution IDs, re-resolved fresh at
    /// retry time — never trust a stale PRD location).
    pub execution_ids: Vec<String>,
    /// The run's branch (pushed or about to be).
    pub branch: String,
    /// The retained managed worktree the delivery ran in.
    pub worktree: PathBuf,
    /// Stage detected when the failure was recorded; re-detected live on
    /// every retry.
    pub stage: DeliveryStage,
    /// Why the delivery stopped, as reported to the user.
    pub reason: String,
    /// The rubric result the passing turn produced, retained so the retry's
    /// PR body carries the same evidence.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rubric: Option<RubricResult>,
    /// Human title for the PR (the delivered loop's title).
    pub pr_title: String,
    pub recorded_at: DateTime<Utc>,
}

pub fn retry_record_path(repo_path: &Path) -> PathBuf {
    repo_path.join(".loopdeck").join("delivery-retry.yaml")
}

pub fn load(repo_path: &Path) -> Result<Option<DeliveryRetryRecord>, AppError> {
    let path = retry_record_path(repo_path);
    if !path.exists() {
        return Ok(None);
    }
    Ok(serde_yaml::from_str(&std::fs::read_to_string(&path)?)?)
}

pub fn save(repo_path: &Path, record: &DeliveryRetryRecord) -> Result<(), AppError> {
    let path = retry_record_path(repo_path);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&path, serde_yaml::to_string(record)?)?;
    Ok(())
}

pub fn clear(repo_path: &Path) {
    let _ = std::fs::remove_file(retry_record_path(repo_path));
}

/// What one retry invocation did — the UI's toast and the report's outcome.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum RetryOutcome {
    /// Stage `nothing_mutated`: the recorded phases were requeued for a fresh
    /// agent turn (same mechanism as the morning report's retry).
    Requeued { execution_ids: Vec<String> },
    /// The delivery finished: PR exists (adopted or freshly created) and the
    /// bookkeeping (checklist, plan phases, delivery links) is complete.
    DeliveryCompleted {
        pr_url: String,
        resumed_from: DeliveryStage,
    },
    /// The retry itself failed; the record is updated (stage refreshed, new
    /// reason) and remains recoverable. Never leaves a half-mutated delivery.
    StillBlocked {
        stage: DeliveryStage,
        reason: String,
    },
}

/// Render the retry PR body from the retained rubric — the same per-criterion
/// evidence the original turn's PR body was required to carry.
fn render_pr_body(record: &DeliveryRetryRecord) -> String {
    let mut body = String::from(
        "## Retry delivery\n\n\
         This draft PR was opened by LoopDeck's idempotent delivery retry — \
         the original run passed verification but its delivery operation \
         stopped before the PR existed.\n\n",
    );
    body.push_str(&format!("- Loops: {}\n", record.execution_ids.join(", ")));
    if let Some(rubric) = &record.rubric {
        body.push_str(&format!(
            "\n## Verify Verdict\n\n**Verdict:** {}\n\n| # | Criterion | Status | Evidence |\n|---|-----------|--------|----------|\n",
            match rubric.verdict {
                crate::delivery::RubricVerdict::Pass => "PASS",
                crate::delivery::RubricVerdict::Warn => "WARN",
                crate::delivery::RubricVerdict::Fail => "BLOCK",
            }
        ));
        for (i, criterion) in rubric.criteria.iter().enumerate() {
            body.push_str(&format!(
                "| {} | {} | {} | {} |\n",
                i + 1,
                criterion.criterion,
                match criterion.status {
                    crate::delivery::CriterionStatus::Pass => "PASS",
                    crate::delivery::CriterionStatus::Partial => "PARTIAL",
                    crate::delivery::CriterionStatus::Fail => "FAIL",
                },
                criterion.note,
            ));
        }
    }
    body
}

/// Create a draft PR via `gh`. PATH lookup keeps this stubbable in tests.
/// Returns the new PR's URL, or `None` when `gh` failed or emitted no URL.
fn create_draft_pr(record: &DeliveryRetryRecord) -> Option<String> {
    let output = std::process::Command::new("gh")
        .args([
            "pr",
            "create",
            "--draft",
            "--title",
            &record.pr_title,
            "--body",
            &render_pr_body(record),
            "--head",
            &record.branch,
        ])
        .current_dir(&record.worktree)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(str::trim)
        .find(|line| line.starts_with("https://") && line.contains("/pull/"))
        .map(str::to_string)
}

/// The idempotent retry. Resumes from the *live* stage (the record's stage is
/// advisory; Git is the truth), so a retry that half-completed yesterday and
/// a retry clicked twice in a row both converge to the same end state:
/// PR exists + checklist checked + delivery links persisted + record cleared.
pub fn run_retry(root: &Path) -> Result<RetryOutcome, AppError> {
    let Some(mut record) = load(root)? else {
        return Err(AppError::Conflict(
            "no recoverable delivery to retry — the last delivery either completed or never started".into(),
        ));
    };
    let live_stage = detect_stage(&record.worktree);
    record.stage = live_stage;

    // Stage `nothing_mutated`: nothing in Git to deliver. The safe resume is
    // a fresh agent turn — requeue the recorded phases, same as the morning
    // report's phase retry.
    if live_stage == DeliveryStage::NothingMutated {
        let mut requeued = Vec::new();
        if let Some(mut plan) = runplan::load(root)? {
            for execution_id in &record.execution_ids {
                if crate::commands::run_queue::requeue_terminal_phase(&mut plan, execution_id)
                    .is_ok()
                {
                    requeued.push(execution_id.clone());
                }
            }
            runplan::save(root, &plan)?;
        }
        clear(root);
        return Ok(RetryOutcome::Requeued {
            execution_ids: requeued,
        });
    }

    // Stage `committed`: push first. A push failure leaves everything exactly
    // as it was — still recoverable from the same stage.
    if live_stage == DeliveryStage::Committed {
        if let Err(error) = git::push_branch(&record.worktree, &record.branch) {
            record.reason = format!("push failed: {error}");
            save(root, &record)?;
            return Ok(RetryOutcome::StillBlocked {
                stage: DeliveryStage::Committed,
                reason: record.reason.clone(),
            });
        }
        record.stage = DeliveryStage::Pushed;
    }

    // PR boundary: adopt an already-open PR before creating one (the crash
    // path — PR created, bookkeeping never ran).
    let url = git::open_pr_for_branch(&record.worktree, &record.branch)
        .or_else(|| create_draft_pr(&record));
    let Some(url) = url else {
        record.reason =
            "PR creation failed (gh errored or emitted no PR URL); the branch is pushed and intact"
                .to_string();
        save(root, &record)?;
        return Ok(RetryOutcome::StillBlocked {
            stage: DeliveryStage::Pushed,
            reason: record.reason.clone(),
        });
    };

    complete_bookkeeping(root, &record, &url)?;
    clear(root);
    Ok(RetryOutcome::DeliveryCompleted {
        pr_url: url,
        resumed_from: live_stage,
    })
}

/// the executor's success path performs, in the same order, all idempotent:
/// checklist items first (`complete_prd_loop` never reopens a checked item),
/// then plan phases, then the terminal delivery links on the execution record.
fn complete_bookkeeping(
    root: &Path,
    record: &DeliveryRetryRecord,
    url: &str,
) -> Result<(), AppError> {
    for execution_id in &record.execution_ids {
        if let Some(location) = crate::epic::find_loop_by_id(root, execution_id) {
            crate::epic::complete_prd_loop(root, &location.epic, &location.prd, &location.title)?;
        }
    }

    if let Some(mut plan) = runplan::load(root)? {
        let mut changed = false;
        for phase in &mut plan.phases {
            if record.execution_ids.contains(&phase.execution_id)
                && !matches!(
                    phase.status,
                    RunPhaseStatus::Completed | RunPhaseStatus::Delivered
                )
            {
                phase.status = if crate::epic::find_loop_by_id(root, &phase.execution_id).is_some()
                {
                    RunPhaseStatus::Completed
                } else {
                    RunPhaseStatus::Delivered
                };
                phase.park_payload = Some(format!("draft PR: {url}"));
                changed = true;
            }
        }
        if changed {
            runplan::save(root, &plan)?;
        }
    }

    let links = DeliveryLinks {
        branch: Some(record.branch.clone()),
        commit: git::head_commit(&record.worktree),
        pr_url: Some(url.to_string()),
        pr_provider: url.contains("github.com").then(|| "github".to_string()),
        rubric: record.rubric.clone(),
    };
    let loaded = execution::load(root)?;
    let state = loaded.state.record_recovered_delivery(
        record.execution_ids.as_slice(),
        links,
        Utc::now(),
    )?;
    execution::save(root, &state, loaded.state.revision)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use std::sync::Mutex;

    /// `gh` is stubbed by prepending a script dir to `PATH`; the script's
    /// behavior is selected by process env vars. Both are process-global, so
    /// every stub-using test holds this lock for its duration.
    static GH_LOCK: Mutex<()> = Mutex::new(());

    const LOOP_ID: &str = "test-epic/loop-1";
    const BRANCH: &str = "run/loop-1-abc";

    fn git(root: &Path, args: &[&str]) {
        let output = std::process::Command::new("git")
            .args(["-C"])
            .arg(root)
            .args(args)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .output()
            .expect("git spawns");
        assert!(
            output.status.success(),
            "git {:?} failed: {}",
            args,
            String::from_utf8_lossy(&output.stderr)
        );
    }

    /// A repo with a `main` branch, a PRD checklist loop, a bare `origin`
    /// with `main` pushed, and a run worktree on `BRANCH` holding one commit.
    struct Fixture {
        root: PathBuf,
        origin: PathBuf,
        worktree: PathBuf,
    }

    fn fixture(name: &str) -> Fixture {
        let base = std::env::temp_dir().join(format!(
            "loopdeck-retry-{name}-{}-{}",
            std::process::id(),
            uuid::Uuid::new_v4()
        ));
        let root = base.join("repo");
        let origin = base.join("origin.git");
        let worktree = root.join(".loopdeck").join("runs").join(BRANCH);
        std::fs::create_dir_all(&root).unwrap();

        git(&root, &["init", "-b", "main"]);
        git(&root, &["config", "user.email", "test@loopdeck.dev"]);
        git(&root, &["config", "user.name", "LoopDeck Test"]);
        std::fs::write(root.join("README.md"), "# test\n").unwrap();
        // The PRD checklist item the delivery bookkeeping must check. The
        // epic needs a README (parse_epics skips a directory without one).
        std::fs::create_dir_all(root.join("docs/epics/test-epic")).unwrap();
        std::fs::write(
            root.join("docs/epics/test-epic/README.md"),
            "---\ntitle: Test Epic\nslug: test-epic\nmilestone: \"unassigned\"\n\
             status: proposed\ndescription: test\n---\n\n# Test Epic\n",
        )
        .unwrap();
        std::fs::write(
            root.join("docs/epics/test-epic/prd-test.md"),
            "---\nprd: prd-test\nepic: test-epic\nstatus: proposed\ndescription: d\n---\n\n\
             # Test\n\n## Phases\n\n### Phase 1 — Build\n\
             - [ ] `test-epic/loop-1` Do the thing\n",
        )
        .unwrap();
        git(&root, &["add", "."]);
        git(&root, &["commit", "-m", "initial"]);

        std::process::Command::new("git")
            .args(["init", "--bare", "-b", "main"])
            .arg(&origin)
            .output()
            .expect("bare init");
        git(
            &root,
            &["remote", "add", "origin", origin.to_str().unwrap()],
        );
        git(&root, &["push", "-u", "origin", "main"]);

        git(
            &root,
            &[
                "worktree",
                "add",
                "-b",
                BRANCH,
                worktree.to_str().unwrap(),
                "main",
            ],
        );
        std::fs::write(worktree.join("README.md"), "# delivered\n").unwrap();
        git(&worktree, &["add", "."]);
        git(
            &worktree,
            &[
                "commit",
                "-m",
                "delivered work\n\nRubric: PASS — 1/1 criteria",
            ],
        );

        Fixture {
            root,
            origin,
            worktree,
        }
    }

    impl Fixture {
        /// Persist the failed-delivery state exactly as the executor leaves
        /// it: plan phase `Parked`, execution loop abandoned, retry record on
        /// disk.
        fn record_failure(&self, stage: DeliveryStage, pushed: bool) -> DeliveryRetryRecord {
            if pushed {
                git(&self.worktree, &["push", "-u", "origin", BRANCH]);
            }
            let mut plan = crate::run_executor::build_run_plan(
                format!("run-{}", uuid::Uuid::new_v4()),
                self.root.clone(),
                Utc::now(),
                &[LOOP_ID.to_string()],
                crate::runplan::StallPolicy::Halt,
                true,
            );
            plan.phases[0].status = RunPhaseStatus::Parked;
            plan.phases[0].park_payload = Some("no draft PR URL".into());
            runplan::save(&self.root, &plan).unwrap();

            let state = execution::ExecutionState::default()
                .promote_loop_into_current(
                    LOOP_ID,
                    "Do the thing",
                    crate::execution::LoopOrigin {
                        epic: "test-epic".into(),
                        prd: "prd-test".into(),
                        phase: "Phase 1 — Build".into(),
                    },
                    Utc::now(),
                )
                .unwrap()
                .abandon_current("no draft PR URL", Utc::now(), false)
                .unwrap();
            // Save against the pre-transition revision, exactly as the
            // executor does (load → transition → save with loaded revision).
            let prior_revision = execution::ExecutionState::default().revision;
            execution::save(&self.root, &state, prior_revision).unwrap();

            let record = DeliveryRetryRecord {
                execution_ids: vec![LOOP_ID.to_string()],
                branch: BRANCH.to_string(),
                worktree: self.worktree.clone(),
                stage,
                reason: "no draft PR URL was recorded".into(),
                rubric: None,
                pr_title: "Do the thing".into(),
                recorded_at: Utc::now(),
            };
            save(&self.root, &record).unwrap();
            record
        }

        fn prd_checked(&self) -> Option<bool> {
            crate::epic::loop_checked(&self.root, LOOP_ID)
        }

        fn phase_status(&self) -> RunPhaseStatus {
            runplan::load(&self.root).unwrap().unwrap().phases[0].status
        }

        fn history_outcome(&self) -> Option<(crate::execution::Outcome, Option<DeliveryLinks>)> {
            execution::load(&self.root)
                .unwrap()
                .state
                .history
                .last()
                .map(|record| (record.outcome.clone(), record.delivery.clone()))
        }
    }

    /// Write the stub `gh` script and return its directory.
    fn stub_gh_dir() -> PathBuf {
        let dir = std::env::temp_dir().join(format!("loopdeck-gh-stub-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let script = dir.join("gh");
        std::fs::write(
            &script,
            "#!/bin/sh\n\
             case \"$2\" in\n\
               list)\n\
                 [ -n \"$GH_STUB_LIST\" ] && printf '%s' \"$GH_STUB_LIST\"\n\
                 exit 0\n\
                 ;;\n\
               create)\n\
                 if [ -n \"$GH_STUB_CREATE_URL\" ]; then echo \"$GH_STUB_CREATE_URL\"; exit 0; fi\n\
                 echo 'gh: simulated create failure' >&2\n\
                 exit 1\n\
                 ;;\n\
             esac\n\
             exit 0\n",
        )
        .unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        dir
    }

    /// Run `body` with the stub `gh` first on `PATH`. Serialized by GH_LOCK —
    /// env and PATH are process-global.
    fn with_stub_gh(body: impl FnOnce() + std::panic::UnwindSafe) {
        let _guard = GH_LOCK.lock().unwrap();
        let stub_dir = stub_gh_dir();
        let old_path = std::env::var("PATH").unwrap_or_default();
        let old_list = std::env::var("GH_STUB_LIST").ok();
        let old_url = std::env::var("GH_STUB_CREATE_URL").ok();
        std::env::set_var("PATH", format!("{}:{}", stub_dir.display(), old_path));
        std::env::remove_var("GH_STUB_LIST");
        std::env::remove_var("GH_STUB_CREATE_URL");

        let result = std::panic::catch_unwind(body);

        std::env::set_var("PATH", old_path);
        match old_list {
            Some(value) => std::env::set_var("GH_STUB_LIST", value),
            None => std::env::remove_var("GH_STUB_LIST"),
        }
        match old_url {
            Some(value) => std::env::set_var("GH_STUB_CREATE_URL", value),
            None => std::env::remove_var("GH_STUB_CREATE_URL"),
        }
        std::fs::remove_dir_all(&stub_dir).ok();
        // Drop the lock *before* resuming the panic so one failing test does
        // not poison the lock for the others.
        drop(_guard);
        if let Err(panic) = result {
            std::panic::resume_unwind(panic);
        }
    }

    // ── stage detection ──────────────────────────────────────────────

    #[test]
    fn detect_stage_reads_the_three_delivery_stages() {
        let f = fixture("stages");

        // Committed: clean tree, nothing on a remote.
        assert_eq!(detect_stage(&f.worktree), DeliveryStage::Committed);

        // Pushed: same commit now on a remote-tracking ref.
        git(&f.worktree, &["push", "-u", "origin", BRANCH]);
        assert_eq!(detect_stage(&f.worktree), DeliveryStage::Pushed);

        // Nothing mutated: uncommitted work in the tree wins over everything.
        std::fs::write(f.worktree.join("notes.md"), "uncommitted\n").unwrap();
        assert_eq!(detect_stage(&f.worktree), DeliveryStage::NothingMutated);

        std::fs::remove_dir_all(f.root.parent().unwrap()).ok();
    }

    #[test]
    fn retry_without_a_record_is_a_conflict() {
        let f = fixture("no-record");
        assert!(run_retry(&f.root).is_err());
        std::fs::remove_dir_all(f.root.parent().unwrap()).ok();
    }

    #[test]
    fn retry_record_roundtrips_through_yaml() {
        let dir =
            std::env::temp_dir().join(format!("loopdeck-retry-yaml-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        assert!(load(&dir).unwrap().is_none());

        let record = DeliveryRetryRecord {
            execution_ids: vec![LOOP_ID.to_string()],
            branch: BRANCH.to_string(),
            worktree: dir.join(".loopdeck").join("runs").join(BRANCH),
            stage: DeliveryStage::Pushed,
            reason: "gh failed".into(),
            rubric: None,
            pr_title: "Do the thing".into(),
            recorded_at: Utc::now(),
        };
        save(&dir, &record).unwrap();
        assert_eq!(load(&dir).unwrap(), Some(record));
        clear(&dir);
        assert!(load(&dir).unwrap().is_none());

        std::fs::remove_dir_all(&dir).ok();
    }

    // ── success: adopt an existing PR ────────────────────────────────

    #[test]
    fn retry_adopts_an_existing_pr_and_completes_the_bookkeeping() {
        with_stub_gh(|| {
            let f = fixture("adopt");
            f.record_failure(DeliveryStage::Pushed, true);
            std::env::set_var(
                "GH_STUB_LIST",
                r#"[{"url":"https://github.com/o/r/pull/7"}]"#,
            );

            let outcome = run_retry(&f.root).unwrap();
            assert_eq!(
                outcome,
                RetryOutcome::DeliveryCompleted {
                    pr_url: "https://github.com/o/r/pull/7".into(),
                    resumed_from: DeliveryStage::Pushed,
                }
            );

            // Bookkeeping: checklist checked, phase completed, history record
            // flipped Abandoned → Completed with full delivery links.
            assert_eq!(f.prd_checked(), Some(true));
            assert_eq!(f.phase_status(), RunPhaseStatus::Completed);
            let (outcome, links) = f.history_outcome().unwrap();
            assert_eq!(outcome, crate::execution::Outcome::Completed);
            let links = links.expect("delivery links recorded");
            assert_eq!(links.branch.as_deref(), Some(BRANCH));
            assert_eq!(
                links.pr_url.as_deref(),
                Some("https://github.com/o/r/pull/7")
            );

            // Idempotent end state: the record is gone, so a second click has
            // nothing to retry.
            assert!(load(&f.root).unwrap().is_none());
            assert!(matches!(run_retry(&f.root), Err(AppError::Conflict(_))));

            std::fs::remove_dir_all(f.root.parent().unwrap()).ok();
        });
    }

    // ── success: create the missing draft PR ─────────────────────────

    #[test]
    fn retry_creates_the_draft_pr_when_none_exists() {
        with_stub_gh(|| {
            let f = fixture("create");
            f.record_failure(DeliveryStage::Pushed, true);
            // `pr list` empty; `pr create` succeeds and prints the URL.
            std::env::set_var("GH_STUB_CREATE_URL", "https://github.com/o/r/pull/9");

            let outcome = run_retry(&f.root).unwrap();
            assert_eq!(
                outcome,
                RetryOutcome::DeliveryCompleted {
                    pr_url: "https://github.com/o/r/pull/9".into(),
                    resumed_from: DeliveryStage::Pushed,
                }
            );
            assert_eq!(f.prd_checked(), Some(true));
            assert!(load(&f.root).unwrap().is_none());

            std::fs::remove_dir_all(f.root.parent().unwrap()).ok();
        });
    }

    // ── success: push the committed branch first ─────────────────────

    #[test]
    fn retry_pushes_a_committed_branch_then_completes() {
        with_stub_gh(|| {
            let f = fixture("push");
            f.record_failure(DeliveryStage::Committed, false);
            std::env::set_var("GH_STUB_CREATE_URL", "https://github.com/o/r/pull/11");

            let outcome = run_retry(&f.root).unwrap();
            assert_eq!(
                outcome,
                RetryOutcome::DeliveryCompleted {
                    pr_url: "https://github.com/o/r/pull/11".into(),
                    resumed_from: DeliveryStage::Committed,
                }
            );

            // The branch really landed on the bare remote.
            let output = std::process::Command::new("git")
                .args(["--git-dir"])
                .arg(&f.origin)
                .args(["branch", "--list", BRANCH])
                .output()
                .unwrap();
            assert!(
                String::from_utf8_lossy(&output.stdout).contains(BRANCH),
                "branch must exist on the remote after retry"
            );
            assert_eq!(detect_stage(&f.worktree), DeliveryStage::Pushed);

            std::fs::remove_dir_all(f.root.parent().unwrap()).ok();
        });
    }

    // ── PR-creation failure: record survives, stage advanced ─────────

    #[test]
    fn retry_keeps_the_record_when_pr_creation_fails() {
        with_stub_gh(|| {
            let f = fixture("gh-fails");
            f.record_failure(DeliveryStage::Pushed, true);
            // No GH_STUB_LIST / GH_STUB_CREATE_URL: list → empty, create →
            // non-zero exit.

            let outcome = run_retry(&f.root).unwrap();
            let RetryOutcome::StillBlocked { stage, reason } = outcome else {
                panic!("expected StillBlocked, got {outcome:?}");
            };
            assert_eq!(stage, DeliveryStage::Pushed);
            assert!(reason.contains("PR creation failed"));

            // Nothing was completed and the record stays recoverable, now
            // describing the live (pushed) stage.
            assert_eq!(f.prd_checked(), Some(false));
            assert_eq!(f.phase_status(), RunPhaseStatus::Parked);
            let record = load(&f.root).unwrap().expect("record retained");
            assert_eq!(record.stage, DeliveryStage::Pushed);

            std::fs::remove_dir_all(f.root.parent().unwrap()).ok();
        });
    }

    // ── push failure: record survives at the committed stage ─────────

    #[test]
    fn retry_keeps_the_record_when_push_fails() {
        with_stub_gh(|| {
            let f = fixture("push-fails");
            f.record_failure(DeliveryStage::Committed, false);
            // Break the remote: push must fail without mutating anything.
            std::fs::remove_dir_all(&f.origin).unwrap();

            let outcome = run_retry(&f.root).unwrap();
            let RetryOutcome::StillBlocked { stage, reason } = outcome else {
                panic!("expected StillBlocked, got {outcome:?}");
            };
            assert_eq!(stage, DeliveryStage::Committed);
            assert!(reason.contains("push failed"));

            let record = load(&f.root).unwrap().expect("record retained");
            assert_eq!(record.stage, DeliveryStage::Committed);
            assert_eq!(f.prd_checked(), Some(false));

            std::fs::remove_dir_all(f.root.parent().unwrap()).ok();
        });
    }

    // ── nothing mutated: requeue for a fresh agent turn ──────────────

    #[test]
    fn retry_requeues_phases_when_nothing_was_mutated() {
        let f = fixture("requeue");
        f.record_failure(DeliveryStage::NothingMutated, false);
        std::fs::write(f.worktree.join("wip.md"), "uncommitted\n").unwrap();

        let outcome = run_retry(&f.root).unwrap();
        assert_eq!(
            outcome,
            RetryOutcome::Requeued {
                execution_ids: vec![LOOP_ID.to_string()],
            }
        );
        assert_eq!(f.phase_status(), RunPhaseStatus::Queued);
        assert!(load(&f.root).unwrap().is_none(), "record consumed");
        // The worktree is untouched — still dirty, nothing lost.
        assert_eq!(detect_stage(&f.worktree), DeliveryStage::NothingMutated);

        std::fs::remove_dir_all(f.root.parent().unwrap()).ok();
    }

    // ── failing rubric: a non-passing verifier report blocks delivery ─

    #[test]
    fn a_failing_rubric_report_blocks_the_delivery_gates() {
        // The exact shape of a `loopdeck-prd-verifier` final report whose
        // rubric did not pass, fed through the same extract → gate pipeline
        // the executor runs on a turn's final response.
        let report = "## PRD Verification — prd-test\n\n\
                      **Verdict:** BLOCK\n\n\
                      | # | Criterion | Status | Evidence |\n\
                      |---|-----------|--------|----------|\n\
                      | 1 | Do the thing | FAIL | no code found |\n";
        let rubric = crate::delivery::extract_rubric_result(report, Utc::now()).expect("parses");
        assert!(!rubric.all_pass());
        let blocks =
            crate::delivery::evaluate_delivery_gates(Some(true), true, true, Some(&rubric));
        assert_eq!(blocks, vec![crate::delivery::GateBlock::RubricNotPassing]);
    }

    // ── cross-branch: a fix on another branch is not this delivery ────

    #[test]
    fn a_pushed_fix_on_a_different_branch_does_not_read_as_pushed() {
        let f = fixture("cross-branch");
        // Push main (a "fix" landed elsewhere); the run branch stays local.
        git(&f.root, &["push", "origin", "main:other-fix"]);

        assert_eq!(
            detect_stage(&f.worktree),
            DeliveryStage::Committed,
            "a remote ref that does not contain the run branch's HEAD must not read as pushed"
        );

        std::fs::remove_dir_all(f.root.parent().unwrap()).ok();
    }
}
