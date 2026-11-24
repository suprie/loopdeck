//! Recovery tests exercise real persistence and linked worktrees without a live provider.

use crate::commands::run_queue::ensure_phase_worktree;
use crate::execution::{self, ExecutionState};
use crate::run_executor;
use crate::runplan::{self, RunPhaseStatus, StallPolicy};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

struct Repository {
    directory: PathBuf,
    root: PathBuf,
    worktree: PathBuf,
}

fn git(root: &Path, args: &[&str]) {
    let output = Command::new(crate::binary::git().unwrap())
        .arg("-C")
        .arg(root)
        .args(args)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

impl Repository {
    fn new() -> Self {
        let directory =
            std::env::temp_dir().join(format!("loopdeck-recovery-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&directory).unwrap();
        let directory = directory.canonicalize().unwrap();
        let root = directory.join("project");
        let worktree = directory.join("worker");
        fs::create_dir_all(&root).unwrap();
        git(&root, &["init", "-q"]);
        git(
            &root,
            &[
                "-c",
                "user.name=Recovery Test",
                "-c",
                "user.email=recovery@example.invalid",
                "-c",
                "commit.gpgsign=false",
                "commit",
                "--allow-empty",
                "-qm",
                "Initial",
            ],
        );
        git(
            &root,
            &[
                "worktree",
                "add",
                "-qb",
                "recovery-worker",
                worktree.to_str().unwrap(),
            ],
        );
        Self {
            directory,
            root,
            worktree,
        }
    }
}

impl Drop for Repository {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.directory);
    }
}

#[tokio::test]
async fn restart_continues_unfinished_phase_and_retains_worktree_changes() {
    let repo = Repository::new();
    let mut plan = run_executor::build_run_plan(
        "recovery-test".into(),
        repo.root.clone(),
        chrono::Utc::now(),
        &["sample/finished".into(), "sample/unfinished".into()],
        StallPolicy::ContinueIndependent,
        false,
        &[],
    );
    plan.phases[0].status = RunPhaseStatus::Completed;
    plan.phases[1].status = RunPhaseStatus::Running;
    plan.phases[1].attempt = 1;
    plan.phases[1].session_id = Some("codex:saved-thread".into());
    plan.phases[1].park_payload = Some("verification was interrupted".into());
    plan.environment.worktree_path = Some(repo.worktree.clone());
    plan.environment.worktree_branch = Some("recovery-worker".into());
    fs::write(
        repo.worktree.join("unfinished.rs"),
        "// implementation already written\n",
    )
    .unwrap();
    runplan::save(&repo.root, &plan).unwrap();

    // No in-memory executor/session is retained: reconstruct from central state.
    assert!(run_executor::reconcile_running_phases(&repo.worktree).unwrap());
    assert!(!run_executor::reconcile_running_phases(&repo.root).unwrap());
    let continued = runplan::load(&repo.root).unwrap().unwrap();
    assert_eq!(continued.phases[0].status, RunPhaseStatus::Completed);
    assert_eq!(continued.phases[1].status, RunPhaseStatus::Queued);
    assert_eq!(
        continued.phases[1].resume_session_id.as_deref(),
        Some("codex:saved-thread")
    );
    assert_eq!(
        continued.phases[1].park_payload.as_deref(),
        Some("verification was interrupted")
    );
    let (worktree, branch, created) = ensure_phase_worktree(&repo.root, &continued, 1).unwrap();
    assert_eq!(worktree, repo.worktree);
    assert_eq!(branch, "recovery-worker");
    assert!(!created);
    assert_eq!(
        fs::read_to_string(repo.worktree.join("unfinished.rs")).unwrap(),
        "// implementation already written\n"
    );
    assert!(!runplan::run_plan_path(&repo.root).exists());
    assert!(!runplan::run_plan_path(&repo.worktree).exists());
}

#[test]
fn first_read_from_worktree_imports_main_checkout_instead_of_stale_snapshot() {
    let repo = Repository::new();
    let mut main = run_executor::build_run_plan(
        "main-authority".into(),
        repo.root.clone(),
        chrono::Utc::now(),
        &["sample/main".into()],
        StallPolicy::ContinueIndependent,
        false,
        &[],
    );
    runplan::save_to_path(&runplan::run_plan_path(&repo.root), &main).unwrap();
    main.id = "stale-worktree-copy".into();
    runplan::save_to_path(&runplan::run_plan_path(&repo.worktree), &main).unwrap();
    let imported = runplan::load(&repo.worktree).unwrap().unwrap();
    assert_eq!(imported.id, "main-authority");
    assert_eq!(
        runplan::load(&repo.root).unwrap().unwrap().id,
        "main-authority"
    );
}

#[test]
fn nested_project_operational_state_is_independent_of_its_parent() {
    let repo = Repository::new();
    let nested = repo.root.join("packages/web");
    fs::create_dir_all(&nested).unwrap();
    let mut plan = run_executor::build_run_plan(
        "parent-run".into(),
        repo.root.clone(),
        chrono::Utc::now(),
        &["sample/parent".into()],
        StallPolicy::ContinueIndependent,
        false,
        &[],
    );
    runplan::save(&repo.root, &plan).unwrap();
    execution::save(&repo.root, &ExecutionState::default(), 0).unwrap();

    assert!(runplan::load(&nested).unwrap().is_none());
    assert!(!execution::load(&nested).unwrap().file_present);
    plan.id = "nested-run".into();
    plan.project = nested.clone();
    runplan::save(&nested, &plan).unwrap();

    assert_eq!(runplan::load(&nested).unwrap().unwrap().id, "nested-run");
    assert_eq!(
        runplan::load(&repo.worktree).unwrap().unwrap().id,
        "parent-run"
    );
    assert!(execution::load(&repo.worktree).unwrap().file_present);
}

#[test]
fn central_execution_revision_rejects_stale_writes_without_repository_files() {
    let repo = Repository::new();
    execution::save(&repo.root, &ExecutionState::default(), 0).unwrap();
    let initial = execution::load(&repo.worktree).unwrap();
    let mut newer = initial.state.clone();
    newer.revision = 1;
    execution::save(&repo.worktree, &newer, 0).unwrap();
    assert!(matches!(
        execution::save(&repo.root, &initial.state, 0),
        Err(crate::error::AppError::Conflict(_))
    ));
    assert_eq!(execution::load(&repo.root).unwrap().state.revision, 1);
    assert!(!execution::execution_path(&repo.root).exists());
    assert!(!execution::execution_path(&repo.worktree).exists());
}

#[test]
fn legacy_provider_checkpoint_uses_the_scheduler_session_field() {
    let phase: runplan::RunPhase = serde_yaml::from_str(
        "execution_id: sample/shared-loop\nprovider_session_id: codex:old-thread\n",
    )
    .unwrap();
    assert_eq!(phase.session_id.as_deref(), Some("codex:old-thread"));
}
