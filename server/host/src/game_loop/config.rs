use std::collections::HashMap;
use std::time::Duration;

use tokio::sync::mpsc;

use crate::session::messages::HostCommand;
use crate::session::SessionId;
use pyrat_protocol::MatchConfig;
use pyrat_wire::Player;

/// Which player a bot controls, identified by agent_id.
#[derive(Debug, Clone)]
pub struct PlayerEntry {
    pub player: Player,
    pub agent_id: String,
}

/// Host-only timing for the setup phase (not sent on wire).
#[derive(Debug, Clone)]
pub struct SetupTiming {
    /// Time allowed for all bots to connect, identify, and report ready.
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

/// Handle to an active session after setup completes.
#[derive(Debug)]
pub struct SessionHandle {
    pub session_id: SessionId,
    pub cmd_tx: mpsc::Sender<HostCommand>,
    pub name: String,
    pub author: String,
    pub agent_id: String,
    pub controlled_players: Vec<Player>,
}

/// Configuration for the playing phase.
#[derive(Debug, Clone)]
pub struct PlayingConfig {
    /// Per-turn timeout for receiving actions from bots.
    ///
    /// `Duration::ZERO` means infinite timeout — no deadline, wait for actions
    /// or disconnects. Use this with [`HostCommand::Stop`] for GUI-driven
    /// turn-by-turn control.
    pub move_timeout: Duration,
    /// How much a bot's self-reported think_ms may exceed move_timeout
    /// before the action is rejected. 0.10 = 10% grace.
    pub think_margin: f32,
    /// Fixed network delivery buffer added on top of the think deadline.
    /// The host waits this long past the think deadline for packets to arrive.
    pub network_grace: Duration,
}

impl Default for PlayingConfig {
    fn default() -> Self {
        Self {
            move_timeout: Duration::from_secs(3),
            think_margin: 0.10,
            network_grace: Duration::from_millis(50),
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
    pub match_config: MatchConfig,
    /// Options to set per bot, keyed by agent_id.
    pub bot_options: HashMap<String, Vec<(String, String)>>,
    /// Setup phase timeouts.
    pub timing: SetupTiming,
}
