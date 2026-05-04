//! Concurrent match executor that runs `Vec<Matchup>` and emits a stream of
//! `OrchestratorEvent`s. Domain-free: knows about [`pyrat_host`] and sinks,
//! not SQLite, not Elo, not tournaments.

pub mod descriptor;
pub mod event;
pub mod id;
pub mod matchup;
pub mod outcome;
pub mod sink;
pub mod sinks;
pub mod state;

pub use descriptor::{AdHocDescriptor, Descriptor};
pub use event::OrchestratorEvent;
pub use id::{MatchId, MatchIdAllocator};
pub use matchup::{EmbeddedBotFactory, Matchup, PlayerSpec, Timing};
pub use outcome::{FailureReason, MatchFailure, MatchOutcome};
pub use sink::{MatchSink, NoOpSink, SinkError, SinkRole};
pub use sinks::composite::CompositeSink;
pub use state::ExecutorState;
