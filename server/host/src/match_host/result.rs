//! Final outcome of a match: scores, result, turns played.

use pyrat_wire::GameResult;

/// Result of a completed match.
#[derive(Debug, Clone, PartialEq)]
pub struct MatchResult {
    pub result: GameResult,
    pub player1_score: f32,
    pub player2_score: f32,
    pub turns_played: u16,
}

impl MatchResult {
    /// Derive a `MatchResult` from a finished engine state.
    pub fn from_game(game: &pyrat::GameState) -> Self {
        let p1 = game.player1.score;
        let p2 = game.player2.score;
        let result = if p1 > p2 {
            GameResult::Player1
        } else if p2 > p1 {
            GameResult::Player2
        } else {
            GameResult::Draw
        };
        Self {
            result,
            player1_score: p1,
            player2_score: p2,
            turns_played: game.turn,
        }
    }
}
