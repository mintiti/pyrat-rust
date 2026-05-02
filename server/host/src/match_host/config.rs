//! Host-side timing knobs and fault policy for setup and the playing loop.
//!
//! Wire-level fields (`move_timeout_ms`, `preprocessing_timeout_ms`) live on
//! the `MatchConfig` sent to bots. These structs hold the *host's* policy:
//! how much grace to allow on top of the wire deadlines, how long setup may
//! take, and how to resolve missing actions. Not serialized; not visible to
//! bots.

use std::sync::Arc;
use std::time::Duration;

use super::policy::{default_policy, FaultPolicy};

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
    /// Per-message timeout for the post-`Advance` sync acknowledgment
    /// (`SyncOk` or `Resync`). Decoupled from `move_timeout` because sync is
    /// network round-trip, not bot thinking time, and must stay bounded when
    /// `move_timeout` is infinite (`Duration::ZERO`). Worst case is 2× this
    /// value when a single `Resync → FullState → SyncOk` round-trip happens.
    pub sync_timeout: Duration,
    /// How to resolve per-slot action outcomes (committed / timed out /
    /// disconnected) into a final [`Direction`](pyrat::Direction). Defaults
    /// to [`DefaultFaultPolicy`](super::policy::DefaultFaultPolicy), which
    /// preserves the host's pre-seam behavior (provisional fallback on
    /// timeout, fatal on disconnect).
    pub fault_policy: Arc<dyn FaultPolicy>,
}

impl Default for PlayingConfig {
    fn default() -> Self {
        Self {
            move_timeout: Duration::from_secs(3),
            network_grace: Duration::from_millis(50),
            sync_timeout: Duration::from_secs(2),
            fault_policy: default_policy(),
        }
    }
}
