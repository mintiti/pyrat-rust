//! Public event stream emitted by the orchestrator.
//!
//! `OrchestratorEvent<D>` is a runtime type — *not* required to be
//! `Serialize`. It carries flatbuffers-generated host types (`MatchEvent`,
//! `MatchResult`, `PlayerIdentity`) that aren't serde-friendly. Persistence
//! flows through sink callbacks (sinks extract fields and write them);
//! replay flows through the `ReplayEvent` DTO added in PR 3.

use pyrat_host::match_host::MatchEvent;
use pyrat_host::player::PlayerIdentity;

use crate::descriptor::Descriptor;
use crate::id::MatchId;
use crate::outcome::{MatchFailure, MatchOutcome};

/// One event from the orchestrator's merged stream.
///
/// Ordering rules (enforced in PR 3 by `run_match.rs`):
/// - `MatchQueued` is the first event for a given id.
/// - `MatchStarted` is published once players are identified.
/// - `MatchEvent` carries every non-terminal host event tagged with id.
/// - `MatchFinished` / `MatchFailed` are the only terminal signals; the
///   host's `MatchEvent::MatchOver` is suppressed (the canonical terminal
///   value is the `MatchResult` carried inside `MatchFinished.outcome`).
#[derive(Debug, Clone)]
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
