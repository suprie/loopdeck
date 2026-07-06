use crate::error::AppError;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use tracing::debug;

// ── Public response types ──────────────────────────────────────────────────
#[derive(Clone, Serialize, Deserialize, Default)]
pub struct AgentConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub base_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub auth_token: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub effort: Option<String>,
}

impl std::fmt::Debug for AgentConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AgentConfig")
            .field("base_url", &self.base_url)
            .field("model", &self.model)
            .field(
                "auth_token",
                if self.auth_token.is_some() {
                    &"***REDACTED***"
                } else {
                    &"None"
                },
            )
            .field("effort", &self.effort)
            .finish()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub enum ProjectStatus {
    #[default]
    Active,
    Archived,
    NonActive,
    Warning,
}

/// Aggregate uncommitted change stats for a project's working tree. Mirrors
/// `git::UncommittedStats` but defined here to avoid a circular dependency
/// (`git` doesn't depend on `config`, and serde serializes this 1:1).
#[derive(Debug, Clone, Copy, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct UncommittedStats {
    #[serde(default)]
    pub files: u32,
    #[serde(default)]
    pub added: u32,
    #[serde(default)]
    pub deleted: u32,
}

impl From<crate::git::UncommittedStats> for UncommittedStats {
    fn from(s: crate::git::UncommittedStats) -> Self {
        Self {
            files: s.files,
            added: s.added,
            deleted: s.deleted,
        }
    }
}

/// Live agent run state for a project. Ephemeral — derived from `AppState`
/// (live session + pending approvals/questions) on each `list_projects` call,
/// not persisted to YAML. Serializes to lowercase strings for the frontend.
///
/// - `Idle` — no live session, or session exists but no turn is in flight.
/// - `Working` — a streaming agent turn is in flight right now.
/// - `Waiting` — the in-flight turn is parked awaiting the user (a manual
///   permission approval or an `AskUserQuestion` answer).
/// - `Done` — the most recent turn finished recently (transient UI affordance).
#[derive(Debug, Clone, Copy, Serialize, Deserialize, Default, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum RunState {
    #[default]
    Idle,
    Working,
    Waiting,
    Done,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectEntry {
    pub path: PathBuf,
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub status: ProjectStatus,
    #[serde(default)]
    /// High-level summary of .loopdeck/current-loop.md (≤100 chars by convention).
    pub current_loop: Option<String>,
    pub last_opened: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    /// ISO 8601 timestamp of the last git commit (refreshed on startup).
    #[serde(default)]
    pub last_commit_date: Option<String>,
    #[serde(default)]
    pub last_commit_message: Option<String>,
    /// ISO 8601 timestamp of the most recently modified file (refreshed on startup).
    #[serde(default)]
    pub last_modified: Option<String>,
    /// Uncommitted working-tree diff stats (refreshed on startup/rescan).
    /// Older configs without this field deserialize to all-zero.
    #[serde(default)]
    pub uncommitted: UncommittedStats,
    /// Live agent run state. Ephemeral — derived at read time, not persisted.
    /// Older configs without this field deserialize to `Idle`.
    #[serde(default, skip_serializing_if = "is_run_state_idle")]
    pub run_state: RunState,
}

/// serde `skip_serializing_if` predicate: omit `run_state` when `Idle` so the
/// ephemeral field doesn't clutter the persisted YAML.
fn is_run_state_idle(state: &RunState) -> bool {
    matches!(state, RunState::Idle)
}

impl Default for ProjectEntry {
    fn default() -> Self {
        Self {
            path: PathBuf::new(),
            name: String::new(),
            description: String::new(),
            status: ProjectStatus::Active,
            current_loop: None,
            last_opened: None,
            created_at: Utc::now(),
            last_commit_date: None,
            last_commit_message: None,
            last_modified: None,
            uncommitted: UncommittedStats::default(),
            run_state: RunState::Idle,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Settings {
    #[serde(default = "default_scan_depth")]
    pub scan_depth: u8,
}

fn default_scan_depth() -> u8 {
    5
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            scan_depth: default_scan_depth(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct GlobalConfig {
    #[serde(default)]
    pub agent: Option<AgentConfig>,
    #[serde(default)]
    pub projects: Vec<ProjectEntry>,
    #[serde(default)]
    pub settings: Settings,
}

impl GlobalConfig {
    /// Load global config from `~/.config/loopdeck/config.yaml`.
    /// Returns a fresh default if the file does not exist.
    pub fn load() -> Result<Self, AppError> {
        let config_path = Self::config_path()?;

        if !config_path.exists() {
            return Ok(Self::default());
        }

        let contents = std::fs::read_to_string(&config_path)?;
        let config: GlobalConfig = serde_yaml::from_str(&contents)?;
        Ok(config)
    }

    /// Save global config to `~/.config/loopdeck/config.yaml`.
    /// Creates parent directories if needed.
    pub fn save(&self) -> Result<(), AppError> {
        let config_path = Self::config_path()?;

        if let Some(parent) = config_path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let contents = serde_yaml::to_string(self)?;
        std::fs::write(&config_path, contents)?;

        Ok(())
    }

    /// Find a project entry by path.
    pub fn find_by_path(&self, path: &Path) -> Option<&ProjectEntry> {
        self.projects.iter().find(|p| p.path == path)
    }

    /// Find a project entry by path (mutable).
    pub fn find_by_path_mut(&mut self, path: &Path) -> Option<&mut ProjectEntry> {
        self.projects.iter_mut().find(|p| p.path == path)
    }

    /// Remove a project from the registry by path.
    pub fn remove_project(&mut self, path: &Path) -> bool {
        let len_before = self.projects.len();
        self.projects.retain(|p| p.path != path);
        self.projects.len() < len_before
    }

    /// Add a project to the registry. Returns error if already registered.
    pub fn add_project(&mut self, entry: ProjectEntry) -> Result<(), AppError> {
        if self.find_by_path(&entry.path).is_some() {
            return Err(AppError::ProjectAlreadyExists(
                entry.path.display().to_string(),
            ));
        }
        self.projects.push(entry);
        Ok(())
    }

    /// Path to the config directory: `~/.config/loopdeck/`
    pub fn config_dir() -> Result<PathBuf, AppError> {
        let dir = directories::ProjectDirs::from("com", "loopdeck", "LoopDeck")
            .map(|dirs| dirs.config_dir().to_path_buf())
            .or_else(|| {
                // Fallback: use ~/.config/loopdeck if XDG resolution fails
                dirs_fallback()
            })
            .ok_or_else(|| AppError::Config("Could not determine config directory".into()))?;
        Ok(dir)
    }

    /// Full path to the config file: `~/.config/loopdeck/config.yaml`
    pub fn config_path() -> Result<PathBuf, AppError> {
        Ok(Self::config_dir()?.join("config.yaml"))
    }
}

/// Fallback to `~/.config/loopdeck` using the `dirs` crate (part of `directories`).
fn dirs_fallback() -> Option<PathBuf> {
    let home = dirs_sys_fallback()?;
    Some(home.join(".config").join("loopdeck"))
}

fn dirs_sys_fallback() -> Option<PathBuf> {
    std::env::var("HOME")
        .or_else(|_| {
            std::env::var("USERPROFILE").or_else(|_| {
                let drive = std::env::var("HOMEDRIVE").unwrap_or_default();
                let path = std::env::var("HOMEPATH").unwrap_or_default();
                Ok::<String, std::env::VarError>(format!("{drive}{path}"))
            })
        })
        .ok()
        .map(PathBuf::from)
}

/// Derive project status from git date freshness stored on the entry.
/// Tries `last_commit` first, falls back to `last_modified`.
/// 0–6 days → Active, 7–30 days → Warning, 30+ → NonActive.
pub fn update_project_status(project: &mut ProjectEntry) {
    let now = Utc::now();
    let date_str = project
        .last_commit_date
        .as_deref()
        .or(project.last_modified.as_deref());

    if let Some(date_str) = date_str {
        debug!("Last activity: {}", date_str);
        if let Ok(date) = DateTime::parse_from_rfc3339(date_str) {
            let days = now
                .signed_duration_since(date.with_timezone(&Utc))
                .num_days();
            project.status = match days {
                0..=6 => ProjectStatus::Active,
                7..=30 => ProjectStatus::Warning,
                _ => ProjectStatus::NonActive,
            };
            debug!(
                "status: {:?} ({} days since last activity)",
                project.status, days
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Duration;

    #[test]
    fn test_config_roundtrip() {
        let config = GlobalConfig {
            agent: None,
            projects: vec![ProjectEntry {
                path: PathBuf::from("/tmp/test-project"),
                name: "Test Project".into(),
                description: "A test project".into(),
                status: ProjectStatus::Active,
                last_opened: None,
                created_at: Utc::now(),
                last_commit_date: None,
                last_commit_message: None,
                last_modified: None,
                current_loop: None,
                ..Default::default()
            }],
            settings: Settings::default(),
        };

        let yaml = serde_yaml::to_string(&config).unwrap();
        let parsed: GlobalConfig = serde_yaml::from_str(&yaml).unwrap();
        assert_eq!(parsed.projects.len(), 1);
        assert_eq!(parsed.projects[0].name, "Test Project");
    }

    #[test]
    fn test_empty_config_is_default() {
        let config: GlobalConfig = serde_yaml::from_str("").unwrap();
        assert!(config.projects.is_empty());
    }

    #[test]
    fn test_add_and_remove_project() {
        let mut config = GlobalConfig::default();
        let entry = ProjectEntry {
            path: PathBuf::from("/tmp/foo"),
            name: "Foo".into(),
            description: String::new(),
            status: ProjectStatus::Active,
            last_opened: None,
            created_at: Utc::now(),
            last_commit_date: None,
            last_commit_message: None,
            last_modified: None,
            current_loop: None,
            ..Default::default()
        };

        config.add_project(entry).unwrap();
        assert_eq!(config.projects.len(), 1);

        // Duplicate should error
        let dup = ProjectEntry {
            path: PathBuf::from("/tmp/foo"),
            name: "Foo2".into(),
            description: String::new(),
            status: ProjectStatus::Active,
            last_opened: None,
            created_at: Utc::now(),
            last_commit_date: None,
            last_commit_message: None,
            last_modified: None,
            current_loop: None,
            ..Default::default()
        };
        assert!(config.add_project(dup).is_err());

        // Remove
        assert!(config.remove_project(&PathBuf::from("/tmp/foo")));
        assert!(config.projects.is_empty());
        assert!(!config.remove_project(&PathBuf::from("/tmp/foo")));
    }

    #[test]
    fn test_serialize_deserialize_tempfile() {
        let dir = std::env::temp_dir().join("loopdeck-test-config");
        let _ = std::fs::remove_dir_all(&dir);

        // We override the config path for this test by saving/loading manually
        let config_path = dir.join("config.yaml");
        std::fs::create_dir_all(&dir).unwrap();

        let config = GlobalConfig {
            agent: None,
            projects: vec![],
            settings: Settings::default(),
        };

        let yaml = serde_yaml::to_string(&config).unwrap();
        std::fs::write(&config_path, yaml).unwrap();

        let read = std::fs::read_to_string(&config_path).unwrap();
        let parsed: GlobalConfig = serde_yaml::from_str(&read).unwrap();
        assert!(parsed.projects.is_empty());

        let _ = std::fs::remove_dir_all(&dir);
    }

    // ── update_project_status tests ──

    fn make_entry(last_commit_date: Option<String>, last_modified: Option<String>) -> ProjectEntry {
        ProjectEntry {
            path: PathBuf::from("/tmp/test-status"),
            name: "Test".into(),
            description: String::new(),
            status: ProjectStatus::Active,
            last_opened: None,
            created_at: Utc::now(),
            last_commit_date,
            last_commit_message: None,
            last_modified,
            current_loop: None,
            ..Default::default()
        }
    }

    fn rfc3339_days_ago(days: i64) -> String {
        (Utc::now() - Duration::days(days))
            .format("%Y-%m-%dT%H:%M:%S%:z")
            .to_string()
    }

    #[test]
    fn test_status_active_today() {
        let mut entry = make_entry(Some(rfc3339_days_ago(0)), None);
        update_project_status(&mut entry);
        assert_eq!(entry.status, ProjectStatus::Active);
    }

    #[test]
    fn test_status_active_6_days() {
        let mut entry = make_entry(Some(rfc3339_days_ago(6)), None);
        update_project_status(&mut entry);
        assert_eq!(entry.status, ProjectStatus::Active);
    }

    #[test]
    fn test_status_warning_7_days() {
        let mut entry = make_entry(Some(rfc3339_days_ago(7)), None);
        update_project_status(&mut entry);
        assert_eq!(entry.status, ProjectStatus::Warning);
    }

    #[test]
    fn test_status_warning_30_days() {
        let mut entry = make_entry(Some(rfc3339_days_ago(30)), None);
        update_project_status(&mut entry);
        assert_eq!(entry.status, ProjectStatus::Warning);
    }

    #[test]
    fn test_status_nonactive_31_days() {
        let mut entry = make_entry(Some(rfc3339_days_ago(31)), None);
        update_project_status(&mut entry);
        assert_eq!(entry.status, ProjectStatus::NonActive);
    }

    #[test]
    fn test_status_falls_back_to_last_modified() {
        let mut entry = make_entry(None, Some(rfc3339_days_ago(10)));
        update_project_status(&mut entry);
        assert_eq!(entry.status, ProjectStatus::Warning);
    }

    #[test]
    fn test_status_prefers_last_commit_over_modified() {
        // last_commit = 3 days (Active), last_modified = 20 days (Warning)
        let mut entry = make_entry(Some(rfc3339_days_ago(3)), Some(rfc3339_days_ago(20)));
        update_project_status(&mut entry);
        // Should use last_commit (3 days → Active), not last_modified
        assert_eq!(entry.status, ProjectStatus::Active);
    }

    #[test]
    fn test_status_no_dates_unchanged() {
        let mut entry = make_entry(None, None);
        entry.status = ProjectStatus::Archived;
        update_project_status(&mut entry);
        assert_eq!(entry.status, ProjectStatus::Archived);
    }

    #[test]
    fn test_status_invalid_date_unchanged() {
        let mut entry = make_entry(Some("not-a-date".into()), None);
        entry.status = ProjectStatus::Active;
        update_project_status(&mut entry);
        assert_eq!(entry.status, ProjectStatus::Active);
    }

    #[test]
    fn test_status_nonactive_365_days() {
        let mut entry = make_entry(Some(rfc3339_days_ago(365)), None);
        update_project_status(&mut entry);
        assert_eq!(entry.status, ProjectStatus::NonActive);
    }

    // ── AgentConfig tests ─────────────────────────────────────────────────

    #[test]
    fn test_config_with_agent_block() {
        let yaml = r#"
settings:
  scan_depth: 3
agent:
  auth_token: sk-test-123
  base_url: https://api.anthropic.com
  model: claude-sonnet-4-6
  effort: high
projects: []
"#;
        let config: GlobalConfig = serde_yaml::from_str(yaml).unwrap();
        let agent = config.agent.expect("agent block should be present");
        assert_eq!(agent.auth_token.as_deref(), Some("sk-test-123"));
        assert_eq!(agent.base_url.as_deref(), Some("https://api.anthropic.com"));
        assert_eq!(agent.model.as_deref(), Some("claude-sonnet-4-6"));
        assert_eq!(agent.effort.as_deref(), Some("high"));
    }

    #[test]
    fn test_config_without_agent_block_is_none() {
        let yaml = r#"
settings:
  scan_depth: 2
projects:
  - path: /tmp/foo
    name: Foo
    description: ""
    status: Active
    last_opened: null
    created_at: "2025-01-01T00:00:00Z"
"#;
        let config: GlobalConfig = serde_yaml::from_str(yaml).unwrap();
        assert!(config.agent.is_none(), "agent should be None when missing");
        assert_eq!(config.settings.scan_depth, 2);
        assert_eq!(config.projects.len(), 1);
    }

    #[test]
    fn test_config_empty_is_default() {
        let config: GlobalConfig = serde_yaml::from_str("").unwrap();
        assert!(config.agent.is_none());
        assert!(config.projects.is_empty());
        assert_eq!(config.settings.scan_depth, 5); // default
    }

    #[test]
    fn test_agent_config_roundtrip() {
        let agent = AgentConfig {
            auth_token: Some("sk-round-trip-test".into()),
            base_url: Some("https://api.example.com/v1".into()),
            model: Some("claude-opus-4-8".into()),
            effort: Some("max".into()),
        };

        let config = GlobalConfig {
            agent: Some(agent.clone()),
            projects: vec![],
            settings: Settings::default(),
        };

        let yaml = serde_yaml::to_string(&config).unwrap();
        let parsed: GlobalConfig = serde_yaml::from_str(&yaml).unwrap();

        let round_tripped = parsed.agent.expect("agent should survive round-trip");
        assert_eq!(round_tripped.auth_token, agent.auth_token);
        assert_eq!(round_tripped.base_url, agent.base_url);
        assert_eq!(round_tripped.model, agent.model);
        assert_eq!(round_tripped.effort, agent.effort);
    }

    #[test]
    fn test_agent_config_serialize_skips_none() {
        // Only set model — other fields should be absent from YAML output
        let agent = AgentConfig {
            auth_token: None,
            base_url: None,
            model: Some("claude-haiku-4-5".into()),
            effort: None,
        };

        let config = GlobalConfig {
            agent: Some(agent),
            projects: vec![],
            settings: Settings::default(),
        };

        let yaml = serde_yaml::to_string(&config).unwrap();

        // These keys should NOT appear in the output
        assert!(!yaml.contains("auth_token"));
        assert!(!yaml.contains("base_url"));
        assert!(!yaml.contains("effort"));
        // Model SHOULD appear
        assert!(yaml.contains("model"));
        assert!(yaml.contains("claude-haiku-4-5"));
    }

    #[test]
    fn test_agent_config_debug_redacts_auth_token() {
        let agent = AgentConfig {
            auth_token: Some("sk-super-secret".into()),
            base_url: Some("https://api.anthropic.com".into()),
            model: None,
            effort: None,
        };

        let debug_str = format!("{:?}", agent);

        // Must NOT leak the real token
        assert!(!debug_str.contains("sk-super-secret"));
        // Must show that a token IS set (redacted)
        assert!(debug_str.contains("***REDACTED***"));

        // Without a token
        let agent_no_token = AgentConfig {
            auth_token: None,
            base_url: None,
            model: None,
            effort: None,
        };
        let debug_no_token = format!("{:?}", agent_no_token);
        assert!(debug_no_token.contains("None"));
    }

    // ── RunState / UncommittedStats tests ──

    #[test]
    fn test_run_state_idle_not_serialized() {
        // Idle run_state should be skipped in YAML so ephemeral state never
        // pollutes the persisted config.
        let entry = ProjectEntry {
            path: PathBuf::from("/tmp/x"),
            name: "X".into(),
            ..Default::default()
        };
        let yaml = serde_yaml::to_string(&entry).unwrap();
        assert!(!yaml.contains("run_state"));
    }

    #[test]
    fn test_run_state_working_serializes_lowercase() {
        let mut entry = ProjectEntry {
            path: PathBuf::from("/tmp/x"),
            name: "X".into(),
            ..Default::default()
        };
        entry.run_state = RunState::Working;
        let yaml = serde_yaml::to_string(&entry).unwrap();
        assert!(yaml.contains("run_state: working"));
    }

    #[test]
    fn test_uncommitted_stats_default_round_trip() {
        // Old configs without `uncommitted` / `run_state` should still parse.
        let yaml = r#"
path: /tmp/legacy
name: Legacy
description: ""
status: Active
last_opened: null
created_at: "2025-01-01T00:00:00Z"
"#;
        let entry: ProjectEntry = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(entry.uncommitted, UncommittedStats::default());
        assert_eq!(entry.run_state, RunState::Idle);
    }
}
