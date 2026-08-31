//! Clean handoff record — `prd-verified-delivery-reconciliation.md` Phase 4,
//! loop `delivery-bookkeeping/clean-handoff`.
//!
//! Persisted once at delivery; consumed lazily. The record states that the
//! delivered branch + worktree are **retained for review** (PR stays
//! unmerged) and that the *next* run starts fresh from the repo's default
//! branch. Per the run's pre-answered clarification, nothing is created
//! eagerly at delivery: the `.loopdeck/runs/<next-branch>/` worktree comes
//! into existence only when the next loop starts (`commands::run_queue::
//! ensure_worktree` bases every new run branch on the default branch), and
//! the user is never switched off whatever they have checked out.

use crate::error::AppError;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// The single latest delivery handoff. Overwritten by each successful
/// delivery — it describes "where the last verified delivery landed and
/// where the next run starts from", not an audit log (that's
/// `execution.yaml` history + delivery links).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct HandoffRecord {
    /// Branch the delivery landed on (retained, PR unmerged).
    pub delivered_branch: String,
    /// Draft PR awaiting human review.
    pub pr_url: String,
    /// The retained managed worktree (`.loopdeck/runs/<branch>/`).
    pub worktree: PathBuf,
    /// Default branch the next run's branch is cut from.
    pub next_base: String,
    pub delivered_at: DateTime<Utc>,
}

pub fn handoff_path(repo_path: &Path) -> PathBuf {
    repo_path.join(".loopdeck").join("handoff.yaml")
}

pub fn load(repo_path: &Path) -> Result<Option<HandoffRecord>, AppError> {
    let path = handoff_path(repo_path);
    if !path.exists() {
        return Ok(None);
    }
    let contents = std::fs::read_to_string(&path)?;
    Ok(serde_yaml::from_str(&contents)?)
}

pub fn save(repo_path: &Path, record: &HandoffRecord) -> Result<(), AppError> {
    let path = handoff_path(repo_path);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&path, serde_yaml::to_string(record)?)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn record() -> HandoffRecord {
        HandoffRecord {
            delivered_branch: "run/x-abc".into(),
            pr_url: "https://github.com/o/r/pull/9".into(),
            worktree: PathBuf::from("/repo/.loopdeck/runs/run/x-abc"),
            next_base: "main".into(),
            delivered_at: Utc.with_ymd_and_hms(2026, 8, 31, 9, 0, 0).unwrap(),
        }
    }

    #[test]
    fn roundtrips_through_yaml() {
        let dir = std::env::temp_dir().join(format!("loopdeck-handoff-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();

        assert!(load(&dir).unwrap().is_none());

        save(&dir, &record()).unwrap();
        assert_eq!(load(&dir).unwrap(), Some(record()));

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn save_creates_the_loopdeck_dir() {
        let dir = std::env::temp_dir()
            .join(format!("loopdeck-handoff-nested-{}", uuid::Uuid::new_v4()))
            .join("repo");
        std::fs::create_dir_all(&dir).unwrap();

        save(&dir, &record()).unwrap();
        assert!(handoff_path(&dir).exists());

        std::fs::remove_dir_all(dir.parent().unwrap()).ok();
    }
}
