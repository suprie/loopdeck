//! Central, per-user SQLite store for mutable operational snapshots.
//!
//! Human-authored project files remain in the repository. The first read of a
//! legacy operational document imports it once; subsequent writes stay in the
//! app data directory, shared by the project's managed worktrees.

use crate::error::AppError;
use sqlx::{
    sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions},
    Row, SqlitePool,
};
use std::future::Future;
use std::path::{Path, PathBuf};

const SCHEMA_VERSION: i64 = 1;
// Existing callers expose synchronous persistence. Serialize their short DB
// operations on worker threads; providers never hold this lock while running.
static STORE_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn app_database_path(_project: &Path) -> Result<PathBuf, AppError> {
    #[cfg(test)]
    {
        use std::sync::OnceLock;
        static TEST_DB: OnceLock<PathBuf> = OnceLock::new();
        let _ = _project;
        return Ok(TEST_DB
            .get_or_init(|| {
                std::env::temp_dir()
                    .join(format!("loopdeck-tests-{}", std::process::id()))
                    .join("operational.sqlite3")
            })
            .clone());
    }
    #[cfg(not(test))]
    {
        let dirs =
            directories::ProjectDirs::from("com", "loopdeck", "LoopDeck").ok_or_else(|| {
                AppError::OperationalStore("cannot resolve app data directory".into())
            })?;
        Ok(dirs.data_local_dir().join("operational.sqlite3"))
    }
}

/// Resolve a repository/worktree to the shared git common directory's parent.
/// Non-git project folders keep their canonical path as their identity.
pub(crate) fn project_key(path: &Path) -> Result<String, AppError> {
    let canonical = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    if let Some(common_dir) = crate::git::common_dir(&canonical) {
        let common_dir = common_dir.canonicalize().unwrap_or(common_dir);
        if common_dir.file_name().is_some_and(|name| name == ".git") {
            if let Some(root) = common_dir.parent() {
                return Ok(root.to_string_lossy().into_owned());
            }
        }
    }
    Ok(canonical.to_string_lossy().into_owned())
}

fn runtime() -> Result<tokio::runtime::Runtime, AppError> {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|error| AppError::OperationalStore(error.to_string()))
}

fn sync<T, F, Fut>(make_future: F) -> Result<T, AppError>
where
    T: Send + 'static,
    F: FnOnce() -> Fut + Send + 'static,
    Fut: Future<Output = Result<T, AppError>> + Send + 'static,
{
    let future: std::pin::Pin<Box<dyn Future<Output = Result<T, AppError>> + Send>> =
        Box::pin(make_future());
    std::thread::spawn(move || {
        let _guard = STORE_LOCK.lock().map_err(|_| AppError::LockError)?;
        runtime()?.block_on(future)
    })
    .join()
    .map_err(|_| AppError::OperationalStore("database worker panicked".into()))?
}

async fn open(path: PathBuf) -> Result<SqlitePool, AppError> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let options = SqliteConnectOptions::new()
        .filename(&path)
        .create_if_missing(true)
        .journal_mode(SqliteJournalMode::Wal)
        .busy_timeout(std::time::Duration::from_secs(3));
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(options)
        .await
        .map_err(|error| AppError::OperationalStore(error.to_string()))?;
    sqlx::raw_sql(
        "PRAGMA foreign_keys = ON;
             CREATE TABLE IF NOT EXISTS schema_meta (
                 id INTEGER PRIMARY KEY CHECK (id = 1),
                 version INTEGER NOT NULL
             );
             INSERT OR IGNORE INTO schema_meta(id, version) VALUES(1, 1);
             CREATE TABLE IF NOT EXISTS operational_documents (
                 project_key TEXT NOT NULL,
                 kind TEXT NOT NULL,
                 payload TEXT NOT NULL,
                 source_path TEXT,
                 imported_at TEXT,
                 updated_at TEXT NOT NULL,
                 PRIMARY KEY(project_key, kind)
             );",
    )
    .execute(&pool)
    .await
    .map_err(|error| AppError::OperationalStore(error.to_string()))?;
    let row = sqlx::query("SELECT version FROM schema_meta WHERE id = 1")
        .fetch_one(&pool)
        .await
        .map_err(|error| AppError::OperationalStore(error.to_string()))?;
    let version: i64 = row
        .try_get(0)
        .map_err(|error| AppError::OperationalStore(error.to_string()))?;
    if version != SCHEMA_VERSION {
        return Err(AppError::OperationalStore(format!(
            "unsupported schema version {version} (expected {SCHEMA_VERSION})"
        )));
    }
    Ok(pool)
}

/// Read an operational snapshot, importing `legacy_path` exactly once when
/// the central record does not yet exist. The legacy file is never rewritten.
pub(crate) fn load_document(
    project: &Path,
    kind: &str,
    legacy_path: &Path,
) -> Result<Option<String>, AppError> {
    let db_path = app_database_path(project)?;
    let project = project.to_path_buf();
    let kind = kind.to_string();
    let legacy_path = legacy_path.to_path_buf();
    sync(move || load_document_at(db_path, project, kind, legacy_path))
}

async fn load_document_at(
    db_path: PathBuf,
    project: PathBuf,
    kind: String,
    legacy_path: PathBuf,
) -> Result<Option<String>, AppError> {
    let pool = open(db_path).await?;
    let key = project_key(&project)?;
    let canonical_project = project.canonicalize().unwrap_or_else(|_| project.clone());
    let legacy_relative = legacy_path
        .strip_prefix(&project)
        .or_else(|_| legacy_path.strip_prefix(&canonical_project))
        .unwrap_or(&legacy_path);
    let legacy_path = PathBuf::from(&key).join(legacy_relative);
    let existing = sqlx::query(
        "SELECT payload FROM operational_documents WHERE project_key = ?1 AND kind = ?2",
    )
    .bind(&key)
    .bind(&kind)
    .fetch_optional(&pool)
    .await
    .map_err(|error| AppError::OperationalStore(error.to_string()))?
    .map(|row| row.try_get::<String, _>(0))
    .transpose()
    .map_err(|error| AppError::OperationalStore(error.to_string()))?;
    if existing.is_some() {
        return Ok(existing);
    }
    if !legacy_path.exists() {
        return Ok(None);
    }
    // Validate before import. A malformed file must not poison the central
    // record; execution's existing loader also preserves backup recovery.
    let payload = match kind.as_str() {
        "execution" => {
            serde_yaml::to_string(&crate::execution::load_from_path(&legacy_path)?.state)?
        }
        "run_plan" => {
            crate::runplan::load_from_path(&legacy_path)?;
            crate::limits::read_bounded_to_string(&legacy_path, crate::limits::RUN_PLAN_MAX_BYTES)?
        }
        _ => crate::limits::read_bounded_to_string(&legacy_path, 2 * 1024 * 1024)?,
    };
    let imported_at = chrono::Utc::now().to_rfc3339();
    sqlx::query(
        "INSERT OR IGNORE INTO operational_documents
             (project_key, kind, payload, source_path, imported_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?5)",
    )
    .bind(key)
    .bind(&kind)
    .bind(payload)
    .bind(legacy_path.to_string_lossy().to_string())
    .bind(imported_at)
    .execute(&pool)
    .await
    .map_err(|error| AppError::OperationalStore(error.to_string()))?;
    let row = sqlx::query(
        "SELECT payload FROM operational_documents WHERE project_key = ?1 AND kind = ?2",
    )
    .bind(project_key(&project)?)
    .bind(&kind)
    .fetch_one(&pool)
    .await
    .map_err(|error| AppError::OperationalStore(error.to_string()))?;
    let imported: String = row
        .try_get(0)
        .map_err(|error| AppError::OperationalStore(error.to_string()))?;
    Ok(Some(imported))
}

pub(crate) fn save_document(project: &Path, kind: &str, payload: &str) -> Result<(), AppError> {
    let db_path = app_database_path(project)?;
    let project = project.to_path_buf();
    let kind = kind.to_string();
    let payload = payload.to_string();
    sync(move || save_document_at(db_path, project, kind, payload))
}

pub(crate) fn save_revisioned_document(
    project: &Path,
    kind: &str,
    payload: &str,
    expected_revision: u64,
) -> Result<(), AppError> {
    let db_path = app_database_path(project)?;
    let project = project.to_path_buf();
    let kind = kind.to_string();
    let payload = payload.to_string();
    sync(move || save_revisioned_document_at(db_path, project, kind, payload, expected_revision))
}

async fn save_revisioned_document_at(
    db_path: PathBuf,
    project: PathBuf,
    kind: String,
    payload: String,
    expected_revision: u64,
) -> Result<(), AppError> {
    let pool = open(db_path).await?;
    let mut tx = pool
        .begin_with("BEGIN IMMEDIATE")
        .await
        .map_err(|error| AppError::OperationalStore(error.to_string()))?;
    let key = project_key(&project)?;
    let current = sqlx::query(
        "SELECT payload FROM operational_documents WHERE project_key = ?1 AND kind = ?2",
    )
    .bind(&key)
    .bind(&kind)
    .fetch_optional(&mut *tx)
    .await
    .map_err(|error| AppError::OperationalStore(error.to_string()))?;
    let actual = current
        .map(|row| row.try_get::<String, _>(0))
        .transpose()
        .map_err(|error| AppError::OperationalStore(error.to_string()))?
        .map(|yaml| {
            let value = serde_yaml::from_str::<serde_yaml::Value>(&yaml).map_err(|error| {
                AppError::OperationalStore(format!(
                    "cannot verify stored execution revision: {error}"
                ))
            })?;
            Ok::<u64, AppError>(
                value
                    .get("revision")
                    .and_then(serde_yaml::Value::as_u64)
                    .unwrap_or(0),
            )
        })
        .transpose()?
        .unwrap_or(0);
    if actual != expected_revision {
        return Err(AppError::Conflict(format!(
            "execution state changed before this write (expected revision {expected_revision}, found {actual} in central store); reload and retry."
        )));
    }
    sqlx::query(
        "INSERT INTO operational_documents
         (project_key, kind, payload, source_path, imported_at, updated_at)
         VALUES (?1, ?2, ?3, NULL, NULL, ?4)
         ON CONFLICT(project_key, kind) DO UPDATE SET payload = excluded.payload,
             updated_at = excluded.updated_at",
    )
    .bind(key)
    .bind(kind)
    .bind(payload)
    .bind(chrono::Utc::now().to_rfc3339())
    .execute(&mut *tx)
    .await
    .map_err(|error| AppError::OperationalStore(error.to_string()))?;
    tx.commit()
        .await
        .map_err(|error| AppError::OperationalStore(error.to_string()))
}

async fn save_document_at(
    db_path: PathBuf,
    project: PathBuf,
    kind: String,
    payload: String,
) -> Result<(), AppError> {
    let pool = open(db_path).await?;
    let mut tx = pool
        .begin()
        .await
        .map_err(|error| AppError::OperationalStore(error.to_string()))?;
    sqlx::query(
        "INSERT INTO operational_documents
         (project_key, kind, payload, source_path, imported_at, updated_at)
         VALUES (?1, ?2, ?3, NULL, NULL, ?4)
         ON CONFLICT(project_key, kind) DO UPDATE SET payload = excluded.payload,
             updated_at = excluded.updated_at",
    )
    .bind(project_key(&project)?)
    .bind(kind)
    .bind(payload)
    .bind(chrono::Utc::now().to_rfc3339())
    .execute(&mut *tx)
    .await
    .map_err(|error| AppError::OperationalStore(error.to_string()))?;
    tx.commit()
        .await
        .map_err(|error| AppError::OperationalStore(error.to_string()))
}

pub(crate) fn has_document(project: &Path, kind: &str) -> Result<bool, AppError> {
    let db_path = app_database_path(project)?;
    let project = project.to_path_buf();
    let kind = kind.to_string();
    sync(move || async move {
        let pool = open(db_path).await?;
        sqlx::query("SELECT 1 FROM operational_documents WHERE project_key = ?1 AND kind = ?2")
            .bind(project_key(&project)?)
            .bind(kind)
            .fetch_optional(&pool)
            .await
            .map(|row| row.is_some())
            .map_err(|error| AppError::OperationalStore(error.to_string()))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn temp_dir(label: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("loopdeck-store-{label}-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn import_is_idempotent_and_does_not_modify_legacy_file() {
        let root = temp_dir("import");
        let db = root.join("app-data.sqlite3");
        let project = root.join("repo");
        fs::create_dir_all(project.join(".loopdeck")).unwrap();
        let legacy = project.join(".loopdeck/run-plan.yaml");
        fs::write(&legacy, "original plan").unwrap();
        assert_eq!(
            sync({
                let db = db.clone();
                let project = project.clone();
                let legacy = legacy.clone();
                move || load_document_at(db, project, "fixture".into(), legacy)
            })
            .unwrap()
            .as_deref(),
            Some("original plan")
        );
        fs::write(&legacy, "changed source must not overwrite import").unwrap();
        assert_eq!(
            sync({
                let db = db.clone();
                let project = project.clone();
                let legacy = legacy.clone();
                move || load_document_at(db, project, "fixture".into(), legacy)
            })
            .unwrap()
            .as_deref(),
            Some("original plan")
        );
        assert_eq!(
            fs::read_to_string(legacy).unwrap(),
            "changed source must not overwrite import"
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn worktree_identity_resolves_to_shared_git_root() {
        let root = temp_dir("worktree");
        let repo = root.join("repo");
        let worktree = root.join("managed");
        fs::create_dir_all(&repo).unwrap();
        let status = std::process::Command::new("git")
            .args(["init", "-q"])
            .current_dir(&repo)
            .status()
            .unwrap();
        assert!(status.success());
        fs::write(repo.join("README.md"), "fixture\n").unwrap();
        for args in [
            vec!["config", "user.email", "loopdeck-test@example.invalid"],
            vec!["config", "user.name", "LoopDeck Test"],
            vec!["add", "README.md"],
            vec!["commit", "-qm", "fixture"],
        ] {
            let status = std::process::Command::new("git")
                .args(args)
                .current_dir(&repo)
                .status()
                .unwrap();
            assert!(status.success());
        }
        let status = std::process::Command::new("git")
            .args([
                "worktree",
                "add",
                "-qb",
                "managed",
                worktree.to_str().unwrap(),
            ])
            .current_dir(&repo)
            .status()
            .unwrap();
        assert!(status.success());
        assert_eq!(project_key(&repo).unwrap(), project_key(&worktree).unwrap());
        let _ = fs::remove_dir_all(root);
    }
}
