//! Stable-ID-derived execution and delivery progress (milestone 0.2.1).
//!
//! This is a read model. Loop IDs remain the identity join. Structured records
//! are authoritative when present; checklist/history completion fills gaps for
//! spec loops that have no execution record yet.

use crate::epic::{self, Epic};
use crate::error::AppError;
use crate::execution::{self, ExecutionState, HistoryLoop, Outcome};
use crate::git::{self, CommitReachability};
use crate::persist;
use serde::Serialize;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ExecutionStatus {
    Planned,
    Queued,
    InProgress,
    Completed,
    Abandoned,
    Unmatched,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DeliveryStatus {
    Planned,
    Implemented,
    Committed,
    InReview,
    Shipped,
}

/// Optional remote enrichment boundary. Core progress never requires an
/// implementation: without one, the strongest provable local state wins.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct DeliveryEvidence {
    pub in_review: bool,
    pub shipped: bool,
}

pub trait DeliveryProvider {
    fn delivery_for_commit(
        &self,
        repo_path: &Path,
        commit: &str,
    ) -> Result<DeliveryEvidence, AppError>;
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct LoopProgress {
    pub execution: ExecutionStatus,
    pub delivery: DeliveryStatus,
    pub commit: Option<String>,
    pub commit_reachability: CommitReachability,
    pub discrepancy: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, PartialEq, Eq)]
pub struct ProgressCount {
    pub completed: usize,
    pub total: usize,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct UnmatchedExecution {
    pub id: String,
    pub title: String,
    pub execution: ExecutionStatus,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct ProgressSnapshot {
    pub loops: BTreeMap<String, LoopProgress>,
    /// Keyed by `<epic>/<prd>`.
    pub prds: BTreeMap<String, ProgressCount>,
    /// Keyed by epic slug.
    pub epics: BTreeMap<String, ProgressCount>,
    pub unmatched: Vec<UnmatchedExecution>,
    pub execution_file_present: bool,
}

#[derive(Debug, Clone)]
struct IndexedState {
    execution: ExecutionStatus,
    commit: Option<String>,
    title: String,
}

pub fn snapshot(repo_path: &Path) -> Result<ProgressSnapshot, AppError> {
    snapshot_with_provider(repo_path, None)
}

/// Build the progress read model with optional remote delivery enrichment.
///
/// `None` is the normal offline path. Provider failures are deliberately
/// non-fatal: the strongest locally provable delivery state remains visible.
pub fn snapshot_with_provider(
    repo_path: &Path,
    provider: Option<&dyn DeliveryProvider>,
) -> Result<ProgressSnapshot, AppError> {
    let epics = epic::parse_epics(repo_path);
    let loaded = execution::load(repo_path)?;
    if !loaded.file_present {
        return Ok(ProgressSnapshot {
            loops: BTreeMap::new(),
            prds: BTreeMap::new(),
            epics: BTreeMap::new(),
            unmatched: Vec::new(),
            execution_file_present: false,
        });
    }
    Ok(derive(repo_path, &epics, &loaded.state, provider))
}

fn derive(
    repo_path: &Path,
    epics: &[Epic],
    state: &ExecutionState,
    provider: Option<&dyn DeliveryProvider>,
) -> ProgressSnapshot {
    let index = execution_index(state);
    let mut loops = BTreeMap::new();
    let mut prds = BTreeMap::new();
    let mut epic_counts = BTreeMap::new();
    let mut spec_ids = HashSet::new();

    for epic in epics {
        let mut epic_progress = ProgressCount::default();
        for prd in &epic.prds {
            let mut prd_progress = ProgressCount::default();
            for phase in &prd.phases {
                for item in &phase.loops {
                    let Some(id) = item.id.as_deref() else {
                        continue;
                    };
                    spec_ids.insert(id.to_string());
                    prd_progress.total += 1;
                    let indexed = index.get(id);
                    let execution = indexed
                        .map(|value| value.execution)
                        .unwrap_or(ExecutionStatus::Planned);
                    let completed = indexed
                        .map(|_| execution == ExecutionStatus::Completed)
                        .unwrap_or(item.checked || item.done_in_history);
                    if completed {
                        prd_progress.completed += 1;
                    }
                    let commit = indexed.and_then(|value| value.commit.clone());
                    let commit_reachability =
                        git::commit_reachability(repo_path, commit.as_deref());
                    let local_delivery = match (execution, commit_reachability) {
                        (ExecutionStatus::Completed, CommitReachability::Reachable) => {
                            DeliveryStatus::Committed
                        }
                        (ExecutionStatus::Completed, _) => DeliveryStatus::Implemented,
                        _ => DeliveryStatus::Planned,
                    };
                    let delivery =
                        enrich_delivery(repo_path, commit.as_deref(), local_delivery, provider);
                    let discrepancy = indexed.and_then(|_| match (item.checked, execution) {
                        (true, status) if status != ExecutionStatus::Completed => Some(
                            "Authored checkbox is checked, but structured execution has not completed"
                                .to_string(),
                        ),
                        (false, ExecutionStatus::Completed) => Some(
                            "Implemented in structured execution; authored checkbox remains unchecked"
                                .to_string(),
                        ),
                        _ => None,
                    });
                    loops.insert(
                        id.to_string(),
                        LoopProgress {
                            execution,
                            delivery,
                            commit,
                            commit_reachability,
                            discrepancy,
                        },
                    );
                }
            }
            epic_progress.completed += prd_progress.completed;
            epic_progress.total += prd_progress.total;
            prds.insert(format!("{}/{}", epic.slug, prd.slug), prd_progress);
        }
        epic_counts.insert(epic.slug.clone(), epic_progress);
    }

    let mut unmatched: Vec<_> = index
        .into_iter()
        .filter(|(id, _)| !spec_ids.contains(id))
        .map(|(id, value)| UnmatchedExecution {
            id,
            title: value.title,
            execution: ExecutionStatus::Unmatched,
        })
        .collect();
    unmatched.sort_by(|a, b| a.id.cmp(&b.id));

    ProgressSnapshot {
        loops,
        prds,
        epics: epic_counts,
        unmatched,
        execution_file_present: true,
    }
}

fn enrich_delivery(
    repo_path: &Path,
    commit: Option<&str>,
    local: DeliveryStatus,
    provider: Option<&dyn DeliveryProvider>,
) -> DeliveryStatus {
    let (Some(provider), Some(commit), DeliveryStatus::Committed) = (provider, commit, local)
    else {
        return local;
    };
    let evidence = provider
        .delivery_for_commit(repo_path, commit)
        .unwrap_or_default();
    if evidence.shipped {
        DeliveryStatus::Shipped
    } else if evidence.in_review {
        DeliveryStatus::InReview
    } else {
        local
    }
}

fn execution_index(state: &ExecutionState) -> HashMap<String, IndexedState> {
    let mut index = HashMap::new();
    let mut history: Vec<&HistoryLoop> = state.history.iter().collect();
    history.sort_by_key(|item| (item.id.as_str(), item.attempt, item.completed_at));
    for item in history {
        index.insert(
            item.id.clone(),
            IndexedState {
                execution: match item.outcome {
                    Outcome::Completed => ExecutionStatus::Completed,
                    Outcome::Abandoned => ExecutionStatus::Abandoned,
                },
                commit: item.git.as_ref().map(|git| git.commit.clone()),
                title: item.title.clone(),
            },
        );
    }
    for item in &state.queue {
        let completed = index
            .get(&item.id)
            .is_some_and(|value| value.execution == ExecutionStatus::Completed);
        if !completed {
            index.insert(
                item.id.clone(),
                IndexedState {
                    execution: ExecutionStatus::Queued,
                    commit: None,
                    title: item.title.clone(),
                },
            );
        }
    }
    if let Some(item) = &state.current {
        let completed = index
            .get(&item.id)
            .is_some_and(|value| value.execution == ExecutionStatus::Completed);
        if !completed {
            index.insert(
                item.id.clone(),
                IndexedState {
                    execution: ExecutionStatus::InProgress,
                    commit: None,
                    title: item.title.clone(),
                },
            );
        }
    }
    index
}

pub fn export_markdown(repo_path: &Path) -> Result<PathBuf, AppError> {
    let epics = epic::parse_epics(repo_path);
    let snapshot = snapshot(repo_path)?;
    if !snapshot.execution_file_present {
        return Err(AppError::ExecutionState(
            "execution.yaml does not exist; migrate this project before exporting".to_string(),
        ));
    }
    let markdown = render_markdown(&epics, &snapshot);
    let path = repo_path.join(".loopdeck").join("execution-summary.md");
    persist::atomic_write(&path, &markdown)?;
    Ok(path)
}

fn render_markdown(epics: &[Epic], snapshot: &ProgressSnapshot) -> String {
    let mut out = String::from(
        "<!-- Generated by Selasar. Non-authoritative snapshot; do not use as execution state. -->\n\n# Execution Summary\n\n",
    );
    for epic in epics {
        let count = snapshot.epics.get(&epic.slug).cloned().unwrap_or_default();
        out.push_str(&format!(
            "## {} ({}/{})\n\n",
            epic.title, count.completed, count.total
        ));
        for prd in &epic.prds {
            let key = format!("{}/{}", epic.slug, prd.slug);
            let count = snapshot.prds.get(&key).cloned().unwrap_or_default();
            out.push_str(&format!(
                "### {} ({}/{})\n\n",
                prd.slug, count.completed, count.total
            ));
            for phase in &prd.phases {
                for item in &phase.loops {
                    let Some(id) = item.id.as_deref() else {
                        continue;
                    };
                    let status = snapshot
                        .loops
                        .get(id)
                        .map(|value| format!("{:?}", value.execution).to_lowercase())
                        .unwrap_or_else(|| "planned".to_string());
                    out.push_str(&format!("- `{id}` — {} — {status}\n", item.title));
                }
            }
            out.push('\n');
        }
    }
    if !snapshot.unmatched.is_empty() {
        out.push_str("## Unmatched execution records\n\n");
        for item in &snapshot.unmatched {
            out.push_str(&format!("- `{}` — {}\n", item.id, item.title));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::epic::{Prd, PrdLoop, PrdPhase};
    use crate::execution::{ActiveLoop, LoopOrigin};
    use chrono::Utc;

    fn epic_with_loop(id: &str, checked: bool) -> Epic {
        Epic {
            slug: "epic".into(),
            title: "Epic".into(),
            milestone: "0.2.1".into(),
            status: "in_progress".into(),
            description: String::new(),
            started: None,
            completed: None,
            owner: None,
            dir: "docs/epics/epic".into(),
            prds: vec![Prd {
                slug: "prd".into(),
                epic: "epic".into(),
                status: "accepted".into(),
                description: String::new(),
                milestone: Some("0.2.1".into()),
                file: "prd.md".into(),
                order: None,
                phases: vec![PrdPhase {
                    name: "Phase 1".into(),
                    loops: vec![PrdLoop {
                        title: "Renamable title".into(),
                        checked,
                        done_in_history: false,
                        id: Some(id.into()),
                    }],
                }],
            }],
        }
    }

    #[test]
    fn stable_id_not_title_drives_in_progress_and_discrepancy() {
        let epics = vec![epic_with_loop("prd/loop", true)];
        let state = ExecutionState {
            current: Some(ActiveLoop {
                id: "prd/loop".into(),
                title: "Old title".into(),
                origin: LoopOrigin {
                    epic: "epic".into(),
                    prd: "prd".into(),
                    phase: "Phase 1".into(),
                },
                started_at: Utc::now(),
                attempt: 1,
                delivery: None,
            }),
            ..ExecutionState::default()
        };
        let result = derive(Path::new("."), &epics, &state, None);
        let progress = &result.loops["prd/loop"];
        assert_eq!(progress.execution, ExecutionStatus::InProgress);
        assert!(progress.discrepancy.is_some());
        assert_eq!(result.prds["epic/prd"].total, 1);
        assert_eq!(result.prds["epic/prd"].completed, 0);
    }

    #[test]
    fn authored_completion_fills_missing_execution_record() {
        let epics = vec![epic_with_loop("prd/legacy-complete", true)];
        let result = derive(Path::new("."), &epics, &ExecutionState::default(), None);

        assert_eq!(result.prds["epic/prd"].total, 1);
        assert_eq!(result.prds["epic/prd"].completed, 1);
        assert_eq!(result.epics["epic"].completed, 1);
        assert_eq!(
            result.loops["prd/legacy-complete"].execution,
            ExecutionStatus::Planned
        );
        assert!(result.loops["prd/legacy-complete"].discrepancy.is_none());
    }

    #[test]
    fn runtime_id_absent_from_spec_is_unmatched() {
        let epics = vec![epic_with_loop("prd/known", false)];
        let state = ExecutionState {
            current: Some(ActiveLoop {
                id: "prd/missing".into(),
                title: "Removed from spec".into(),
                origin: LoopOrigin {
                    epic: "epic".into(),
                    prd: "prd".into(),
                    phase: "Phase 1".into(),
                },
                started_at: Utc::now(),
                attempt: 1,
                delivery: None,
            }),
            ..ExecutionState::default()
        };
        let result = derive(Path::new("."), &epics, &state, None);
        assert_eq!(result.unmatched.len(), 1);
        assert_eq!(result.unmatched[0].id, "prd/missing");
    }

    struct StaticDeliveryProvider(DeliveryEvidence);

    impl DeliveryProvider for StaticDeliveryProvider {
        fn delivery_for_commit(
            &self,
            _repo_path: &Path,
            _commit: &str,
        ) -> Result<DeliveryEvidence, AppError> {
            Ok(self.0)
        }
    }

    #[test]
    fn provider_can_enrich_committed_delivery_without_weakening_local_state() {
        let review = StaticDeliveryProvider(DeliveryEvidence {
            in_review: true,
            shipped: false,
        });
        let shipped = StaticDeliveryProvider(DeliveryEvidence {
            in_review: true,
            shipped: true,
        });
        let no_remote_evidence = StaticDeliveryProvider(DeliveryEvidence::default());

        assert_eq!(
            enrich_delivery(
                Path::new("."),
                Some("abc123"),
                DeliveryStatus::Committed,
                Some(&review),
            ),
            DeliveryStatus::InReview
        );
        assert_eq!(
            enrich_delivery(
                Path::new("."),
                Some("abc123"),
                DeliveryStatus::Committed,
                Some(&shipped),
            ),
            DeliveryStatus::Shipped
        );
        assert_eq!(
            enrich_delivery(
                Path::new("."),
                Some("abc123"),
                DeliveryStatus::Committed,
                Some(&no_remote_evidence),
            ),
            DeliveryStatus::Committed
        );
    }
}
