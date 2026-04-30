mod config;
mod events;
mod playing;
mod setup;
mod slots;

pub use crate::launch::{launch_bots, BotConfig, BotExitInfo, BotProcesses, LaunchError};
pub use crate::match_config::build_match_config;
pub use crate::probe::{probe_bot, ProbeError, ProbeResult};
pub use crate::session::messages::DisconnectReason;
pub use config::{MatchSetup, PlayerEntry, PlayingConfig, SessionHandle, SetupTiming};
pub use events::MatchEvent;
pub use playing::{
    determine_result, run_one_turn, run_playing, MatchResult, PlayingError, PlayingState,
    TurnOutcome,
};
pub use pyrat_protocol::{HashedTurnState, Info, MatchConfig, OptionDef, TurnState};
pub use setup::{accept_connections, run_setup, SetupError, SetupResult};
