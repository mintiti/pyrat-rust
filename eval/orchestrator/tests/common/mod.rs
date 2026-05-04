//! Shared fixtures for the orchestrator integration tests.
//!
//! Embedded test bots, deterministic small-game configs, matchup builders.
//! Subprocess-path coverage stays in the host crate's integration tests
//! and the headless e2e. Running real subprocesses from this crate would
//! double the integration runtime for no incremental signal.

#![allow(dead_code)]

use std::sync::Arc;
use std::time::SystemTime;

use pyrat::game::builder::GameConfig;
use pyrat::{Coordinates, Direction, GameBuilder};
use pyrat_bot_api::Options;
use pyrat_host::player::{EmbeddedBot, EmbeddedCtx};
use pyrat_host::wire::GameResult;
use pyrat_orchestrator::{
    AdHocDescriptor, EmbeddedBotFactory, MatchId, Matchup, PlayerSpec, Timing,
};
use pyrat_protocol::HashedTurnState;

/// Always returns Stay. Plays a deterministic do-nothing match.
pub struct MockBot;
impl Options for MockBot {}
impl EmbeddedBot for MockBot {
    fn think(&mut self, _state: &HashedTurnState, _ctx: &EmbeddedCtx) -> Direction {
        Direction::Stay
    }
}

/// Spins inside `think` until the host signals stop. Triggers the
/// cancellation drop path: when `Match::run` is dropped, players drop,
/// dispatcher detects host_tx closure and flips `should_stop`, the bot
/// returns Stay, dispatcher exits cleanly, spawn_blocking thread reaped.
pub struct SlowBot;
impl Options for SlowBot {}
impl EmbeddedBot for SlowBot {
    fn think(&mut self, _state: &HashedTurnState, ctx: &EmbeddedCtx) -> Direction {
        while !ctx.should_stop() {
            std::hint::spin_loop();
        }
        Direction::Stay
    }
}

pub fn mock_factory() -> EmbeddedBotFactory {
    Arc::new(|| Box::new(MockBot))
}

pub fn slow_factory() -> EmbeddedBotFactory {
    Arc::new(|| Box::new(SlowBot))
}

/// 3x3 open maze, corner starts, single-cheese centre, 5-turn cap. Two
/// MockBots staying put end in Draw with `turns_played == 5`.
pub fn small_game_config() -> GameConfig {
    GameBuilder::new(3, 3)
        .with_max_turns(5)
        .with_open_maze()
        .with_custom_positions(Coordinates::new(0, 0), Coordinates::new(2, 2))
        .with_custom_cheese(vec![Coordinates::new(1, 1)])
        .build()
}

pub fn descriptor(id: u64) -> AdHocDescriptor {
    AdHocDescriptor {
        match_id: MatchId(id),
        seed: 42,
        planned_at: SystemTime::UNIX_EPOCH,
    }
}

/// Two-embedded-bot matchup with given factories and identifying labels.
pub fn embedded_matchup(
    id: u64,
    p1: EmbeddedBotFactory,
    p2: EmbeddedBotFactory,
) -> Matchup<AdHocDescriptor> {
    Matchup {
        descriptor: descriptor(id),
        game_config: small_game_config(),
        players: [
            PlayerSpec::Embedded {
                agent_id: "test/p1".into(),
                name: "Player1Bot".into(),
                author: "tests".into(),
                factory: p1,
            },
            PlayerSpec::Embedded {
                agent_id: "test/p2".into(),
                name: "Player2Bot".into(),
                author: "tests".into(),
                factory: p2,
            },
        ],
        timing: Timing::default(),
    }
}

pub fn mock_vs_mock(id: u64) -> Matchup<AdHocDescriptor> {
    embedded_matchup(id, mock_factory(), mock_factory())
}

pub fn _ensure_used(_: GameResult) {}
