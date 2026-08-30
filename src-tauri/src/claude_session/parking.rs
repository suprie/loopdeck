use crate::agents::AskUserQuestionSpec;
use crate::permission::Decision;
use std::collections::HashMap;
use std::sync::{Arc, Mutex as StdMutex};
use tokio::sync::oneshot;

#[derive(Debug, Clone)]
pub struct QuestionAnswer {
    pub labels: Vec<String>,
    pub other_text: Option<String>,
}

impl QuestionAnswer {
    pub fn as_single(&self) -> String {
        self.labels
            .first()
            .cloned()
            .or_else(|| self.other_text.clone())
            .unwrap_or_default()
    }

    pub fn as_multi(&self) -> Vec<String> {
        let mut answers = self.labels.clone();
        if let Some(other) = self
            .other_text
            .clone()
            .filter(|text| !text.trim().is_empty())
        {
            answers.push(other);
        }
        answers
    }
}

pub type QuestionAnswers = HashMap<String, QuestionAnswer>;

/// The payload stays with its sender so UI remounts can restore a parked prompt.
pub struct PendingQuestion {
    pub request_id: String,
    pub questions: Vec<AskUserQuestionSpec>,
    pub sender: Option<oneshot::Sender<QuestionAnswers>>,
}

pub type QuestionSlot = Arc<StdMutex<Option<PendingQuestion>>>;

pub struct PendingPermission {
    pub request_id: String,
    pub tool_name: String,
    pub input: String,
    pub sender: Option<oneshot::Sender<Decision>>,
}

pub type PermissionSlot = Arc<StdMutex<Option<PendingPermission>>>;

pub struct PendingPlan {
    pub request_id: String,
    pub plan: String,
    pub sender: Option<oneshot::Sender<Decision>>,
}

pub type PlanSlot = Arc<StdMutex<Option<PendingPlan>>>;

pub struct ParkSlots<'a> {
    pub question: &'a QuestionSlot,
    pub permission: &'a PermissionSlot,
    pub plan: &'a PlanSlot,
}

/// One-shot Stop bridge; locks are never held across an await.
pub type InterruptSlot = Arc<StdMutex<Option<oneshot::Sender<()>>>>;
