mod config;
mod events;
mod launch;
mod playing;
mod setup;
mod slots;

pub use crate::session::messages::{DisconnectReason, OwnedInfo, OwnedMatchConfig, OwnedTurnState};
pub use config::{
    build_owned_match_config, BotConfig, MatchSetup, PlayerEntry, PlayingConfig, SessionHandle,
    SetupTiming,
};
pub use events::MatchEvent;
pub use launch::{launch_bots, BotProcesses, LaunchError};
pub use playing::{
    run_one_turn, run_playing, MatchResult, PlayingError, PlayingState, TurnOutcome,
};
pub use setup::{accept_connections, run_setup, SetupError, SetupResult};
