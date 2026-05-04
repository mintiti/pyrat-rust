//! Shared fixtures for the orchestrator integration tests.
//!
//! Embedded test bots, deterministic small-game configs, matchup builders.
//! Subprocess coverage in this crate is intentionally minimal — *one*
//! test (`setup_failure_durable.rs`) confirms the launch setup-failure
//! path; TCP accept/handshake and mid-match disconnect coverage stay in
//! the host crate.

#![allow(dead_code)]

use std::path::PathBuf;
use std::sync::Arc;
use std::time::SystemTime;

use async_trait::async_trait;
use parking_lot::Mutex;
use pyrat::game::builder::GameConfig;
use pyrat::{Coordinates, Direction, GameBuilder};
use pyrat_bot_api::Options;
use pyrat_host::match_host::MatchEvent;
use pyrat_host::player::{EmbeddedBot, EmbeddedCtx, PlayerIdentity};
use pyrat_host::wire::GameResult;
use pyrat_orchestrator::{
    AdHocDescriptor, EmbeddedBotFactory, MatchFailure, MatchId, MatchOutcome, MatchSink, Matchup,
    PlayerSpec, SinkError, Timing,
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

/// Factory whose closure panics on invocation. The orchestrator calls the
/// factory inside the per-match task body (`run_match.rs`'s
/// `BoxedEmbeddedBot(factory())` site), so the panic propagates up through
/// the per-match task and reaches `JoinSet::join_next_with_id` as
/// `JoinError::is_panic() == true`. A panic inside `EmbeddedBot::think`
/// would be caught by `EmbeddedPlayer`'s `spawn_blocking` and converted to
/// `PlayerError`, which is *not* what we want to test for the
/// `JoinError::is_panic()` mapping.
pub fn panicking_factory() -> EmbeddedBotFactory {
    Arc::new(|| panic!("test factory panic"))
}

/// Subprocess matchup whose subprocess slot points at a `working_dir`
/// that does not exist. `launch_bots` calls
/// `Command::current_dir(...).spawn()`, which fails at spawn time when
/// the directory is missing — this is the deterministic trigger for the
/// `SetupError::Launch -> FailureReason::SpawnFailed` path. Slot 1 is an
/// embedded MockBot so we don't depend on a second subprocess fixture.
pub fn subprocess_matchup_with_bad_workdir(id: u64) -> Matchup<AdHocDescriptor> {
    Matchup {
        descriptor: descriptor(id),
        game_config: small_game_config(),
        players: [
            PlayerSpec::Subprocess {
                agent_id: "test/bad-spawn".into(),
                command: "echo unreachable".into(),
                working_dir: Some(PathBuf::from("/this/path/does/not/exist/orchestrator-test")),
            },
            PlayerSpec::Embedded {
                agent_id: "test/p2".into(),
                name: "Player2Bot".into(),
                author: "tests".into(),
                factory: mock_factory(),
            },
        ],
        timing: Timing::default(),
    }
}

/// Records every sink callback for inspection in tests. `Required` or
/// `Optional` is decided at composition time by the test using
/// `CompositeSink`; this sink itself returns `Ok` from every method.
#[derive(Default)]
pub struct RecordingSink {
    pub calls: Mutex<Vec<RecordedCall>>,
}

#[derive(Debug, Clone)]
pub enum RecordedCall {
    Started {
        match_id: MatchId,
    },
    Event {
        match_id: MatchId,
        kind: &'static str,
    },
    Finished {
        match_id: MatchId,
    },
    Failed {
        match_id: MatchId,
        durable_record: bool,
        reason_debug: String,
    },
    Abandoned {
        match_id: MatchId,
    },
}

impl RecordingSink {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn snapshot(&self) -> Vec<RecordedCall> {
        self.calls.lock().clone()
    }

    pub fn failures(&self) -> Vec<RecordedCall> {
        self.calls
            .lock()
            .iter()
            .filter(|c| matches!(c, RecordedCall::Failed { .. }))
            .cloned()
            .collect()
    }
}

#[async_trait]
impl MatchSink<AdHocDescriptor> for RecordingSink {
    async fn on_match_started(
        &self,
        descriptor: &AdHocDescriptor,
        _players: &[PlayerIdentity; 2],
    ) -> Result<(), SinkError> {
        self.calls.lock().push(RecordedCall::Started {
            match_id: descriptor.match_id,
        });
        Ok(())
    }

    async fn on_match_event(&self, match_id: MatchId, event: &MatchEvent) -> Result<(), SinkError> {
        self.calls.lock().push(RecordedCall::Event {
            match_id,
            kind: match event {
                MatchEvent::BotIdentified { .. } => "BotIdentified",
                MatchEvent::PreprocessingStarted => "PreprocessingStarted",
                MatchEvent::SetupComplete => "SetupComplete",
                MatchEvent::MatchStarted { .. } => "MatchStarted",
                MatchEvent::TurnPlayed { .. } => "TurnPlayed",
                MatchEvent::BotInfo { .. } => "BotInfo",
                MatchEvent::BotProvisional { .. } => "BotProvisional",
                MatchEvent::BotTimeout { .. } => "BotTimeout",
                MatchEvent::MatchOver { .. } => "MatchOver",
                _ => "Other",
            },
        });
        Ok(())
    }

    async fn on_match_finished(
        &self,
        outcome: &MatchOutcome<AdHocDescriptor>,
    ) -> Result<(), SinkError> {
        self.calls.lock().push(RecordedCall::Finished {
            match_id: outcome.descriptor.match_id,
        });
        Ok(())
    }

    async fn on_match_failed(
        &self,
        failure: &MatchFailure<AdHocDescriptor>,
    ) -> Result<(), SinkError> {
        self.calls.lock().push(RecordedCall::Failed {
            match_id: failure.descriptor.match_id,
            durable_record: failure.durable_record,
            reason_debug: format!("{:?}", failure.reason),
        });
        Ok(())
    }

    async fn on_match_abandoned(&self, match_id: MatchId) -> Result<(), SinkError> {
        self.calls.lock().push(RecordedCall::Abandoned { match_id });
        Ok(())
    }
}

pub fn _ensure_used(_: GameResult) {}
