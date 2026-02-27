use serde::{Deserialize, Serialize};
use specta::Type;
use tauri_specta::Event;

use crate::commands::{Coord, MazeState, PlayerState};

/// Full initial state so the frontend can initialize the renderer.
#[derive(Serialize, Deserialize, Debug, Clone, Type, Event)]
pub struct MatchStartedEvent {
    pub match_id: u32,
    pub maze: MazeState,
}

/// Per-turn delta. Walls/mud never change, so we only send positions + cheese.
#[derive(Serialize, Deserialize, Debug, Clone, Type, Event)]
pub struct TurnPlayedEvent {
    pub match_id: u32,
    pub turn: u16,
    pub player1: PlayerState,
    pub player2: PlayerState,
    pub cheese: Vec<Coord>,
}

/// Emitted when the match ends normally.
#[derive(Serialize, Deserialize, Debug, Clone, Type, Event)]
pub struct MatchOverEvent {
    pub match_id: u32,
    pub winner: MatchWinner,
    pub player1_score: f32,
    pub player2_score: f32,
    pub turns_played: u16,
}

/// Emitted on setup failures, bot crashes, etc.
#[derive(Serialize, Deserialize, Debug, Clone, Type, Event)]
pub struct MatchErrorEvent {
    pub match_id: u32,
    pub message: String,
}

/// Emitted when a bot disconnects mid-game.
#[derive(Serialize, Deserialize, Debug, Clone, Type, Event)]
pub struct BotDisconnectedEvent {
    pub match_id: u32,
    pub player: String,
    pub reason: String,
}

#[derive(Serialize, Deserialize, Debug, Clone, Type)]
pub enum MatchWinner {
    Player1,
    Player2,
    Draw,
}
