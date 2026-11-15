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
pub mod eta;
pub mod legacy_record;
pub mod mapping;
pub mod observation;
pub mod plan;
pub mod session;
pub mod state;
pub mod store_sink;

pub use descriptor::EvalMatchDescriptor;
pub use eta::{
    compare_eta_models_walk_forward, replay_eta, replay_m0, replay_m1, replay_m1p,
    schedule_snapshot, ActiveScheduleAttempt, EtaEstimate, EtaModel, EtaModelComparisonPoint,
    EtaReplayPoint, EtaScoredEstimate, EtaState, EtaTraceError, TournamentScheduleSnapshot,
    TournamentTimingReader, TournamentTimingTrace, M0_MODEL_ID, M1P_MODEL_ID, M1_MODEL_ID,
    TIMING_TRACE_SCHEMA_VERSION,
};
pub use legacy_record::{GameRecord, LegacyRecordSink};
pub use mapping::{
    failure_reason_string, failure_report, failure_to_new_attempt, format_sqlite_datetime,
    game_config_to_record, outcome_to_new_attempt, MappingError,
};
pub use observation::Observation;
pub use plan::{
    gauntlet_slot_order, matchup_seed, split_gauntlet_players, DispatchPolicy,
    DispatchPolicyParseError, GauntletPlanner, GauntletPlannerConfig, PlannedSlot, Planner,
    PlannerConfigError, PlayerNotInList, ResolvedPlayer, RoundRobinPlanner,
    RoundRobinPlannerConfig, SeatPolicy, TournamentParams,
};
// Re-export store-native tournament records so consumers (CLI, GUI) name one
// type at the session boundary.
pub use pyrat_eval_store::{SeatOrientation, TournamentMethodology, TournamentTimingMode};
pub use session::{
    CreatedTournament, EvalSession, SessionCompletion, SessionConfig, SessionError, SessionEvent,
    SessionMode, TournamentMismatch, TournamentSpec,
};
pub use state::{
    ExecutionSummaryError, GameConfigId, MatchupAttempt, MatchupHistory, MatchupKey,
    MatchupOutcome, PlayerId, TournamentExecutionSummary, TournamentState,
};
pub use store_sink::{StoreSink, StoreSinkError};
