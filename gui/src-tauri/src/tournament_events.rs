//! Tauri events the tournament runner emits to the frontend.
//!
//! Two channels feed these, mirroring the session's own split:
//! - Lossless lifecycle + standings come from `SessionEvent` +
//!   `TournamentState` (the snapshot-before-event contract). Everything
//!   verdict-bearing (scores, W/L/D, Elo) flows this way.
//! - Lossy per-turn `NowPlaying` comes from `live_events()` — glance-only
//!   liveness, where a dropped frame just means the next one shows a later turn.

use serde::{Deserialize, Serialize};
use specta::Type;
use tauri_specta::Event;

/// One row of the live standings: Elo with a 95% CI band and game count.
/// CI bounds come from `compute_elo_with_uncertainty` (point Elo alone has
/// no covariance), so the frontend never reconstructs them.
#[derive(Serialize, Deserialize, Debug, Clone, Type)]
pub struct StandingRow {
    pub player_id: String,
    pub elo: f64,
    pub elo_ci_low: f64,
    pub elo_ci_high: f64,
    pub games: u32,
    /// True before the player has enough games for a stable estimate; the
    /// frontend shows "warming up" instead of a bar.
    pub pending: bool,
}

/// A participant, for the launch→live handoff (name + display fields).
#[derive(Serialize, Deserialize, Debug, Clone, Type)]
pub struct PlayerLite {
    pub player_id: String,
}

/// Emitted once, right after the tournament row is created. Scaffolds the
/// header, hero, and axis before any game finishes.
#[derive(Serialize, Deserialize, Debug, Clone, Type, Event)]
pub struct TournamentStartedEvent {
    pub tournament_id: i64,
    pub name: Option<String>,
    /// "gauntlet" or "round_robin".
    pub format: String,
    /// In a gauntlet, the measured bot (highlighted, not clickable). `None`
    /// for round-robin.
    pub target: Option<String>,
    pub total_games: u32,
    pub anchor_id: String,
    /// Shared-core provenance string, identical in the launch line and the
    /// live header: e.g. "my-bot vs 6 (gauntlet) · tiny preset · 200 ms/move".
    pub plan_summary: String,
    pub players: Vec<PlayerLite>,
}

/// Emitted on every `MatchFinished`, carrying the freshly recomputed standings
/// plus progress. The hero, standings bars/whiskers, progress, ETA, and chip
/// all read this.
#[derive(Serialize, Deserialize, Debug, Clone, Type, Event)]
pub struct StandingsUpdatedEvent {
    pub tournament_id: i64,
    pub done: u32,
    pub total: u32,
    pub success: u32,
    pub failure: u32,
    pub standings: Vec<StandingRow>,
}

/// Emitted on every `MatchFinished`. Scores are canonical (player1_id is the
/// lex-min of the pair); the frontend re-orients per target / per displayed
/// bot. Drives form dots, the game-card grid, and the W-L-D record. `turns`
/// and the board are loaded lazily via `get_game_replay` when a card renders.
#[derive(Serialize, Deserialize, Debug, Clone, Type, Event)]
pub struct TournamentMatchFinishedEvent {
    pub tournament_id: i64,
    pub player1_id: String,
    pub player2_id: String,
    pub repetition_index: u32,
    pub player1_score: f64,
    pub player2_score: f64,
    pub match_id: u64,
}

/// Emitted on every `MatchStarted` (slice B). Lets the matchup view show a
/// live-game row before the first turn arrives.
#[derive(Serialize, Deserialize, Debug, Clone, Type, Event)]
pub struct TournamentMatchStartedEvent {
    pub tournament_id: i64,
    pub match_id: u64,
    pub player1_id: String,
    pub player2_id: String,
    pub repetition_index: u32,
}

/// Throttled per-turn liveness from `live_events()` (slice B). Drives the
/// now-playing line and the depth-2 live-game row. Lossy by design.
#[derive(Serialize, Deserialize, Debug, Clone, Type, Event)]
pub struct NowPlayingEvent {
    pub tournament_id: i64,
    pub match_id: u64,
    pub turn: u16,
    pub player1_score: f32,
    pub player2_score: f32,
}

/// Terminal: tournament finished naturally. The hero swaps to the verdict and
/// the chip goes green.
#[derive(Serialize, Deserialize, Debug, Clone, Type, Event)]
pub struct TournamentFinishedEvent {
    pub tournament_id: i64,
}

/// Terminal: tournament aborted (e.g. persistent sink-flush failure, or
/// user-requested stop). Carries a reason for the UI.
#[derive(Serialize, Deserialize, Debug, Clone, Type, Event)]
pub struct TournamentAbortedEvent {
    pub tournament_id: i64,
    pub reason: String,
}
