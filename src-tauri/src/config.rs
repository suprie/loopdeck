use crate::error::AppError;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use tracing::debug;

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub enum ProjectStatus {
    #[default]
    Active,
    Archived,
    NonActive,
    Warning,
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
}
