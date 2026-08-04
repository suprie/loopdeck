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

    #[error("Execution state error: {0}")]
    ExecutionState(String),

    #[error("Run plan error: {0}")]
    RunPlan(String),

    #[error("Lock poisoned")]
    LockError,

    #[error("Project already exists at: {0}")]
    ProjectAlreadyExists(String),

    #[error("Conflict: {0}")]
    Conflict(String),

    #[error("Agent error: {0}")]
    Agent(String),

    /// A turn ended because it was parked on an unanswered `AskUserQuestion` /
    /// manual-approval / plan-approval card past `TURN_DEADLINE`, not because
    /// of a transport/child failure. Distinct from `Agent` so callers driving
    /// unattended runs (`prd-run-queue`'s executor) can tell "nobody was there
    /// to answer" apart from a genuine turn failure — the former parks the
    /// phase and moves on; the latter fails it.
    ///
    /// `parked_questions_json`: when the park was on an `AskUserQuestion`, the
    /// full `Vec<AskUserQuestionSpec>` serialized as JSON so the morning report
    /// can reconstruct the question cards. `None` for manual approvals and plan
    /// approvals (which carry their own detail text).
    #[error("Turn parked: {detail}")]
    TurnParked {
        detail: String,
        parked_questions_json: Option<String>,
    },

    #[error("Background task failed: {0}")]
    BlockingTask(String),

    #[error("Resource limit exceeded: {0}")]
    Limit(String),
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
                AppError::ExecutionState(_) => "executionState",
                AppError::RunPlan(_) => "runPlan",
                AppError::LockError => "lockError",
                AppError::ProjectAlreadyExists(_) => "projectAlreadyExists",
                AppError::Conflict(_) => "conflict",
                AppError::Agent(_) => "agent",
                AppError::TurnParked { .. } => "turnParked",
                AppError::BlockingTask(_) => "blockingTask",
                AppError::Limit(_) => "limit",
            },
        )?;
        state.end()
    }
}
