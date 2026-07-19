use crate::error::AppError;
use crate::persist;
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
    /// Presence flag for a token stored in the OS keychain (`secrets` module).
    ///
    /// Populated *only* on the `get_agent_config` read path so the UI can show
    /// a "token stored" affordance without the plaintext ever crossing to the
    /// renderer. It is **never** persisted to `config.yaml`: every path that
    /// saves the config leaves it `false` (the default), so
    /// `skip_serializing_if = "is_false"` keeps it out of the file. It is also
    /// ignored on the `set_agent_config` write path, where presence is always
    /// recomputed from the keychain.
    #[serde(default, skip_serializing_if = "is_false")]
    pub has_auth_token: bool,
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
            .field("has_auth_token", &self.has_auth_token)
            .finish()
    }
}

/// High-level project status. Serialized to lowercase strings for the
/// frontend (matching `RunState`). Aliases accept the legacy PascalCase
/// form so configs written before this normalization still deserialize.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, Default, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ProjectStatus {
    #[default]
    #[serde(alias = "Active")]
    Active,
    #[serde(alias = "Archived")]
    Archived,
    #[serde(alias = "NonActive")]
    NonActive,
    #[serde(alias = "Warning")]
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

/// serde `skip_serializing_if` predicate for `AgentConfig::has_auth_token`:
/// omit the keychain-presence flag when false so it never clutters the
/// persisted YAML — it is only ever `true` on the transient `get_agent_config`
/// response clone, never on the config held in `Mutex<GlobalConfig>`.
fn is_false(b: &bool) -> bool {
    !b
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
    ///
    /// Recovery order (PRD FR2):
    /// 1. Primary missing → fresh default (first launch).
    /// 2. Primary parses → load it.
    /// 3. Primary malformed → try the `.bak`. If it parses, load it and warn.
    ///    The malformed primary is NOT overwritten.
    /// 4. Both malformed/missing → `Err`. The caller MUST NOT silently
    ///    overwrite the malformed primary with a fresh default — the file is
    ///    preserved for manual recovery.
    pub fn load() -> Result<Self, AppError> {
        let config_path = Self::config_path()?;
        Self::load_from_path(&config_path)
    }

    /// Test-friendly inner: same recovery logic as [`load`] but takes an
    /// explicit primary path. The backup path is derived via [`backup_path`].
    pub(crate) fn load_from_path(config_path: &Path) -> Result<Self, AppError> {
        let backup = backup_path(config_path);

        // Primary missing entirely → first launch.
        if !config_path.exists() {
            return Ok(Self::default());
        }

        // Primary exists — try to parse it.
        let contents = std::fs::read_to_string(config_path)?;
        match serde_yaml::from_str::<GlobalConfig>(&contents) {
            Ok(config) => Ok(config),
            Err(primary_err) => {
                // Primary is malformed. Do NOT overwrite it. Try the backup.
                tracing::warn!(
                    "malformed registry at {}: {primary_err}",
                    config_path.display()
                );
                if backup.exists() {
                    let backup_contents = std::fs::read_to_string(&backup)?;
                    match serde_yaml::from_str::<GlobalConfig>(&backup_contents) {
                        Ok(config) => {
                            tracing::warn!(
                                "recovered registry from backup at {}",
                                backup.display()
                            );
                            return Ok(config);
                        }
                        Err(backup_err) => {
                            tracing::warn!(
                                "backup at {} also malformed: {backup_err}",
                                backup.display()
                            );
                        }
                    }
                }
                // Both missing/malformed — surface the error, preserve the
                // primary for manual recovery.
                Err(AppError::Config(format!(
                    "registry at {} is malformed and no valid backup was found; \
                     the file has been preserved for manual recovery. Parse error: {primary_err}",
                    config_path.display()
                )))
            }
        }
    }

    /// Save global config to `~/.config/loopdeck/config.yaml`.
    ///
    /// Crash-safe via [`persist::atomic_write`] (temp + fsync + same-dir
    /// rename). Before overwriting, copies the existing primary to
    /// `config.yaml.bak` so a malformed future primary can be recovered from
    /// the backup.
    ///
    /// Also applies an owner-only permission floor (0600 on Unix) as
    /// defense-in-depth: the auth token itself lives in the OS keychain now
    /// (see `secrets`), but the file still holds provider config, so we don't
    /// rely on the process umask to keep it private.
    pub fn save(&self) -> Result<(), AppError> {
        let config_path = Self::config_path()?;
        self.save_to_path(&config_path)
    }

    /// Test-friendly inner: same atomic-write + backup logic as [`save`] but
    /// takes an explicit primary path.
    pub(crate) fn save_to_path(&self, config_path: &Path) -> Result<(), AppError> {
        // Preserve the current primary as last-known-good before overwriting.
        // Best-effort: a missing primary (first launch) or a backup failure
        // is logged but doesn't abort the save — the primary is the source of
        // truth, the backup is a recovery floor.
        if config_path.exists() {
            let backup = backup_path(config_path);
            if let Err(e) = std::fs::copy(config_path, &backup) {
                tracing::warn!(
                    "failed to update registry backup at {}: {e}",
                    backup.display()
                );
            }
        }

        let contents = serde_yaml::to_string(self)?;
        persist::atomic_write(config_path, &contents)?;
        restrict_file_perms(config_path);

        Ok(())
    }

    /// Migrate any plaintext `agent.auth_token` still present in the loaded
    /// config into the OS keychain, scrubbing it from the in-memory (and, on
    /// the next `save()`, on-disk) config.
    ///
    /// Returns:
    /// - `Ok(true)` — a token was moved; the caller should `save()` so the
    ///   plaintext copy is gone from disk.
    /// - `Ok(false)` — nothing to migrate (no agent block, or no/empty token).
    /// - `Err` — a token was present but the keychain rejected it. The token is
    ///   put back in place so it is not silently lost; the caller should keep
    ///   it in the 0600 file as the interim floor rather than drop it.
    pub fn migrate_auth_token_to_keychain(&mut self) -> Result<bool, AppError> {
        let Some(agent) = self.agent.as_mut() else {
            return Ok(false);
        };
        // Only a non-empty token is a real credential worth moving. `None` and
        // an empty string are left untouched — checked *before* mutating so an
        // empty value isn't silently cleared.
        let Some(token) = agent.auth_token.as_deref() else {
            return Ok(false);
        };
        if token.is_empty() {
            return Ok(false);
        }
        let token = token.to_string();
        // Scrub from config first, then store. If the keychain rejects it we
        // restore the token so it is never silently lost.
        agent.auth_token = None;
        match crate::secrets::store_auth_token(&token) {
            Ok(()) => {
                debug!("migrated plaintext auth token from config.yaml to OS keychain");
                Ok(true)
            }
            Err(e) => {
                agent.auth_token = Some(token);
                Err(e)
            }
        }
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

/// Lock the config file down to owner-only. Best-effort: a failure here is
/// logged but not fatal (the file's contents are no longer secret once the
/// auth token has moved to the keychain; this is defense-in-depth).
#[cfg(unix)]
fn restrict_file_perms(path: &Path) {
    use std::os::unix::fs::PermissionsExt;
    if let Err(e) = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600)) {
        tracing::warn!("failed to set 0600 on {}: {e}", path.display());
    }
}

/// No-op on non-Unix: the config file lives under `%APPDATA%` / `~/Library`,
/// which the OS already scopes to the current user via ACLs. There is no
/// portable `chmod` equivalent, and the keychain backends handle their own
/// access control.
#[cfg(not(unix))]
fn restrict_file_perms(_path: &Path) {}

/// Sibling `.bak` path for a registry primary: `config.yaml` → `config.yaml.bak`.
/// Lives in the same directory so a cross-device rename can never be an issue.
fn backup_path(primary: &Path) -> PathBuf {
    let mut name = primary
        .file_name()
        .expect("registry path has a file name")
        .to_os_string();
    name.push(".bak");
    primary.with_file_name(name)
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
            ..Default::default()
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
            ..Default::default()
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
            ..Default::default()
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
            ..Default::default()
        };
        let debug_no_token = format!("{:?}", agent_no_token);
        assert!(debug_no_token.contains("None"));
    }

    #[test]
    fn test_has_auth_token_not_persisted_to_yaml() {
        // The presence flag must never reach config.yaml. With `skip_serializing_if
        // = "is_false"` it is omitted when false — and every save path leaves it
        // false. Verify a saved config carries no `has_auth_token` key.
        let agent = AgentConfig {
            auth_token: None,
            base_url: Some("https://api.anthropic.com".into()),
            model: Some("claude-sonnet-4-5".into()),
            effort: None,
            has_auth_token: false,
        };
        let config = GlobalConfig {
            agent: Some(agent),
            projects: vec![],
            settings: Settings::default(),
        };
        let yaml = serde_yaml::to_string(&config).unwrap();
        assert!(!yaml.contains("has_auth_token"));
    }

    #[test]
    fn test_has_auth_token_round_trips_on_wire_but_not_from_yaml() {
        // On the IPC wire (serde_json) the flag CAN be true so the frontend
        // learns a token is stored. It deserializes back faithfully here.
        let agent = AgentConfig {
            auth_token: None,
            base_url: None,
            model: Some("claude-sonnet-4-5".into()),
            effort: None,
            has_auth_token: true,
        };
        let json = serde_json::to_string(&agent).unwrap();
        assert!(json.contains("has_auth_token"));
        let back: AgentConfig = serde_json::from_str(&json).unwrap();
        assert!(back.has_auth_token);

        // But a YAML config file that never wrote the flag deserializes to
        // the default (false) — old configs keep working.
        let yaml = r#"
agent:
  model: claude-sonnet-4-5
"#;
        let cfg: GlobalConfig = serde_yaml::from_str(yaml).unwrap();
        assert!(!cfg.agent.unwrap().has_auth_token);
    }

    // ── migrate_auth_token_to_keychain (offline no-op paths) ──
    //
    // The "token present" branch writes to the real OS keychain, so it is
    // covered by the `#[ignore]`d live test in `secrets.rs`. These exercise
    // the early-return paths that never touch the keychain.

    #[test]
    fn migrate_noop_when_no_agent_block() {
        let mut config = GlobalConfig::default();
        assert!(!config.migrate_auth_token_to_keychain().unwrap());
        assert!(config.agent.is_none());
    }

    #[test]
    fn migrate_noop_when_token_none() {
        let mut config = GlobalConfig {
            agent: Some(AgentConfig {
                auth_token: None,
                base_url: Some("https://api.anthropic.com".into()),
                model: Some("claude-sonnet-4-5".into()),
                effort: None,
                has_auth_token: false,
            }),
            ..Default::default()
        };
        assert!(!config.migrate_auth_token_to_keychain().unwrap());
        assert!(config.agent.as_ref().unwrap().auth_token.is_none());
    }

    #[test]
    fn migrate_noop_when_token_empty() {
        let mut config = GlobalConfig {
            agent: Some(AgentConfig {
                auth_token: Some(String::new()),
                base_url: None,
                model: None,
                effort: None,
                has_auth_token: false,
            }),
            ..Default::default()
        };
        // Empty string is treated as "no token" — must not call the keychain.
        assert!(!config.migrate_auth_token_to_keychain().unwrap());
        assert_eq!(
            config.agent.as_ref().unwrap().auth_token.as_deref(),
            Some("")
        );
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

    // ── Phase 2: atomic-write + backup recovery ──────────────────────────

    /// Unique test dir keyed by name + PID + nanos so parallel tests can't
    /// race on shared parents.
    fn phase2_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "loopdeck-config-{name}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn load_returns_default_when_primary_missing() {
        // First-launch case: no primary, no backup → fresh default, no error.
        let dir = phase2_dir("missing_primary");
        let primary = dir.join("config.yaml");
        let config = GlobalConfig::load_from_path(&primary).unwrap();
        assert!(config.projects.is_empty());
        assert!(config.agent.is_none());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn load_recovers_from_backup_when_primary_malformed() {
        // PRD FR2: a malformed primary MUST NOT be silently overwritten. The
        // backup is loaded instead, and the malformed primary is preserved
        // on disk for manual inspection.
        let dir = phase2_dir("recover_from_bak");
        let primary = dir.join("config.yaml");
        let backup = dir.join("config.yaml.bak");

        // Seed a valid backup carrying an identifiable model value, then a
        // genuinely malformed primary (an unclosed flow sequence —
        // `:::not yaml:::` parses as an empty document under serde_yaml's
        // lenient scalar rules, so use something it actually rejects).
        std::fs::write(&backup, "agent:\n  model: backup-model\n").unwrap();
        std::fs::write(&primary, "agent: [unclosed").unwrap();

        let config = GlobalConfig::load_from_path(&primary).unwrap();
        assert_eq!(
            config.agent.and_then(|a| a.model),
            Some("backup-model".into()),
            "should recover the backup's contents"
        );

        // The malformed primary must still be on disk, unchanged — not
        // overwritten by the recovery.
        assert_eq!(
            std::fs::read_to_string(&primary).unwrap(),
            "agent: [unclosed",
            "malformed primary must be preserved for manual recovery"
        );

        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn load_errors_when_both_primary_and_backup_malformed() {
        // If neither primary nor backup parses, surface an error rather than
        // silently defaulting. lib.rs startup turns this into a visible exit.
        let dir = phase2_dir("both_bad");
        let primary = dir.join("config.yaml");
        let backup = dir.join("config.yaml.bak");

        std::fs::write(&primary, "agent: [unclosed").unwrap();
        std::fs::write(&backup, "{ invalid: ").unwrap();

        let result = GlobalConfig::load_from_path(&primary);
        assert!(
            result.is_err(),
            "should error when neither primary nor backup parses"
        );

        // Both files must still be on disk, unchanged.
        assert_eq!(
            std::fs::read_to_string(&primary).unwrap(),
            "agent: [unclosed"
        );
        assert_eq!(std::fs::read_to_string(&backup).unwrap(), "{ invalid: ");

        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn save_creates_backup_before_overwriting() {
        // Every save must preserve the previous primary as .bak, so a future
        // malformed primary can be recovered.
        let dir = phase2_dir("save_makes_bak");
        let primary = dir.join("config.yaml");
        let backup = dir.join("config.yaml.bak");

        // Initial state: a primary with an identifiable model, no backup.
        std::fs::write(&primary, "agent:\n  model: old-model\n").unwrap();

        let mut config = GlobalConfig::load_from_path(&primary).unwrap();
        // Mutate + save.
        config.agent = Some(AgentConfig {
            base_url: None,
            model: Some("new-model".into()),
            auth_token: None,
            effort: None,
            has_auth_token: false,
        });
        config.save_to_path(&primary).unwrap();

        // The backup should now hold the OLD primary contents.
        assert!(backup.exists(), "save must create a .bak sibling");
        let backup_contents = std::fs::read_to_string(&backup).unwrap();
        assert!(
            backup_contents.contains("old-model"),
            "backup should hold the pre-save primary, got: {backup_contents}"
        );

        // The primary should hold the NEW contents.
        let primary_contents = std::fs::read_to_string(&primary).unwrap();
        assert!(
            primary_contents.contains("new-model"),
            "primary should hold the new save, got: {primary_contents}"
        );

        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn save_is_atomic_no_truncated_primary_on_temp_left_behind() {
        // If a prior crashed write left a stale temp in the target's dir, a
        // subsequent save must still produce a complete primary. (Reassoc
        // check: the temp-suffix includes the PID, so a stale temp from a
        // different PID never conflicts.)
        let dir = phase2_dir("stale_temp");
        let primary = dir.join("config.yaml");
        let stale_temp = dir.join("config.yaml.999999.tmp");

        std::fs::write(&primary, "agent:\n  model: original\n").unwrap();
        std::fs::write(&stale_temp, "stale temp from a crashed prior run").unwrap();

        let config = GlobalConfig::default();
        config.save_to_path(&primary).unwrap();

        // Primary must be valid YAML (the save succeeded atomically) — and
        // the stale temp from the other PID must be untouched (not renamed
        // over by mistake).
        GlobalConfig::load_from_path(&primary)
            .unwrap_or_else(|e| panic!("primary should parse after save despite stale temp: {e}"));
        assert_eq!(
            std::fs::read_to_string(&stale_temp).unwrap(),
            "stale temp from a crashed prior run",
            "stale temp from another PID must not be touched"
        );

        std::fs::remove_dir_all(&dir).unwrap();
    }
}
