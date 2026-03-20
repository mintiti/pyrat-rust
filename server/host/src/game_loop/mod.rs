mod config;
mod events;
mod launch;
mod playing;
mod probe;
mod setup;
mod slots;

pub use crate::session::messages::{
    DisconnectReason, OwnedInfo, OwnedMatchConfig, OwnedOptionDef, OwnedTurnState,
};
pub use config::{
    build_owned_match_config, BotConfig, MatchSetup, PlayerEntry, PlayingConfig, SessionHandle,
    SetupTiming,
};
pub use events::MatchEvent;
pub use launch::{launch_bots, BotProcesses, LaunchError};
pub use playing::{
    determine_result, engine_to_wire, run_one_turn, run_playing, wire_to_engine, MatchResult,
    PlayingError, PlayingState, TurnOutcome,
};
pub use probe::{probe_bot, ProbeError, ProbeResult};
pub use setup::{accept_connections, run_setup, SetupError, SetupResult};
