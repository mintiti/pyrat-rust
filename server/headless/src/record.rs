//! JSON game record builder. Consumes a `MatchEvent` stream and the final
//! `MatchResult`; produces a serializable `GameRecord` for `--output`.

use serde::Serialize;
use tracing::warn;

use pyrat_host::match_host::{MatchEvent, MatchResult};
use pyrat_host::wire::{GameResult, Player};

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

pub fn build(seed: Option<u64>, events: Vec<MatchEvent>, match_result: &MatchResult) -> GameRecord {
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
                let player_name = if player == Player::Player1 {
                    "Player1"
                } else {
                    "Player2"
                };
                players.push(PlayerRecord {
                    player: player_name.to_string(),
                    name,
                    author,
                    agent_id,
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
                    p1_action: p1_action as u8,
                    p2_action: p2_action as u8,
                    p1_position: (state.player1_position.x, state.player1_position.y),
                    p2_position: (state.player2_position.x, state.player2_position.y),
                    p1_score: state.player1_score,
                    p2_score: state.player2_score,
                    cheese_remaining: state.cheese.len(),
                    p1_think_ms,
                    p2_think_ms,
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
