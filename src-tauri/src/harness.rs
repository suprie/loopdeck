//! Provider-neutral agent session used by the command layer.
//!
//! Claude and Codex expose different wire protocols, but Selasar's UI expects
//! one stable stream (`ClaudeEvent`) and one stable terminal response
//! (`AgentResponse`). This enum keeps that protocol detail behind a small
//! delegation surface so commands, transcript persistence, retry handling, and
//! approval UI behave identically for both harnesses.

use crate::agents::{AgentResponse, ClaudeEvent, TokenBudget};
use crate::claude_session::{ClaudeSession, InterruptSlot, ParkSlots};
use crate::codex_session::CodexSession;
use crate::config::{AgentConfig, AgentHarness};
use crate::conversation::Attachment;
use crate::error::AppError;
use crate::permission::PermissionPolicy;
use std::path::Path;
use tauri::ipc::Channel;

/// Provider adapter contract used by the heterogeneous session owner below.
///
/// The two command-line harnesses intentionally retain concrete session types:
/// their wire protocols, child-process lifetimes, and internal control flows
/// differ substantially. Their command-facing surface, however, is identical.
/// Keeping that surface in a crate-private trait makes additions deliberate and
/// prevents the enum from becoming a second, drifting implementation of every
/// turn operation. The enum remains the owner so we do not need dynamic async
/// dispatch (or another dependency) for cached mixed-provider sessions.
#[allow(async_fn_in_trait)]
pub(crate) trait HarnessAdapter: Sized {
    fn spawn(
        cwd: &Path,
        config: &AgentConfig,
        resume_session_id: Option<&str>,
        policy: PermissionPolicy,
    ) -> Result<Self, AppError>;

    /// Whether this cached provider process can safely accept another turn.
    fn is_usable(&mut self) -> bool;

    async fn send_message(
        &mut self,
        text: &str,
        attachments: &[Attachment],
        slots: &ParkSlots<'_>,
        interrupt_slot: &InterruptSlot,
    ) -> Result<AgentResponse, AppError>;

    #[allow(clippy::too_many_arguments)]
    async fn send_message_streaming(
        &mut self,
        text: &str,
        attachments: &[Attachment],
        channel: &Channel<ClaudeEvent>,
        slots: &ParkSlots<'_>,
        interrupt_slot: &InterruptSlot,
        plan_mode: bool,
        token_budget: Option<&TokenBudget>,
    ) -> Result<AgentResponse, AppError>;
}

pub enum HarnessSession {
    Claude(ClaudeSession),
    Codex(CodexSession),
}

impl HarnessSession {
    pub fn harness(&self) -> AgentHarness {
        match self {
            Self::Claude(_) => AgentHarness::Claude,
            Self::Codex(_) => AgentHarness::Codex,
        }
    }

    /// Whether this cached harness can safely accept another turn.
    ///
    /// Codex marks itself unusable when a wedged child must be terminated
    /// after ignoring Stop; `with_session` then replaces it on the next send.
    pub fn is_usable(&mut self) -> bool {
        match self {
            Self::Claude(session) => HarnessAdapter::is_usable(session),
            Self::Codex(session) => HarnessAdapter::is_usable(session),
        }
    }

    pub fn spawn(
        cwd: &Path,
        config: &AgentConfig,
        resume_session_id: Option<&str>,
        policy: PermissionPolicy,
    ) -> Result<Self, AppError> {
        // Single choke point for role-scoped autonomy: every spawn path
        // (interactive, run-queue, multi-agent) funnels through here, so the
        // charter's rules ride the policy regardless of who spawned us.
        let policy = policy.with_role_rules(config.charter.as_ref().and_then(|c| c.rules.clone()));
        match config.harness {
            AgentHarness::Claude => {
                // Codex ids are explicitly tagged in Selasar transcripts.
                // Never pass one to Claude's `--resume`.
                let resume = resume_session_id.filter(|id| !id.starts_with("codex:"));
                HarnessAdapter::spawn(cwd, config, resume, policy).map(Self::Claude)
            }
            AgentHarness::Codex => {
                // Legacy untagged ids belong to Claude. Codex ids are tagged so
                // switching harnesses cannot cross-resume incompatible state.
                let resume = resume_session_id.and_then(|id| id.strip_prefix("codex:"));
                HarnessAdapter::spawn(cwd, config, resume, policy).map(Self::Codex)
            }
        }
    }

    pub async fn send_message(
        &mut self,
        text: &str,
        attachments: &[Attachment],
        slots: &ParkSlots<'_>,
        interrupt_slot: &InterruptSlot,
    ) -> Result<AgentResponse, AppError> {
        match self {
            Self::Claude(session) => {
                HarnessAdapter::send_message(session, text, attachments, slots, interrupt_slot)
                    .await
            }
            Self::Codex(session) => {
                HarnessAdapter::send_message(session, text, attachments, slots, interrupt_slot)
                    .await
            }
        }
    }

    /// `plan_mode` only has meaning for Claude — it toggles the CLI's `plan`
    /// permission mode, entirely a Claude-CLI concept (`ExitPlanMode`, the
    /// `PlanSlot` carried inside `slots`). Codex has its own approval model
    /// (always `readOnly` + `on-request` — see the harness-boundary decision
    /// in `.loopdeck/decisions.md`) with no equivalent read-only-until-approved
    /// gate, so `plan_mode: true` against a Codex session is a hard error
    /// rather than a silently-dropped flag: Codex would otherwise start the
    /// turn with its normal `workspace-write` access despite the caller
    /// believing it asked for a plan-first review. This is the single choke
    /// point for that guarantee — any caller (not just the current frontend
    /// toggle, which separately fails closed while the harness is unknown)
    /// gets the same rejection.
    #[allow(clippy::too_many_arguments)]
    pub async fn send_message_streaming(
        &mut self,
        text: &str,
        attachments: &[Attachment],
        channel: &Channel<ClaudeEvent>,
        slots: &ParkSlots<'_>,
        interrupt_slot: &InterruptSlot,
        plan_mode: bool,
        token_budget: Option<&TokenBudget>,
    ) -> Result<AgentResponse, AppError> {
        match self {
            Self::Claude(session) => {
                HarnessAdapter::send_message_streaming(
                    session,
                    text,
                    attachments,
                    channel,
                    slots,
                    interrupt_slot,
                    plan_mode,
                    token_budget,
                )
                .await
            }
            Self::Codex(session) => {
                HarnessAdapter::send_message_streaming(
                    session,
                    text,
                    attachments,
                    channel,
                    slots,
                    interrupt_slot,
                    plan_mode,
                    token_budget,
                )
                .await
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::HarnessAdapter;
    use crate::claude_session::ClaudeSession;
    use crate::codex_session::CodexSession;

    // This intentionally compiles the complete shared surface for both
    // providers. If a future provider changes one method's contract, the
    // adapter cannot silently regress into enum-specific dispatch.
    fn assert_adapter_contract<T: HarnessAdapter>() {}

    #[test]
    fn concrete_sessions_implement_the_shared_adapter_contract() {
        assert_adapter_contract::<ClaudeSession>();
        assert_adapter_contract::<CodexSession>();
    }
}
