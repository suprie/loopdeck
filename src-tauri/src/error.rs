use serde::Serialize;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum AppError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("YAML serialization error: {0}")]
    Yaml(#[from] serde_yaml::Error),

    #[error("Walkdir error: {0}")]
    Walkdir(#[from] walkdir::Error),

    #[error("Project not found: {0}")]
    ProjectNotFound(String),

    #[error("Invalid path: {0}")]
    InvalidPath(String),

    #[error("Scan error: {0}")]
    Scan(String),

    #[error("Configuration error: {0}")]
    Config(String),

    #[error("Lock poisoned")]
    LockError,

    #[error("Project already exists at: {0}")]
    ProjectAlreadyExists(String),

    #[error("Agent error: {0}")]
    Agent(String),
}

impl Serialize for AppError {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;
        let mut state = serializer.serialize_struct("AppError", 3)?;
        state.serialize_field("message", &self.to_string())?;
        state.serialize_field(
            "kind",
            match self {
                AppError::Io(_) => "io",
                AppError::Yaml(_) => "yaml",
                AppError::Walkdir(_) => "walkdir",
                AppError::ProjectNotFound(_) => "projectNotFound",
                AppError::InvalidPath(_) => "invalidPath",
                AppError::Scan(_) => "scan",
                AppError::Config(_) => "config",
                AppError::LockError => "lockError",
                AppError::ProjectAlreadyExists(_) => "projectAlreadyExists",
                AppError::Agent(_) => "agent",
            },
        )?;
        state.end()
    }
}
