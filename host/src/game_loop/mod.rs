mod config;
mod launch;
mod playing;
mod setup;
mod slots;

pub use config::{BotConfig, MatchSetup, PlayerEntry, PlayingConfig, SessionHandle, SetupTiming};
pub use launch::{launch_bots, BotProcesses, LaunchError};
pub use playing::{run_playing, MatchResult, PlayingError};
pub use setup::{accept_connections, run_setup, SetupError, SetupResult};
