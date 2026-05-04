//! Concurrent match executor that runs `Vec<Matchup>` and emits a stream of
//! `OrchestratorEvent`s. Domain-free: knows about [`pyrat_host`] and sinks,
//! not SQLite, not Elo, not tournaments.

pub mod descriptor;
pub mod error;
pub mod event;
pub mod executor;
pub mod id;
pub mod matchup;
pub mod outcome;
pub mod replay_event;
mod run_match;
pub mod sink;
pub mod sinks;
pub mod state;

pub use descriptor::{AdHocDescriptor, Descriptor};
pub use error::{OrchestratorError, OrchestratorInternalError};
pub use event::{DriverEvent, OrchestratorEvent};
pub use executor::{Orchestrator, OrchestratorConfig};
pub use id::{MatchId, MatchIdAllocator};
pub use matchup::{EmbeddedBotFactory, Matchup, PlayerSpec, Timing};
pub use outcome::{FailureReason, MatchFailure, MatchOutcome};
pub use replay_event::{ReplayEvent, ReplayInfo, ReplayMatchConfig, ReplayMatchResult};
pub use sink::{MatchSink, NoOpSink, SinkError, SinkRole};
pub use sinks::composite::CompositeSink;
pub use sinks::replay::{DirectoryWriter, MemoryWriter, ReplayFile, ReplaySink, ReplayWriter};
pub use state::ExecutorState;
