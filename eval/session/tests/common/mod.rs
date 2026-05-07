//! Shared fixtures for session integration tests.
//!
//! Mirrors `eval/orchestrator/tests/common/mod.rs` but specialised to the
//! eval-session types (`EvalMatchDescriptor`, `ResolvedPlayer`).

#![allow(dead_code)]

use std::sync::Arc;

use parking_lot::Mutex;
use pyrat::game::builder::GameConfig;
use pyrat::{Coordinates, Direction, GameBuilder};
use pyrat_bot_api::Options;
use pyrat_eval::{
    GameConfigId, ResolvedPlayer, RoundRobinPlanner, RoundRobinPlannerConfig, TournamentSpec,
};
use pyrat_eval_store::{EvalStore, GameConfigRecord};
use pyrat_host::player::{EmbeddedBot, EmbeddedCtx};
use pyrat_host::wire::TimingMode;
use pyrat_orchestrator::{EmbeddedBotFactory, OrchestratorConfig, PlayerSpec, Timing};
use pyrat_protocol::HashedTurnState;

pub struct MockBot;
impl Options for MockBot {}
impl EmbeddedBot for MockBot {
    fn think(&mut self, _: &HashedTurnState, _: &EmbeddedCtx) -> Direction {
        Direction::Stay
    }
}

pub fn mock_factory() -> EmbeddedBotFactory {
    Arc::new(|| Box::new(MockBot))
}

/// Tiny deterministic config: 3x3 open maze, corner starts, single cheese,
/// 5-turn cap. Two MockBots draw with player1/player2 score = 0.5 / 0.5.
pub fn small_game_config() -> GameConfig {
    GameBuilder::new(3, 3)
        .with_max_turns(5)
        .with_open_maze()
        .with_custom_positions(Coordinates::new(0, 0), Coordinates::new(2, 2))
        .with_custom_cheese(vec![Coordinates::new(1, 1)])
        .build()
}

pub fn embedded_player(id: &str) -> ResolvedPlayer {
    ResolvedPlayer {
        id: id.into(),
        spec: PlayerSpec::Embedded {
            agent_id: id.into(),
            name: id.into(),
            author: "tests".into(),
            factory: mock_factory(),
        },
    }
}

pub fn fast_orch_config() -> OrchestratorConfig {
    OrchestratorConfig {
        max_parallel: 2,
        ..OrchestratorConfig::default()
    }
}

pub fn fast_timing() -> Timing {
    Timing {
        mode: TimingMode::Wait,
        move_timeout_ms: 1000,
        preprocessing_timeout_ms: 5000,
    }
}

pub fn open_store_with_config(store: &Arc<Mutex<EvalStore>>) -> GameConfigId {
    store
        .lock()
        .ensure_game_config(&GameConfigRecord {
            width: 3,
            height: 3,
            max_turns: 5,
            wall_density: 0.0,
            mud_density: 0.0,
            mud_range: 2,
            connected: true,
            symmetric: false,
            cheese_count: 1,
            cheese_symmetric: false,
        })
        .expect("ensure_game_config")
}

pub fn round_robin(
    players: Vec<ResolvedPlayer>,
    game_config: GameConfig,
    game_config_id: String,
    tournament_id: pyrat_eval_store::TournamentId,
    target_per_pair: u32,
) -> RoundRobinPlanner {
    RoundRobinPlanner::new(RoundRobinPlannerConfig {
        players,
        game_config,
        game_config_id,
        timing: fast_timing(),
        tournament_id,
        target_per_pair,
        max_failures_per_pair: 3,
        tournament_seed: 0xC0FFEE,
    })
}

pub fn round_robin_spec() -> TournamentSpec {
    TournamentSpec {
        format: "round_robin".into(),
        target_games_per_matchup: Some(1),
        params_json: "{}".into(),
    }
}
