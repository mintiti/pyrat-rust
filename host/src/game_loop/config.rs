use std::collections::HashMap;
use std::time::Duration;

use pyrat::game::game_logic::GameState;
use tokio::sync::mpsc;

use crate::session::messages::{HostCommand, OwnedMatchConfig};
use crate::session::SessionId;
use pyrat_wire::{Player, TimingMode};

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
}

impl Default for PlayingConfig {
    fn default() -> Self {
        Self {
            move_timeout: Duration::from_secs(3),
        }
    }
}

/// How to launch a bot subprocess. Separate from [`MatchSetup`] because
/// launching is optional — tests and GUIs that manage bots externally
/// never provide this.
#[derive(Debug, Clone)]
pub struct BotConfig {
    /// Shell command to spawn the bot. Empty string = manual start (skipped).
    pub run_command: String,
    /// Working directory for the spawned process.
    pub working_dir: std::path::PathBuf,
    /// Agent identifier the bot uses to identify itself on connect.
    pub agent_id: String,
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

/// Build an `OwnedMatchConfig` from engine state + timing parameters.
///
/// `controlled_players` is left empty — the setup phase fills it per session.
pub fn build_owned_match_config(
    game: &GameState,
    timing: TimingMode,
    move_timeout_ms: u32,
    preprocessing_timeout_ms: u32,
) -> OwnedMatchConfig {
    let walls = game
        .wall_entries()
        .into_iter()
        .map(|w| ((w.pos1.x, w.pos1.y), (w.pos2.x, w.pos2.y)))
        .collect();

    let mud = game
        .mud_positions()
        .iter()
        .map(|((from, to), value)| {
            let (p1, p2) = if from < to { (from, to) } else { (to, from) };
            ((p1.x, p1.y), (p2.x, p2.y), value)
        })
        .collect();

    let cheese = game
        .cheese_positions()
        .into_iter()
        .map(|c| (c.x, c.y))
        .collect();

    let p1 = game.player1_position();
    let p2 = game.player2_position();

    OwnedMatchConfig {
        width: game.width(),
        height: game.height(),
        max_turns: game.max_turns(),
        walls,
        mud,
        cheese,
        player1_start: (p1.x, p1.y),
        player2_start: (p2.x, p2.y),
        controlled_players: vec![],
        timing,
        move_timeout_ms,
        preprocessing_timeout_ms,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pyrat::game::builder::GameConfig;
    use pyrat::{Coordinates, GameBuilder};

    #[test]
    fn build_owned_match_config_round_trips_game_state() {
        let game = GameBuilder::new(3, 3)
            .with_open_maze()
            .with_custom_positions(Coordinates::new(0, 0), Coordinates::new(2, 2))
            .with_custom_cheese(vec![Coordinates::new(1, 1)])
            .build()
            .create(Some(42))
            .unwrap();

        let cfg = build_owned_match_config(&game, TimingMode::Wait, 500, 3000);

        assert_eq!(cfg.width, 3);
        assert_eq!(cfg.height, 3);
        assert_eq!(cfg.max_turns, game.max_turns());
        assert_eq!(cfg.player1_start, (0, 0));
        assert_eq!(cfg.player2_start, (2, 2));
        assert_eq!(cfg.cheese, vec![(1, 1)]);
        assert!(cfg.walls.is_empty(), "open maze should have no walls");
        assert!(
            cfg.controlled_players.is_empty(),
            "controlled_players left for setup"
        );
        assert_eq!(cfg.timing, TimingMode::Wait);
        assert_eq!(cfg.move_timeout_ms, 500);
        assert_eq!(cfg.preprocessing_timeout_ms, 3000);
    }

    #[test]
    fn build_owned_match_config_extracts_walls_and_mud() {
        let game = GameConfig::classic(7, 5, 3).create(Some(42)).unwrap();

        let cfg = build_owned_match_config(&game, TimingMode::Wait, 500, 3000);

        assert_eq!(cfg.width, 7);
        assert_eq!(cfg.height, 5);
        assert!(!cfg.walls.is_empty(), "classic 7×5 maze should have walls");

        // Mud entries should be normalized: pos1 <= pos2.
        for &(p1, p2, value) in &cfg.mud {
            assert!(p1 <= p2, "mud entry not normalized: {p1:?} > {p2:?}");
            assert!(value >= 2, "mud value should be >= 2, got {value}");
        }
    }
}
