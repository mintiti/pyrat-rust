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

/// Emitted during the pre-tournament bot warmup, before the row exists (so it
/// carries no `tournament_id`). Drives a transient launch-side "Preparing
/// bots… (done/total)" state that clears when `TournamentStartedEvent` lands.
/// `done` increments before each bot's warmup and once more on completion.
#[derive(Serialize, Deserialize, Debug, Clone, Type, Event)]
pub struct TournamentPreparingEvent {
    pub done: u32,
    pub total: u32,
    /// The bot currently being warmed (agent_id), or `None` on the final
    /// completion emit. Drives "Preparing {bot}…" so the line reads as progress
    /// even while `done` sits at 0 during the first cold build.
    pub current: Option<String>,
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
    /// Games per matchup (= 2 × mazes under the paired schedule). Configurable,
    /// so the matchup view reads this instead of a hardcoded count.
    pub games_per_matchup: u32,
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

/// Which phase a timeout fired in, for labeling bot health
/// ("move timeout" vs "preprocessing").
#[derive(Serialize, Deserialize, Debug, Clone, Copy, Type)]
#[serde(rename_all = "snake_case")]
pub enum TimeoutPhase {
    Setup,
    Preprocessing,
    Sync,
    Move,
}

/// Category of a match failure, for the per-bot health summary. Mirrors
/// `pyrat_eval::orchestrator::FailureReason` collapsed to what the UI groups on
/// (payload strings dropped; the implicated bot rides `failing_player_id`).
#[derive(Serialize, Deserialize, Debug, Clone, Copy, Type)]
#[serde(rename_all = "snake_case")]
pub enum FailureKind {
    Timeout,
    Disconnected,
    SpawnFailed,
    HandshakeTimeout,
    ProtocolError,
    Cancelled,
    /// Tournament-infrastructure failure (panic, result-sink flush, internal),
    /// not the bot's fault — kept distinct from `Other` so the UI doesn't read
    /// an infra bug as "the bot failed".
    Internal,
    Other,
}

/// Terminal, per-match: a match failed (timeout, disconnect, spawn failure).
/// Verdict-bearing data (the success/failure tally) still rides the lossless
/// standings path; this also carries enough to (a) drop the now-playing row
/// and (b) accumulate per-bot health. Distinct from
/// `TournamentMatchFinishedEvent`, which means a *successful scored game* (form
/// dots, game cards, replay).
#[derive(Serialize, Deserialize, Debug, Clone, Type, Event)]
pub struct TournamentMatchFailedEvent {
    pub tournament_id: i64,
    pub match_id: u64,
    /// Canonical pair (player1_id = lex-min), so the frontend can attribute the
    /// failure to a matchup without a lookup.
    pub player1_id: String,
    pub player2_id: String,
    /// The bot the failure points at (timeout / clean disconnect), resolved
    /// from the engine seat via the match's seat orientation. `None` for
    /// structural failures (spawn, sink, internal) with no single seat.
    pub failing_player_id: Option<String>,
    pub kind: FailureKind,
    /// Set only when `kind == Timeout`.
    pub timeout_phase: Option<TimeoutPhase>,
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
