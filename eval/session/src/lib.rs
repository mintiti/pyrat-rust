//! Domain layer that wires planner + orchestrator + store.
//!
//! Three responsibilities:
//! - **Planner trait + impls** — round-robin, gauntlet. Decides which
//!   matchups to issue based on `TournamentState.history`. Never touches the
//!   store.
//! - **Store sink** — `MatchSink<EvalMatchDescriptor>` that writes
//!   `match_attempts` rows on terminal callbacks. Skips writes for
//!   `failure.durable_record == false` (kill-9 / sink flush failure) so
//!   runtime state stays byte-equivalent to the durable record.
//! - **Session** — drives the orchestrator-planner loop, owns the lossless
//!   `DriverEvent` mpsc, exposes `state()` (eval-layer watch over
//!   `TournamentState`) and `live_events()` (pass-through to the orchestrator
//!   broadcast for per-turn UI consumers).

// Re-export the orchestrator so consumers (GUI, alpharat) don't need a
// version-matched direct dependency for the types in our signatures
// (`OrchestratorConfig`, `MatchSink`, `SinkRole`, ...). Same pattern as
// `pyrat_host::wire`.
pub use pyrat_orchestrator as orchestrator;

pub mod descriptor;
pub mod legacy_record;
pub mod mapping;
pub mod observation;
pub mod plan;
pub mod session;
pub mod state;
pub mod store_sink;

pub use descriptor::EvalMatchDescriptor;
pub use legacy_record::{GameRecord, LegacyRecordSink};
pub use mapping::{
    failure_reason_string, failure_to_new_attempt, format_sqlite_datetime, game_config_to_record,
    outcome_to_new_attempt, MappingError,
};
pub use observation::Observation;
pub use plan::{
    matchup_seed, GauntletPlanner, GauntletPlannerConfig, Planner, ResolvedPlayer,
    RoundRobinPlanner, RoundRobinPlannerConfig, TournamentParams,
};
pub use session::{
    CreatedTournament, EvalSession, SessionConfig, SessionError, SessionEvent, SessionMode,
    TournamentSpec,
};
pub use state::{
    GameConfigId, MatchupAttempt, MatchupHistory, MatchupKey, MatchupOutcome, PlayerId,
    TournamentState,
};
pub use store_sink::{StoreSink, StoreSinkError};
