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

const HANDOFFS_DIR: &str = "handoffs";
const AGENT_ARTIFACTS_DIR: &str = "handoff-artifacts";
const MAX_FRONTMATTER_BYTES: usize = 1024;
const MAX_BODY_BYTES: usize = 8 * 1024;

/// The kinds of contract artifacts that can cross a phase boundary.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ArtifactType {
    Plan,
    Analysis,
    Decision,
    Report,
    Content,
}

/// Machine-readable metadata carried by an artifact's YAML frontmatter.
/// `path` is the durable path in the project's handoff store and is not
/// duplicated in frontmatter.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ArtifactRecord {
    pub artifact: String,
    pub author_role: String,
    pub phase: String,
    #[serde(rename = "type")]
    pub artifact_type: ArtifactType,
    pub created: chrono::NaiveDate,
    pub summary: String,
    #[serde(default)]
    pub cites: Vec<String>,
    #[serde(skip)]
    pub path: PathBuf,
}

/// A validated handoff artifact, including the body that follows its
/// frontmatter. Artifacts are immutable once persisted: a duplicate topic is
/// an error so a retry can never overwrite the original handoff.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Artifact {
    pub record: ArtifactRecord,
    pub body: String,
}

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

/// The persistent, cross-run artifact store for this project.
pub fn store_dir(repo_path: &Path) -> PathBuf {
    repo_path.join(".loopdeck").join(HANDOFFS_DIR)
}

/// Stable topic slug used by the executor when it asks an agent to emit a
/// phase artifact. The full execution id is retained, with separators mapped
/// to hyphens so it cannot escape the store directory.
pub fn topic_for_phase(phase: &str) -> String {
    phase
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() {
                character.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect::<String>()
        .split('-')
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join("-")
}

pub fn artifact_path(repo_path: &Path, topic: &str) -> Result<PathBuf, AppError> {
    let topic = topic_for_phase(topic);
    if topic.is_empty() {
        return Err(AppError::Conflict("handoff topic cannot be empty".into()));
    }
    Ok(store_dir(repo_path).join(format!("{topic}.md")))
}

/// Locate the agent-authored source in an isolated run worktree. The first
/// location is the executor's staging convention; the second is accepted for
/// agents that follow the public store path literally. Neither location is
/// ever used as the persistent destination in the main checkout.
pub fn agent_artifact_path(worktree: &Path, phase: &str) -> Result<PathBuf, AppError> {
    let topic = topic_for_phase(phase);
    if topic.is_empty() {
        return Err(AppError::Conflict("handoff topic cannot be empty".into()));
    }
    let staging = worktree
        .join(".loopdeck")
        .join(AGENT_ARTIFACTS_DIR)
        .join(format!("{topic}.md"));
    if staging.is_file() {
        return Ok(staging);
    }
    Ok(worktree
        .join(".loopdeck")
        .join(HANDOFFS_DIR)
        .join(format!("{topic}.md")))
}

#[allow(dead_code)]
pub fn load_artifact(repo_path: &Path, topic: &str) -> Result<Option<Artifact>, AppError> {
    let path = artifact_path(repo_path, topic)?;
    if !path.exists() {
        return Ok(None);
    }
    let contents = std::fs::read_to_string(&path)?;
    Ok(Some(parse_artifact(
        &contents,
        &topic_for_phase(topic),
        path,
    )?))
}

/// Validate and persist a new artifact. Existing topics are deliberately not
/// replaceable: retries must fail while preserving the original handoff.
pub fn save_artifact(repo_path: &Path, artifact: &Artifact) -> Result<(), AppError> {
    validate_artifact(artifact, None)?;
    let destination = artifact_path(repo_path, &artifact.record.artifact)?;
    if destination.exists() {
        return Err(AppError::Conflict(format!(
            "handoff artifact \"{}\" already exists; original preserved",
            artifact.record.artifact
        )));
    }
    let contents = render_artifact(artifact)?;
    crate::persist::atomic_write(&destination, &contents)?;
    Ok(())
}

/// Validate and copy the authoritative file written by the agent into the
/// persistent store. The source is never removed or modified.
pub fn copy_agent_artifact(
    repo_path: &Path,
    worktree: &Path,
    phase: &str,
) -> Result<Artifact, AppError> {
    let source = agent_artifact_path(worktree, phase)?;
    if !source.is_file() {
        return Err(AppError::RunPlan(format!(
            "phase \"{phase}\" completed without its handoff artifact at {}",
            source.display()
        )));
    }
    let contents = std::fs::read_to_string(&source)?;
    let topic = topic_for_phase(phase);
    let mut artifact = parse_artifact(&contents, &topic, source)?;
    if artifact.record.phase != phase {
        return Err(AppError::RunPlan(format!(
            "handoff artifact {} declares phase \"{}\", expected \"{phase}\"",
            artifact.record.artifact, artifact.record.phase
        )));
    }
    let destination = artifact_path(repo_path, &topic)?;
    artifact.record.path = destination;
    save_artifact(repo_path, &artifact)?;
    Ok(artifact)
}

fn parse_artifact(contents: &str, topic: &str, path: PathBuf) -> Result<Artifact, AppError> {
    let Some(rest) = contents.strip_prefix("---\n") else {
        return Err(AppError::RunPlan(format!(
            "handoff artifact {} is missing YAML frontmatter",
            path.display()
        )));
    };
    let Some(end) = rest.find("\n---\n") else {
        return Err(AppError::RunPlan(format!(
            "handoff artifact {} has an unterminated YAML frontmatter block",
            path.display()
        )));
    };
    let frontmatter = &rest[..end];
    let body = &rest[end + "\n---\n".len()..];
    if frontmatter.len() > MAX_FRONTMATTER_BYTES {
        return Err(AppError::RunPlan(format!(
            "handoff artifact {} frontmatter exceeds 1 KiB",
            path.display()
        )));
    }
    let record: ArtifactRecord = serde_yaml::from_str(frontmatter)?;
    let artifact = Artifact {
        record: ArtifactRecord { path, ..record },
        body: body.to_string(),
    };
    validate_artifact(&artifact, Some(topic))?;
    Ok(artifact)
}

fn validate_artifact(artifact: &Artifact, expected_topic: Option<&str>) -> Result<(), AppError> {
    let record = &artifact.record;
    let topic = topic_for_phase(&record.artifact);
    if topic != record.artifact || record.artifact.is_empty() {
        return Err(AppError::RunPlan(format!(
            "handoff artifact topic \"{}\" must be a kebab-case slug",
            record.artifact
        )));
    }
    if expected_topic.is_some_and(|expected| expected != record.artifact) {
        return Err(AppError::RunPlan(format!(
            "handoff artifact topic \"{}\" does not match expected \"{}\"",
            record.artifact,
            expected_topic.unwrap_or_default()
        )));
    }
    if record.author_role.trim().is_empty() || record.phase.trim().is_empty() {
        return Err(AppError::RunPlan(
            "handoff artifact author_role and phase are required".into(),
        ));
    }
    if record.summary.chars().count() > 200 || record.summary.contains('\n') {
        return Err(AppError::RunPlan(
            "handoff artifact summary must be one line and at most 200 characters".into(),
        ));
    }
    if artifact.body.len() > MAX_BODY_BYTES {
        return Err(AppError::RunPlan(format!(
            "handoff artifact {} body exceeds 8 KiB",
            record.artifact
        )));
    }
    let headings = artifact
        .body
        .lines()
        .filter(|line| line.starts_with("## "))
        .map(|line| line.trim_start_matches("## ").trim())
        .collect::<Vec<_>>();
    if !headings.contains(&"Summary") {
        return Err(AppError::RunPlan(format!(
            "handoff artifact {} is missing the required ## Summary section",
            record.artifact
        )));
    }
    if matches!(
        record.artifact_type,
        ArtifactType::Plan | ArtifactType::Analysis
    ) && !headings.contains(&"Requirements")
    {
        return Err(AppError::RunPlan(format!(
            "handoff artifact {} is missing the required ## Requirements section",
            record.artifact
        )));
    }
    if record.artifact_type == ArtifactType::Plan && !headings.contains(&"Non-Goals") {
        return Err(AppError::RunPlan(format!(
            "handoff artifact {} is missing the required ## Non-Goals section",
            record.artifact
        )));
    }
    let sections = headings.len();
    if sections > 8 {
        return Err(AppError::RunPlan(format!(
            "handoff artifact {} has more than 8 top-level sections",
            record.artifact
        )));
    }
    let numbered_items = artifact
        .body
        .lines()
        .filter(|line| {
            let trimmed = line.trim_start();
            trimmed.as_bytes().first().is_some_and(u8::is_ascii_digit) && trimmed.contains(". ")
        })
        .count();
    if numbered_items > 12 {
        return Err(AppError::RunPlan(format!(
            "handoff artifact {} has more than 12 numbered items",
            record.artifact
        )));
    }
    Ok(())
}

fn render_artifact(artifact: &Artifact) -> Result<String, AppError> {
    let frontmatter = serde_yaml::to_string(&artifact.record)?;
    let frontmatter = frontmatter
        .lines()
        .filter(|line| !line.starts_with("path:"))
        .collect::<Vec<_>>()
        .join("\n");
    if frontmatter.len() > MAX_FRONTMATTER_BYTES {
        return Err(AppError::RunPlan(
            "handoff frontmatter exceeds 1 KiB".into(),
        ));
    }
    Ok(format!("---\n{frontmatter}\n---\n{}", artifact.body))
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

    fn artifact(topic: &str, phase: &str, artifact_type: ArtifactType) -> Artifact {
        Artifact {
            record: ArtifactRecord {
                artifact: topic.into(),
                author_role: "engineering-manager".into(),
                phase: phase.into(),
                artifact_type,
                created: chrono::NaiveDate::from_ymd_opt(2026, 10, 10).unwrap(),
                summary: "A valid handoff artifact".into(),
                cites: vec![],
                path: PathBuf::new(),
            },
            body: "## Summary\nA summary.\n\n## Requirements\n1. R1 is stable.\n\n## Non-Goals\nNothing else.".into(),
        }
    }

    #[test]
    fn artifact_roundtrips_with_record_path_and_body() {
        let dir = std::env::temp_dir().join(format!("loopdeck-artifact-{}", uuid::Uuid::new_v4()));
        let original = artifact(
            "store-model",
            "agent-handoff/store-model",
            ArtifactType::Plan,
        );
        save_artifact(&dir, &original).unwrap();

        let loaded = load_artifact(&dir, "store-model").unwrap().unwrap();
        assert_eq!(loaded.record.artifact, "store-model");
        assert_eq!(loaded.record.author_role, "engineering-manager");
        assert_eq!(loaded.record.artifact_type, ArtifactType::Plan);
        assert_eq!(loaded.body, original.body);
        assert_eq!(
            loaded.record.path,
            artifact_path(&dir, "store-model").unwrap()
        );

        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn duplicate_topic_fails_without_replacing_original() {
        let dir = std::env::temp_dir().join(format!("loopdeck-artifact-{}", uuid::Uuid::new_v4()));
        let original = artifact(
            "retry-topic",
            "agent-handoff/store-model",
            ArtifactType::Report,
        );
        save_artifact(&dir, &original).unwrap();

        let mut retry = artifact(
            "retry-topic",
            "agent-handoff/store-model",
            ArtifactType::Report,
        );
        retry.body = "## Summary\nA different artifact.".into();
        let error = save_artifact(&dir, &retry).unwrap_err();
        assert!(error.to_string().contains("already exists"));
        assert_eq!(
            load_artifact(&dir, "retry-topic").unwrap().unwrap().body,
            original.body
        );

        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn agent_artifact_is_validated_and_copied_without_removing_source() {
        let root = std::env::temp_dir().join(format!("loopdeck-artifact-{}", uuid::Uuid::new_v4()));
        let worktree = root.join("worktree");
        let source = worktree.join(".loopdeck/handoff-artifacts/agent-handoff-store-model.md");
        std::fs::create_dir_all(source.parent().unwrap()).unwrap();
        let written = artifact(
            "agent-handoff-store-model",
            "agent-handoff/store-model",
            ArtifactType::Plan,
        );
        std::fs::write(&source, render_artifact(&written).unwrap()).unwrap();

        let copied = copy_agent_artifact(&root, &worktree, "agent-handoff/store-model").unwrap();
        assert_eq!(
            copied.record.path,
            artifact_path(&root, "agent-handoff-store-model").unwrap()
        );
        assert!(source.exists());
        assert!(artifact_path(&root, "agent-handoff-store-model")
            .unwrap()
            .exists());

        std::fs::remove_dir_all(root).ok();
    }
}
