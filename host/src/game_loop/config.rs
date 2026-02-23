use std::collections::HashMap;
use std::time::Duration;

use crate::session::messages::OwnedMatchConfig;
use crate::wire::Player;

/// Which player a bot controls, identified by agent_id.
#[derive(Debug, Clone)]
pub struct PlayerEntry {
    pub player: Player,
    pub agent_id: String,
}

/// Host-only timing for the setup phase (not sent on wire).
#[derive(Debug, Clone)]
pub struct SetupTiming {
    /// Time allowed for all bots to connect and identify.
    pub startup_timeout: Duration,
    /// Time allowed for preprocessing after StartPreprocessing is sent.
    pub preprocessing_timeout: Duration,
}

impl Default for SetupTiming {
    fn default() -> Self {
        Self {
            startup_timeout: Duration::from_secs(30),
            preprocessing_timeout: Duration::from_secs(10),
        }
    }
}

/// What the caller provides to run a match setup.
#[derive(Debug, Clone)]
pub struct MatchSetup {
    /// Two entries: one per player. Same agent_id = hivemind.
    pub players: Vec<PlayerEntry>,
    /// Game config sent to bots. `controlled_players` left empty;
    /// the setup phase fills it per session.
    pub match_config: OwnedMatchConfig,
    /// Options to set per bot, keyed by agent_id.
    pub bot_options: HashMap<String, Vec<(String, String)>>,
    /// Setup phase timeouts.
    pub timing: SetupTiming,
}
