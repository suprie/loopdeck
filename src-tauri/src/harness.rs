//! Provider-neutral agent session used by the command layer.
//!
//! Claude and Codex expose different wire protocols, but LoopDeck's UI expects
//! one stable stream (`ClaudeEvent`) and one stable terminal response
//! (`AgentResponse`). This enum keeps that protocol detail behind a small
//! delegation surface so commands, transcript persistence, retry handling, and
//! approval UI behave identically for both harnesses.

use crate::agents::{AgentResponse, ClaudeEvent};
use crate::claude_session::{ClaudeSession, InterruptSlot, ParkSlots};
use crate::codex_session::CodexSession;
use crate::config::{AgentConfig, AgentHarness};
use crate::error::AppError;
use crate::permission::PermissionPolicy;
use std::path::Path;
use tauri::ipc::Channel;

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

    pub fn spawn(
        cwd: &Path,
        config: &AgentConfig,
        resume_session_id: Option<&str>,
        policy: PermissionPolicy,
    ) -> Result<Self, AppError> {
        match config.harness {
            AgentHarness::Claude => {
                // Codex ids are explicitly tagged in LoopDeck transcripts.
                // Never pass one to Claude's `--resume`.
                let resume = resume_session_id.filter(|id| !id.starts_with("codex:"));
                ClaudeSession::spawn(&cwd.to_path_buf(), config, resume, policy).map(Self::Claude)
            }
            AgentHarness::Codex => {
                // Legacy untagged ids belong to Claude. Codex ids are tagged so
                // switching harnesses cannot cross-resume incompatible state.
                let resume = resume_session_id.and_then(|id| id.strip_prefix("codex:"));
                CodexSession::spawn(cwd, config, resume, policy).map(Self::Codex)
            }
        }
    }

    pub async fn send_message(
        &mut self,
        text: &str,
        slots: &ParkSlots<'_>,
        interrupt_slot: &InterruptSlot,
    ) -> Result<AgentResponse, AppError> {
        match self {
            Self::Claude(session) => session.send_message(text, slots, interrupt_slot).await,
            Self::Codex(session) => session.send_message(text, slots, interrupt_slot).await,
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
    pub async fn send_message_streaming(
        &mut self,
        text: &str,
        channel: &Channel<ClaudeEvent>,
        slots: &ParkSlots<'_>,
        interrupt_slot: &InterruptSlot,
        plan_mode: bool,
    ) -> Result<AgentResponse, AppError> {
        match self {
            Self::Claude(session) => {
                session
                    .send_message_streaming(text, channel, slots, interrupt_slot, plan_mode)
                    .await
            }
            Self::Codex(session) => {
                if plan_mode {
                    return Err(AppError::Agent(
                        "plan mode is a Claude-only feature and is not supported by the Codex harness".into(),
                    ));
                }
                session
                    .send_message_streaming(text, channel, slots, interrupt_slot)
                    .await
            }
        }
    }
}
