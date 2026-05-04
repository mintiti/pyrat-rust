//! Public event stream emitted by the orchestrator.
//!
//! `OrchestratorEvent<D>` is a runtime type. It is *not* required to be
//! `Serialize`: it carries flatbuffers-generated host types (`MatchEvent`,
//! `MatchResult`, `PlayerIdentity`) that aren't serde-friendly. Persistence
//! flows through sink callbacks (sinks extract fields and write them);
//! replay flows through the `ReplayEvent` DTO.

use pyrat_host::match_host::MatchEvent;
use pyrat_host::player::PlayerIdentity;

use crate::descriptor::Descriptor;
use crate::id::MatchId;
use crate::outcome::{MatchFailure, MatchOutcome};

/// One event from the orchestrator's merged stream.
///
/// Ordering rules enforced by the executor:
/// - `MatchQueued` is the first event for a given id.
/// - `MatchStarted` is published once players are identified.
/// - `MatchEvent` carries every non-terminal host event tagged with id.
/// - `MatchFinished` / `MatchFailed` are the only terminal signals; the
///   host's `MatchEvent::MatchOver` is suppressed (the canonical terminal
///   value is the `MatchResult` carried inside `MatchFinished.outcome`).
#[derive(Debug, Clone)]
#[non_exhaustive]
pub enum OrchestratorEvent<D: Descriptor> {
    MatchQueued {
        id: MatchId,
        descriptor: D,
    },
    MatchStarted {
        id: MatchId,
        descriptor: D,
        players: [PlayerIdentity; 2],
    },
    MatchEvent {
        id: MatchId,
        event: MatchEvent,
    },
    MatchFinished {
        outcome: MatchOutcome<D>,
    },
    MatchFailed {
        failure: MatchFailure<D>,
    },
}

impl<D: Descriptor> OrchestratorEvent<D> {
    /// `MatchId` carried by every event variant.
    pub fn match_id(&self) -> MatchId {
        match self {
            Self::MatchQueued { id, .. } | Self::MatchStarted { id, .. } => *id,
            Self::MatchEvent { id, .. } => *id,
            Self::MatchFinished { outcome } => outcome.descriptor.match_id(),
            Self::MatchFailed { failure } => failure.descriptor.match_id(),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::SystemTime;

    use pyrat_host::match_host::{MatchEvent, MatchResult};
    use pyrat_host::player::PlayerIdentity;
    use pyrat_host::wire::{GameResult, Player};

    use super::*;
    use crate::descriptor::AdHocDescriptor;
    use crate::outcome::{FailureReason, MatchFailure, MatchOutcome};

    fn descriptor(id: u64) -> AdHocDescriptor {
        AdHocDescriptor {
            match_id: MatchId(id),
            seed: 0,
            planned_at: SystemTime::UNIX_EPOCH,
        }
    }

    fn identity(slot: Player) -> PlayerIdentity {
        PlayerIdentity {
            name: "test".into(),
            author: "test".into(),
            agent_id: "test".into(),
            slot,
        }
    }

    fn match_result() -> MatchResult {
        MatchResult {
            result: GameResult::Draw,
            player1_score: 0.0,
            player2_score: 0.0,
            turns_played: 0,
        }
    }

    /// `MatchQueued`/`MatchStarted`/`MatchEvent` return the explicit `id`
    /// field; `MatchFinished`/`MatchFailed` route through `descriptor.match_id()`.
    /// Catches branch transposition in the accessor the executor routes on.
    #[test]
    fn match_id_returns_correct_id_for_every_variant() {
        let queued = OrchestratorEvent::<AdHocDescriptor>::MatchQueued {
            id: MatchId(1),
            descriptor: descriptor(1),
        };
        assert_eq!(queued.match_id(), MatchId(1));

        let started = OrchestratorEvent::<AdHocDescriptor>::MatchStarted {
            id: MatchId(2),
            descriptor: descriptor(2),
            players: [identity(Player::Player1), identity(Player::Player2)],
        };
        assert_eq!(started.match_id(), MatchId(2));

        let event = OrchestratorEvent::<AdHocDescriptor>::MatchEvent {
            id: MatchId(3),
            event: MatchEvent::PreprocessingStarted,
        };
        assert_eq!(event.match_id(), MatchId(3));

        let finished = OrchestratorEvent::MatchFinished {
            outcome: MatchOutcome {
                descriptor: descriptor(4),
                started_at: SystemTime::UNIX_EPOCH,
                finished_at: SystemTime::UNIX_EPOCH,
                result: match_result(),
                players: [identity(Player::Player1), identity(Player::Player2)],
            },
        };
        assert_eq!(finished.match_id(), MatchId(4));

        let failed = OrchestratorEvent::MatchFailed {
            failure: MatchFailure {
                descriptor: descriptor(5),
                started_at: None,
                failed_at: SystemTime::UNIX_EPOCH,
                reason: FailureReason::Cancelled,
                players: None,
                durable_record: true,
            },
        };
        assert_eq!(failed.match_id(), MatchId(5));
    }
}
