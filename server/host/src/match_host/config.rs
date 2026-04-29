//! Host-side timing knobs for setup and the playing loop.
//!
//! Wire-level fields (`move_timeout_ms`, `preprocessing_timeout_ms`) live on
//! the `MatchConfig` sent to bots. These structs hold the *host's* policy:
//! how much grace to allow on top of the wire deadlines, how long setup may
//! take, and so on. Not serialized; not visible to bots.

use std::time::Duration;

/// Host-only timing for the setup phase.
#[derive(Debug, Clone)]
pub struct SetupTiming {
    /// Time allowed for the Configure → Ready exchange with each bot.
    pub configure_timeout: Duration,
    /// Time allowed for preprocessing after `GoPreprocess` is sent.
    pub preprocessing_timeout: Duration,
}

impl Default for SetupTiming {
    fn default() -> Self {
        Self {
            configure_timeout: Duration::from_secs(5),
            preprocessing_timeout: Duration::from_secs(10),
        }
    }
}

/// Configuration for the playing phase.
#[derive(Debug, Clone)]
pub struct PlayingConfig {
    /// Per-turn timeout for receiving actions from bots.
    ///
    /// `Duration::ZERO` means infinite timeout — wait until both bots send
    /// a committed Action or the host issues `Stop`.
    pub move_timeout: Duration,
    /// Fixed network delivery buffer added on top of the think deadline.
    /// The host waits this long past the think deadline for packets to
    /// arrive before falling back to provisional / Stay.
    pub network_grace: Duration,
}

impl Default for PlayingConfig {
    fn default() -> Self {
        Self {
            move_timeout: Duration::from_secs(3),
            network_grace: Duration::from_millis(50),
        }
    }
}
