//! Pure lifecycle transitions for the execution-foundations domain contract.
//!
//! This module deliberately has no filesystem, Tauri, provider, or clock
//! dependencies. Callers own persistence and pass the resulting status to the
//! existing file-backed models. Keeping the edge table here makes invalid
//! transitions explicit before the operational store is introduced.

use crate::error::AppError;
use crate::runplan::RunPhaseStatus;

/// Run-level lifecycle states shared by interactive, queued, and multi-agent
/// execution. `Completed`, `Delivered`, and the other terminal states never
/// transition again; a retry creates a new attempt rather than reopening one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunStatus {
    Queued,
    Running,
    /// Blocked on an upstream handoff dependency, not on a human.
    Waiting,
    Parked,
    Cancelled,
    Killed,
    Interrupted,
    Delivered,
    Completed,
    Failed,
}

/// A human decision/approval lifecycle. The payload and provider request ID
/// remain owned by the decision service; this enum only validates the edge.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DecisionStatus {
    Pending,
    Answered,
    Rejected,
    Cancelled,
    Expired,
}

/// Delivery evidence lifecycle. Verification is terminal for one delivery
/// attempt; a retry is represented by a new idempotency key/attempt.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeliveryStatus {
    Pending,
    Verified,
    Failed,
}

/// Validate and return a run transition.
pub fn transition_run(from: RunStatus, to: RunStatus) -> Result<RunStatus, AppError> {
    let allowed = match from {
        RunStatus::Queued => matches!(
            to,
            RunStatus::Running | RunStatus::Cancelled | RunStatus::Waiting
        ),
        RunStatus::Running => matches!(
            to,
            RunStatus::Parked
                | RunStatus::Cancelled
                | RunStatus::Killed
                | RunStatus::Interrupted
                | RunStatus::Delivered
                | RunStatus::Completed
                | RunStatus::Failed
        ),
        // `Waiting` is a scheduler hold, not a terminal or human-parked
        // state: a satisfied dependency re-queues it, a terminal-bad upstream
        // or the run's end parks it, and cancellation/budgets kill it.
        RunStatus::Waiting => matches!(
            to,
            RunStatus::Queued
                | RunStatus::Running
                | RunStatus::Parked
                | RunStatus::Cancelled
                | RunStatus::Killed
        ),
        RunStatus::Parked => matches!(
            to,
            RunStatus::Queued | RunStatus::Cancelled | RunStatus::Killed
        ),
        RunStatus::Delivered => matches!(to, RunStatus::Completed),
        RunStatus::Cancelled
        | RunStatus::Killed
        | RunStatus::Interrupted
        | RunStatus::Completed
        | RunStatus::Failed => false,
    };
    if allowed {
        Ok(to)
    } else {
        Err(AppError::InvalidTransition {
            entity: "run".into(),
            from: format!("{from:?}"),
            to: format!("{to:?}"),
            reason: "the source state is terminal or the edge is not defined".into(),
        })
    }
}

/// Validate a phase transition using the existing persisted phase enum.
pub fn transition_phase(
    from: RunPhaseStatus,
    to: RunPhaseStatus,
) -> Result<RunPhaseStatus, AppError> {
    let from_run = run_status_for_phase(from);
    let to_run = run_status_for_phase(to);
    transition_run(from_run, to_run).map(|_| to)
}

/// Validate a decision/approval transition.
pub fn transition_decision(
    from: DecisionStatus,
    to: DecisionStatus,
) -> Result<DecisionStatus, AppError> {
    let allowed = from == DecisionStatus::Pending
        && matches!(
            to,
            DecisionStatus::Answered
                | DecisionStatus::Rejected
                | DecisionStatus::Cancelled
                | DecisionStatus::Expired
        );
    if allowed {
        Ok(to)
    } else {
        Err(AppError::InvalidTransition {
            entity: "decision".into(),
            from: format!("{from:?}"),
            to: format!("{to:?}"),
            reason: "a decision can only be resolved once from pending".into(),
        })
    }
}

/// Validate a delivery-evidence transition.
pub fn transition_delivery(
    from: DeliveryStatus,
    to: DeliveryStatus,
) -> Result<DeliveryStatus, AppError> {
    let allowed = from == DeliveryStatus::Pending
        && matches!(to, DeliveryStatus::Verified | DeliveryStatus::Failed);
    if allowed {
        Ok(to)
    } else {
        Err(AppError::InvalidTransition {
            entity: "delivery".into(),
            from: format!("{from:?}"),
            to: format!("{to:?}"),
            reason: "delivery evidence resolves once; retries use a new attempt".into(),
        })
    }
}

fn run_status_for_phase(status: RunPhaseStatus) -> RunStatus {
    match status {
        RunPhaseStatus::Queued => RunStatus::Queued,
        RunPhaseStatus::Running => RunStatus::Running,
        RunPhaseStatus::Waiting => RunStatus::Waiting,
        RunPhaseStatus::Parked => RunStatus::Parked,
        RunPhaseStatus::Delivered => RunStatus::Delivered,
        RunPhaseStatus::Completed => RunStatus::Completed,
        RunPhaseStatus::Failed => RunStatus::Failed,
        RunPhaseStatus::Interrupted => RunStatus::Interrupted,
        RunPhaseStatus::Killed => RunStatus::Killed,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn run_transition_matrix_covers_every_edge() {
        let all = [
            RunStatus::Queued,
            RunStatus::Running,
            RunStatus::Waiting,
            RunStatus::Parked,
            RunStatus::Cancelled,
            RunStatus::Killed,
            RunStatus::Interrupted,
            RunStatus::Delivered,
            RunStatus::Completed,
            RunStatus::Failed,
        ];
        let allowed = [
            (
                RunStatus::Queued,
                &[RunStatus::Running, RunStatus::Cancelled, RunStatus::Waiting][..],
            ),
            (
                RunStatus::Waiting,
                &[
                    RunStatus::Queued,
                    RunStatus::Running,
                    RunStatus::Parked,
                    RunStatus::Cancelled,
                    RunStatus::Killed,
                ][..],
            ),
            (
                RunStatus::Running,
                &[
                    RunStatus::Parked,
                    RunStatus::Cancelled,
                    RunStatus::Killed,
                    RunStatus::Interrupted,
                    RunStatus::Delivered,
                    RunStatus::Completed,
                    RunStatus::Failed,
                ][..],
            ),
            (
                RunStatus::Parked,
                &[RunStatus::Queued, RunStatus::Cancelled, RunStatus::Killed][..],
            ),
            (RunStatus::Delivered, &[RunStatus::Completed][..]),
            (RunStatus::Cancelled, &[][..]),
            (RunStatus::Killed, &[][..]),
            (RunStatus::Interrupted, &[][..]),
            (RunStatus::Completed, &[][..]),
            (RunStatus::Failed, &[][..]),
        ];

        for (from, destinations) in allowed {
            for to in all {
                let result = transition_run(from, to);
                if destinations.contains(&to) {
                    assert_eq!(result.unwrap(), to, "expected {from:?} -> {to:?}");
                } else {
                    assert_invalid(result, "run", from, to);
                }
            }
        }
    }

    #[test]
    fn duplicate_terminal_commands_are_rejected_for_every_terminal_status() {
        for terminal in [
            RunStatus::Cancelled,
            RunStatus::Killed,
            RunStatus::Interrupted,
            RunStatus::Completed,
            RunStatus::Failed,
        ] {
            assert_invalid(
                transition_run(terminal, terminal),
                "run",
                terminal,
                terminal,
            );
            assert_invalid(
                transition_run(terminal, RunStatus::Running),
                "run",
                terminal,
                RunStatus::Running,
            );
        }
    }

    #[test]
    fn decision_transition_matrix_allows_one_resolution_only() {
        let resolutions = [
            DecisionStatus::Answered,
            DecisionStatus::Rejected,
            DecisionStatus::Cancelled,
            DecisionStatus::Expired,
        ];
        for to in resolutions {
            assert_eq!(
                transition_decision(DecisionStatus::Pending, to).unwrap(),
                to
            );
            assert_invalid(
                transition_decision(to, DecisionStatus::Pending),
                "decision",
                to,
                DecisionStatus::Pending,
            );
            assert_invalid(transition_decision(to, to), "decision", to, to);
        }
    }

    #[test]
    fn delivery_transition_matrix_rejects_retrying_a_resolved_attempt() {
        for to in [DeliveryStatus::Verified, DeliveryStatus::Failed] {
            assert_eq!(
                transition_delivery(DeliveryStatus::Pending, to).unwrap(),
                to
            );
        }
        for from in [DeliveryStatus::Verified, DeliveryStatus::Failed] {
            for to in [
                DeliveryStatus::Pending,
                DeliveryStatus::Verified,
                DeliveryStatus::Failed,
            ] {
                assert_invalid(transition_delivery(from, to), "delivery", from, to);
            }
        }
    }

    #[test]
    fn waiting_is_a_scheduler_hold_not_a_terminal_state() {
        // Queued -> Waiting -> Queued when the dependency is satisfied.
        assert_eq!(
            transition_phase(RunPhaseStatus::Queued, RunPhaseStatus::Waiting).unwrap(),
            RunPhaseStatus::Waiting
        );
        assert_eq!(
            transition_phase(RunPhaseStatus::Waiting, RunPhaseStatus::Queued).unwrap(),
            RunPhaseStatus::Queued
        );
        // A terminal-bad upstream or the run's end parks it.
        assert_eq!(
            transition_phase(RunPhaseStatus::Waiting, RunPhaseStatus::Parked).unwrap(),
            RunPhaseStatus::Parked
        );
        // Cancellation and budgets kill it; a direct dispatch is allowed.
        assert_eq!(
            transition_phase(RunPhaseStatus::Waiting, RunPhaseStatus::Killed).unwrap(),
            RunPhaseStatus::Killed
        );
        assert_eq!(
            transition_phase(RunPhaseStatus::Waiting, RunPhaseStatus::Running).unwrap(),
            RunPhaseStatus::Running
        );
        // Waiting never masquerades as success or as an ordinary failure.
        assert_invalid(
            transition_phase(RunPhaseStatus::Waiting, RunPhaseStatus::Completed),
            "run",
            RunStatus::Waiting,
            RunStatus::Completed,
        );
        assert_invalid(
            transition_phase(RunPhaseStatus::Waiting, RunPhaseStatus::Failed),
            "run",
            RunStatus::Waiting,
            RunStatus::Failed,
        );
    }

    #[test]
    fn phase_restart_reconciliation_uses_running_to_interrupted_edge() {
        assert_eq!(
            transition_phase(RunPhaseStatus::Running, RunPhaseStatus::Interrupted).unwrap(),
            RunPhaseStatus::Interrupted
        );
    }

    fn assert_invalid<T: std::fmt::Debug>(
        result: Result<T, AppError>,
        entity: &str,
        from: impl std::fmt::Debug,
        to: impl std::fmt::Debug,
    ) {
        match result {
            Err(AppError::InvalidTransition {
                entity: actual_entity,
                from: actual_from,
                to: actual_to,
                ..
            }) => {
                assert_eq!(actual_entity, entity);
                assert_eq!(actual_from, format!("{from:?}"));
                assert_eq!(actual_to, format!("{to:?}"));
            }
            other => panic!("expected structured invalid transition, got {other:?}"),
        }
    }
}
