mod config;
mod events;
mod launch;
mod playing;
mod probe;
mod setup;
mod slots;

pub use crate::session::messages::DisconnectReason;
pub use config::{
    build_owned_match_config, BotConfig, MatchSetup, PlayerEntry, PlayingConfig, SessionHandle,
    SetupTiming,
};
pub use events::MatchEvent;
pub use launch::{launch_bots, BotExitInfo, BotProcesses, LaunchError};
pub use playing::{
    determine_result, run_one_turn, run_playing, MatchResult, PlayingError, PlayingState,
    TurnOutcome,
};
pub use probe::{probe_bot, ProbeError, ProbeResult};
pub use pyrat_protocol::{
    HashedTurnState, OwnedInfo, OwnedMatchConfig, OwnedOptionDef, OwnedTurnState,
};
pub use setup::{accept_connections, run_setup, SetupError, SetupResult};
