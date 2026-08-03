use crate::error::AppError;
use crate::persist;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use tracing::debug;
use uuid::Uuid;

// ── Public response types ──────────────────────────────────────────────────
#[derive(Debug, Clone, Copy, Serialize, Deserialize, Default, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum AgentHarness {
    #[default]
    Claude,
    Codex,
}

fn is_default_harness(harness: &AgentHarness) -> bool {
    *harness == AgentHarness::Claude
}

#[derive(Clone, Serialize, Deserialize, Default)]
pub struct AgentConfig {
    #[serde(default, skip_serializing_if = "is_default_harness")]
    pub harness: AgentHarness,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub base_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub auth_token: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub effort: Option<String>,
    /// Presence flag for a token stored in the local secrets file (`secrets`
    /// module).
    ///
    /// Populated *only* on the `get_agent_config` read path so the UI can show
    /// a "token stored" affordance without the plaintext ever crossing to the
    /// renderer. It is **never** persisted to `config.yaml`: every path that
    /// saves the config leaves it `false` (the default), so
    /// `skip_serializing_if = "is_false"` keeps it out of the file. It is also
    /// ignored on the `set_agent_config` write path, where presence is always
    /// recomputed from the secrets file.
    #[serde(default, skip_serializing_if = "is_false")]
    pub has_auth_token: bool,
}

impl std::fmt::Debug for AgentConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AgentConfig")
            .field("harness", &self.harness)
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

/// A reusable agent definition in the global roster.
///
/// The identifier is a UUID generated once at creation time and is the stable
/// key used by loop assignments and the per-agent secrets store.  `name` is a
/// user-facing label and may change, but is unique case-insensitively within a
/// registry. `config` is flattened so the IPC/YAML shape remains pleasantly
/// small (`id`, `name`, `harness`, `model`, …) and never contains a token.
#[derive(Clone, Serialize, Deserialize)]
pub struct NamedAgentConfig {
    pub id: String,
    pub name: String,
    #[serde(flatten)]
    pub config: AgentConfig,
}

// The singleton-to-roster migration must survive a crash between writing the
// UUID-scoped secret and saving config.yaml. A random id would orphan the
// secret on restart, so every legacy singleton deterministically maps to this
// valid, reserved UUID. User-created entries continue to use UUID v4.
const LEGACY_DEFAULT_AGENT_ID: &str = "00000000-0000-4000-8000-000000000001";

impl std::fmt::Debug for NamedAgentConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("NamedAgentConfig")
            .field("id", &self.id)
            .field("name", &self.name)
            .field("config", &self.config)
            .finish()
    }
}

impl NamedAgentConfig {
    pub fn new(name: String, config: AgentConfig) -> Result<Self, AppError> {
        Ok(Self {
            id: Uuid::new_v4().to_string(),
            name: normalize_agent_name(&name)?,
            config: scrub_agent_config(config),
        })
    }

    fn validate(&self) -> Result<(), AppError> {
        Uuid::parse_str(&self.id).map_err(|_| {
            AppError::Config(format!("agent config id '{}' is not a valid UUID", self.id))
        })?;
        normalize_agent_name(&self.name)?;
        Ok(())
    }
}

fn normalize_agent_name(name: &str) -> Result<String, AppError> {
    let name = name.trim();
    if name.is_empty() {
        return Err(AppError::Config("agent config name cannot be empty".into()));
    }
    if name.chars().count() > 120 {
        return Err(AppError::Config(
            "agent config name must be 120 characters or fewer".into(),
        ));
    }
    Ok(name.to_string())
}

fn scrub_agent_config(mut config: AgentConfig) -> AgentConfig {
    config.auth_token = None;
    config.has_auth_token = false;
    config
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
    /// Per-project autonomous mode: when true, the agent self-approves
    /// floor-clearing tool calls (Edit/Write, safe Bash, MCP, WebFetch) so
    /// loops run unattended. The destructive floor still applies. Older
    /// configs without this field deserialize to `false` (confirm-changes).
    /// `skip_serializing_if` keeps the registry tidy for the common case.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub autonomous: bool,
    /// Total `## Next Steps` checklist items parsed from `.loopdeck/loops.md`.
    /// Re-read at every `list_projects`/`import_project` call (same treatment
    /// as `current_loop`: doesn't gate whether a save happens, but is always
    /// serialized — including `0` — since the frontend's `next_steps_done`
    /// interpolation needs both fields present on every IPC response, not
    /// just when non-zero.
    #[serde(default)]
    pub next_steps_total: usize,
    /// Checked (`- [x]`) items within `next_steps_total`. Same treatment as
    /// `next_steps_total` — always serialized, never skipped when zero.
    #[serde(default)]
    pub next_steps_done: usize,
}

/// serde `skip_serializing_if` predicate: omit `run_state` when `Idle` so the
/// ephemeral field doesn't clutter the persisted YAML.
fn is_run_state_idle(state: &RunState) -> bool {
    matches!(state, RunState::Idle)
}

/// serde `skip_serializing_if` predicate for `AgentConfig::has_auth_token`:
/// omit the secrets-file-presence flag when false so it never clutters the
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
            autonomous: false,
            next_steps_total: 0,
            next_steps_done: 0,
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
    /// Legacy singleton retained only long enough to migrate existing
    /// registries. New saves omit it once migration has run.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent: Option<AgentConfig>,
    /// Reusable named agents keyed by their immutable UUID `id`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub agents: Vec<NamedAgentConfig>,
    /// UUID of the roster entry used by legacy single-agent paths.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_agent_id: Option<String>,
    #[serde(default)]
    pub projects: Vec<ProjectEntry>,
    #[serde(default)]
    pub settings: Settings,
}

impl GlobalConfig {
    /// Return the selected default roster entry, falling back to the first
    /// entry when reading an older roster that predates `default_agent_id`.
    pub fn default_named_agent_config(&self) -> Option<&NamedAgentConfig> {
        self.default_agent_id
            .as_deref()
            .and_then(|id| self.find_agent_config(id))
            .or_else(|| self.agents.first())
    }

    pub fn find_agent_config(&self, id: &str) -> Option<&NamedAgentConfig> {
        self.agents.iter().find(|agent| agent.id == id)
    }

    pub fn find_agent_config_mut(&mut self, id: &str) -> Option<&mut NamedAgentConfig> {
        self.agents.iter_mut().find(|agent| agent.id == id)
    }

    pub fn create_agent_config(
        &mut self,
        name: String,
        config: AgentConfig,
    ) -> Result<NamedAgentConfig, AppError> {
        self.ensure_unique_agent_name(&name, None)?;
        let agent = NamedAgentConfig::new(name, config)?;
        if self.default_agent_id.is_none() {
            self.default_agent_id = Some(agent.id.clone());
        }
        self.agents.push(agent.clone());
        Ok(agent)
    }

    pub fn update_agent_config(
        &mut self,
        id: &str,
        name: String,
        config: AgentConfig,
    ) -> Result<NamedAgentConfig, AppError> {
        Uuid::parse_str(id)
            .map_err(|_| AppError::Config(format!("agent config id '{id}' is not a valid UUID")))?;
        self.ensure_unique_agent_name(&name, Some(id))?;
        let agent = self
            .find_agent_config_mut(id)
            .ok_or_else(|| AppError::Config(format!("agent config '{id}' was not found")))?;
        agent.name = normalize_agent_name(&name)?;
        agent.config = scrub_agent_config(config);
        Ok(agent.clone())
    }

    /// Delete an entry and return the removed value. If it was the default,
    /// the oldest remaining entry becomes default (or no default remains).
    pub fn delete_agent_config(&mut self, id: &str) -> Result<NamedAgentConfig, AppError> {
        Uuid::parse_str(id)
            .map_err(|_| AppError::Config(format!("agent config id '{id}' is not a valid UUID")))?;
        let index = self
            .agents
            .iter()
            .position(|agent| agent.id == id)
            .ok_or_else(|| AppError::Config(format!("agent config '{id}' was not found")))?;
        let deleted = self.agents.remove(index);
        if self.default_agent_id.as_deref() == Some(id) {
            self.default_agent_id = self.agents.first().map(|agent| agent.id.clone());
        }
        Ok(deleted)
    }

    pub fn set_default_agent_config(&mut self, id: &str) -> Result<NamedAgentConfig, AppError> {
        let agent = self
            .find_agent_config(id)
            .ok_or_else(|| AppError::Config(format!("agent config '{id}' was not found")))?
            .clone();
        agent.validate()?;
        self.default_agent_id = Some(id.to_string());
        Ok(agent)
    }

    fn ensure_unique_agent_name(
        &self,
        name: &str,
        except_id: Option<&str>,
    ) -> Result<(), AppError> {
        let normalized = normalize_agent_name(name)?;
        if self.agents.iter().any(|agent| {
            Some(agent.id.as_str()) != except_id
                && agent.name.to_lowercase() == normalized.to_lowercase()
        }) {
            return Err(AppError::Config(format!(
                "an agent config named '{normalized}' already exists"
            )));
        }
        Ok(())
    }

    /// Convert the old singleton configuration into one named default entry.
    /// This only changes memory; callers persist the returned `true` result.
    /// It is idempotent: once `agent` is absent, subsequent calls are no-ops.
    pub fn migrate_legacy_agent_to_roster(&mut self) -> Result<bool, AppError> {
        let Some(legacy) = self.agent.take() else {
            return Ok(false);
        };
        if !self.agents.is_empty() {
            // A manually-created roster wins. Do not discard the legacy
            // singleton; retain it so a later manual recovery remains possible.
            self.agent = Some(legacy);
            return Ok(false);
        }
        // Keep a legacy plaintext token in memory just long enough for the
        // caller's immediately-following secrets migration to move it. The
        // public roster CRUD constructors always scrub tokens.
        let legacy_token = legacy.auth_token.clone().filter(|token| !token.is_empty());
        let mut entry = NamedAgentConfig::new("Default".into(), legacy)?;
        entry.id = LEGACY_DEFAULT_AGENT_ID.to_string();
        entry.config.auth_token = legacy_token;
        self.default_agent_id = Some(entry.id.clone());
        self.agents.push(entry);
        Ok(true)
    }
    /// Load global config from the platform config dir —
    /// `~/Library/Application Support/com.loopdeck.LoopDeck/config.yaml` on
    /// macOS (`~/.config/loopdeck/config.yaml` is the headless/Linux fallback).
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

    /// Save global config to the platform config dir —
    /// `~/Library/Application Support/com.loopdeck.LoopDeck/config.yaml` on
    /// macOS (`~/.config/loopdeck/config.yaml` is the headless/Linux fallback).
    ///
    /// Crash-safe via [`persist::atomic_write`] (temp + fsync + same-dir
    /// rename). Before overwriting, copies the existing primary to
    /// `config.yaml.bak` so a malformed future primary can be recovered from
    /// the backup.
    ///
    /// Also applies an owner-only permission floor (0600 on Unix) as
    /// defense-in-depth: the auth token itself lives in the local secrets file
    /// now (see `secrets`), but this file still holds provider config, so we
    /// don't rely on the process umask to keep it private.
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

    /// Migrate the legacy singleton and any plaintext credentials into the
    /// UUID-keyed roster and per-agent secrets files.
    ///
    /// Returns:
    /// - `Ok(true)` — the caller should save; a legacy field or plaintext
    ///   token was scrubbed from the registry.
    /// - `Ok(false)` — no registry change was needed.
    /// - `Err` — a token was present but the secrets file write failed. The
    ///   plaintext is restored in memory rather than silently lost.
    pub fn migrate_auth_token_to_secrets_file(&mut self) -> Result<bool, AppError> {
        let mut changed = self.migrate_legacy_agent_to_roster()?;
        let default_id = self
            .default_named_agent_config()
            .map(|agent| agent.id.clone());

        for agent in &mut self.agents {
            let Some(token) = agent.config.auth_token.take() else {
                continue;
            };
            if token.is_empty() {
                // Empty legacy values are not credentials; retain prior
                // behaviour by keeping them in memory until the next edit.
                agent.config.auth_token = Some(token);
                continue;
            }
            if let Err(e) = crate::secrets::store_agent_auth_token(&agent.id, &token) {
                agent.config.auth_token = Some(token);
                return Err(e);
            }
            changed = true;
            debug!(agent_id = %agent.id, "migrated plaintext auth token to per-agent secrets file");
        }

        // The old singleton file belongs to the selected default entry. Copy
        // rather than overwrite so a token explicitly stored for that UUID
        // always wins. This is safe to repeat after a crash.
        if let Some(id) = default_id {
            changed |= crate::secrets::migrate_legacy_auth_token_to_agent(&id)?;
        }
        Ok(changed)
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

    /// Path to the config directory. Platform-resolved via
    /// `directories::ProjectDirs::config_dir()`: `~/Library/Application
    /// Support/com.loopdeck.LoopDeck/` on macOS, `~/.config/loopdeck/` on
    /// Linux / as the headless fallback.
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

    /// Full path to the config file: `<config_dir>/config.yaml`
    pub fn config_path() -> Result<PathBuf, AppError> {
        Ok(Self::config_dir()?.join("config.yaml"))
    }
}

/// Lock the config file down to owner-only. Best-effort: a failure here is
/// logged but not fatal (the file's contents are no longer secret once the
/// auth token has moved to the local secrets file; this is defense-in-depth).
#[cfg(unix)]
fn restrict_file_perms(path: &Path) {
    use std::os::unix::fs::PermissionsExt;
    if let Err(e) = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600)) {
        tracing::warn!("failed to set 0600 on {}: {e}", path.display());
    }
}

/// No-op on non-Unix: the config file lives under `%APPDATA%` / `~/Library`,
/// which the OS already scopes to the current user via ACLs. There is no
/// portable `chmod` equivalent.
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
            agents: vec![],
            default_agent_id: None,
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
            agents: vec![],
            default_agent_id: None,
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
            agents: vec![],
            default_agent_id: None,
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
    fn agent_config_defaults_legacy_files_to_claude_harness() {
        let agent: AgentConfig = serde_yaml::from_str(
            "model: claude-sonnet-4-6\n\
             effort: high\n",
        )
        .unwrap();
        assert_eq!(agent.harness, AgentHarness::Claude);
    }

    #[test]
    fn agent_config_persists_codex_harness() {
        let agent = AgentConfig {
            harness: AgentHarness::Codex,
            model: Some("gpt-test".into()),
            ..Default::default()
        };
        let yaml = serde_yaml::to_string(&agent).unwrap();
        assert!(yaml.contains("harness: codex"));
        let decoded: AgentConfig = serde_yaml::from_str(&yaml).unwrap();
        assert_eq!(decoded.harness, AgentHarness::Codex);
    }

    #[test]
    fn test_agent_config_serialize_skips_none() {
        // Only set model — other fields should be absent from YAML output
        let agent = AgentConfig {
            harness: AgentHarness::Claude,
            auth_token: None,
            base_url: None,
            model: Some("claude-haiku-4-5".into()),
            effort: None,
            ..Default::default()
        };

        let config = GlobalConfig {
            agent: Some(agent),
            agents: vec![],
            default_agent_id: None,
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
            harness: AgentHarness::Claude,
            auth_token: None,
            base_url: Some("https://api.anthropic.com".into()),
            model: Some("claude-sonnet-4-5".into()),
            effort: None,
            has_auth_token: false,
        };
        let config = GlobalConfig {
            agent: Some(agent),
            agents: vec![],
            default_agent_id: None,
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
            harness: AgentHarness::Claude,
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

    // ── Legacy singleton → roster migration (offline paths) ──────────────
    //
    // The "token present" branch writes to the real secrets file
    // (`<config_dir>/agent_token`), so it is not exercised here. The
    // underlying store/load round-trip is covered hermetically by
    // `secrets::tests::file_backend_roundtrip`. These tests exercise only the
    // early-return paths that never touch the secrets file.

    #[test]
    fn migrate_legacy_agent_noop_when_no_agent_block() {
        let mut config = GlobalConfig::default();
        assert!(!config.migrate_legacy_agent_to_roster().unwrap());
        assert!(config.agent.is_none());
    }

    #[test]
    fn migrates_legacy_agent_without_token_to_default_roster_entry() {
        let mut config = GlobalConfig {
            agent: Some(AgentConfig {
                harness: AgentHarness::Claude,
                auth_token: None,
                base_url: Some("https://api.anthropic.com".into()),
                model: Some("claude-sonnet-4-5".into()),
                effort: None,
                has_auth_token: false,
            }),
            ..Default::default()
        };
        assert!(config.migrate_legacy_agent_to_roster().unwrap());
        assert!(config.agent.is_none());
        let default = config.default_named_agent_config().unwrap();
        assert_eq!(default.name, "Default");
        assert_eq!(default.id, LEGACY_DEFAULT_AGENT_ID);
        assert!(default.config.auth_token.is_none());
        assert!(!config.migrate_legacy_agent_to_roster().unwrap());
    }

    #[test]
    fn legacy_agent_empty_token_is_scrubbed_from_roster() {
        let mut config = GlobalConfig {
            agent: Some(AgentConfig {
                harness: AgentHarness::Claude,
                auth_token: Some(String::new()),
                base_url: None,
                model: None,
                effort: None,
                has_auth_token: false,
            }),
            ..Default::default()
        };
        assert!(config.migrate_legacy_agent_to_roster().unwrap());
        assert_eq!(
            config
                .default_named_agent_config()
                .unwrap()
                .config
                .auth_token
                .as_deref(),
            None
        );
    }

    #[test]
    fn named_roster_persists_ids_default_and_flat_config() {
        let mut config = GlobalConfig::default();
        let first = config
            .create_agent_config(
                "Claude — primary".into(),
                AgentConfig {
                    model: Some("claude-opus".into()),
                    auth_token: Some("must-not-persist".into()),
                    has_auth_token: true,
                    ..Default::default()
                },
            )
            .unwrap();
        let second = config
            .create_agent_config("Codex".into(), AgentConfig::default())
            .unwrap();
        config.set_default_agent_config(&second.id).unwrap();

        let yaml = serde_yaml::to_string(&config).unwrap();
        assert!(yaml.contains("agents:"));
        assert!(yaml.contains(&first.id));
        assert!(yaml.contains("default_agent_id"));
        assert!(!yaml.contains("must-not-persist"));
        assert!(!yaml.contains("has_auth_token"));

        let parsed: GlobalConfig = serde_yaml::from_str(&yaml).unwrap();
        assert_eq!(parsed.agents.len(), 2);
        assert_eq!(parsed.default_agent_id.as_deref(), Some(second.id.as_str()));
        assert_eq!(
            parsed.find_agent_config(&first.id).unwrap().name,
            "Claude — primary"
        );
    }

    #[test]
    fn roster_rejects_case_insensitive_duplicate_names_and_keeps_ids_immutable() {
        let mut config = GlobalConfig::default();
        let created = config
            .create_agent_config("Codex".into(), AgentConfig::default())
            .unwrap();
        assert!(config
            .create_agent_config("  codex  ".into(), AgentConfig::default())
            .is_err());

        let updated = config
            .update_agent_config(
                &created.id,
                "Codex — fast".into(),
                AgentConfig {
                    model: Some("gpt-fast".into()),
                    ..Default::default()
                },
            )
            .unwrap();
        assert_eq!(updated.id, created.id);
        assert_eq!(updated.name, "Codex — fast");
    }

    #[test]
    fn deleting_default_selects_remaining_agent() {
        let mut config = GlobalConfig::default();
        let first = config
            .create_agent_config("First".into(), AgentConfig::default())
            .unwrap();
        let second = config
            .create_agent_config("Second".into(), AgentConfig::default())
            .unwrap();
        config.set_default_agent_config(&second.id).unwrap();
        config.delete_agent_config(&second.id).unwrap();
        assert_eq!(config.default_agent_id.as_deref(), Some(first.id.as_str()));
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
    fn test_next_steps_zero_is_still_serialized() {
        // Unlike `run_state`/`autonomous`, `next_steps_total`/`next_steps_done`
        // must always be present on the wire — including `0` — because the
        // frontend interpolates `next_steps_done` directly (e.g. "0 of 3 steps
        // complete"). Omitting it when zero previously produced "undefined of
        // 3 steps complete" for a waiting project with no steps checked yet.
        let entry = ProjectEntry {
            path: PathBuf::from("/tmp/x"),
            name: "X".into(),
            ..Default::default()
        };
        let yaml = serde_yaml::to_string(&entry).unwrap();
        assert!(yaml.contains("next_steps_total: 0"));
        assert!(yaml.contains("next_steps_done: 0"));
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
            harness: AgentHarness::Claude,
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
