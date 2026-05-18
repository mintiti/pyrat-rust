//! `GameRecord` JSON shape consumed by the GUI replay loader and downstream
//! scripts. Tournament workflows use the richer `ReplayEvent` DTO from
//! `pyrat-orchestrator` instead.

use std::path::PathBuf;

use async_trait::async_trait;
use parking_lot::Mutex;
use serde::Serialize;
use tracing::warn;

use pyrat_host::match_host::{MatchEvent, MatchResult};
use pyrat_host::player::PlayerIdentity;
use pyrat_host::wire::{GameResult, Player};

use pyrat_orchestrator::{
    AdHocDescriptor, Descriptor, MatchFailure, MatchId, MatchOutcome, MatchSink, SinkError,
};

#[derive(Serialize)]
pub struct GameRecord {
    width: u8,
    height: u8,
    max_turns: u16,
    seed: Option<u64>,
    players: Vec<PlayerRecord>,
    turns: Vec<TurnRecord>,
    result: ResultRecord,
}

#[derive(Serialize)]
struct PlayerRecord {
    player: String,
    name: String,
    author: String,
    agent_id: String,
}

#[derive(Serialize)]
struct TurnRecord {
    turn: u16,
    p1_action: u8,
    p2_action: u8,
    p1_position: (u8, u8),
    p2_position: (u8, u8),
    p1_score: f32,
    p2_score: f32,
    cheese_remaining: usize,
    p1_think_ms: u32,
    p2_think_ms: u32,
}

#[derive(Serialize)]
struct ResultRecord {
    winner: String,
    player1_score: f32,
    player2_score: f32,
    turns_played: u16,
}

fn result_label(result: GameResult) -> &'static str {
    match result {
        GameResult::Player1 => "Player1",
        GameResult::Player2 => "Player2",
        GameResult::Draw => "Draw",
        unknown => {
            warn!(?unknown, "unexpected GameResult variant");
            "Draw"
        },
    }
}

pub fn build(seed: Option<u64>, events: &[MatchEvent], match_result: &MatchResult) -> GameRecord {
    let mut players = Vec::new();
    let mut turns = Vec::new();
    let mut width: u8 = 0;
    let mut height: u8 = 0;
    let mut max_turns: u16 = 0;

    for event in events {
        match event {
            MatchEvent::MatchStarted { config } => {
                width = config.width;
                height = config.height;
                max_turns = config.max_turns;
            },
            MatchEvent::BotIdentified {
                player,
                name,
                author,
                agent_id,
            } => {
                let player_name = if *player == Player::Player1 {
                    "Player1"
                } else {
                    "Player2"
                };
                players.push(PlayerRecord {
                    player: player_name.to_string(),
                    name: name.clone(),
                    author: author.clone(),
                    agent_id: agent_id.clone(),
                });
            },
            MatchEvent::TurnPlayed {
                state,
                p1_action,
                p2_action,
                p1_think_ms,
                p2_think_ms,
            } => {
                turns.push(TurnRecord {
                    turn: state.turn,
                    p1_action: *p1_action as u8,
                    p2_action: *p2_action as u8,
                    p1_position: (state.player1_position.x, state.player1_position.y),
                    p2_position: (state.player2_position.x, state.player2_position.y),
                    p1_score: state.player1_score,
                    p2_score: state.player2_score,
                    cheese_remaining: state.cheese.len(),
                    p1_think_ms: *p1_think_ms,
                    p2_think_ms: *p2_think_ms,
                });
            },
            _ => {},
        }
    }

    GameRecord {
        width,
        height,
        max_turns,
        seed,
        players,
        turns,
        result: ResultRecord {
            winner: result_label(match_result.result).to_string(),
            player1_score: match_result.player1_score,
            player2_score: match_result.player2_score,
            turns_played: match_result.turns_played,
        },
    }
}

/// Buffers `MatchEvent`s for one match in flight and writes the JSON record
/// on success. A flush failure surfaces as `MatchFailed { SinkFlushError }`.
/// On failure or abandonment the buffer is dropped without writing, so a
/// half-played match never produces a partial file.
pub struct LegacyRecordSink {
    path: PathBuf,
    buffer: Mutex<Option<Buffer>>,
}

struct Buffer {
    seed: u64,
    events: Vec<MatchEvent>,
}

impl LegacyRecordSink {
    pub fn new(path: PathBuf) -> Self {
        Self {
            path,
            buffer: Mutex::new(None),
        }
    }
}

#[async_trait]
impl MatchSink<AdHocDescriptor> for LegacyRecordSink {
    async fn on_match_started(
        &self,
        descriptor: &AdHocDescriptor,
        _players: &[PlayerIdentity; 2],
    ) -> Result<(), SinkError> {
        *self.buffer.lock() = Some(Buffer {
            seed: descriptor.seed(),
            events: Vec::new(),
        });
        Ok(())
    }

    async fn on_match_event(&self, _id: MatchId, event: &MatchEvent) -> Result<(), SinkError> {
        if let Some(buf) = self.buffer.lock().as_mut() {
            buf.events.push(event.clone());
        }
        Ok(())
    }

    async fn on_match_finished(
        &self,
        outcome: &MatchOutcome<AdHocDescriptor>,
    ) -> Result<(), SinkError> {
        let buf = match self.buffer.lock().take() {
            Some(b) => b,
            None => {
                return Err(SinkError {
                    source: anyhow::anyhow!(
                        "on_match_finished called without prior on_match_started"
                    ),
                });
            },
        };
        let record = build(Some(buf.seed), &buf.events, &outcome.result);
        let json =
            serde_json::to_string_pretty(&record).map_err(|e| SinkError { source: e.into() })?;
        std::fs::write(&self.path, json).map_err(|e| SinkError { source: e.into() })?;
        Ok(())
    }

    async fn on_match_failed(
        &self,
        _failure: &MatchFailure<AdHocDescriptor>,
    ) -> Result<(), SinkError> {
        *self.buffer.lock() = None;
        Ok(())
    }

    async fn on_match_abandoned(&self, _id: MatchId) -> Result<(), SinkError> {
        *self.buffer.lock() = None;
        Ok(())
    }
}
