//! Bot and Hivemind traits, Context for timing.

use std::time::Instant;

use pyrat::Direction;
use pyrat_wire::{GameResult, Player};

use crate::options::Options;
use crate::state::GameState;

/// Timing context passed to `think()` and `preprocess()`.
pub struct Context {
    deadline: Instant,
}

impl Context {
    /// Create a context with a deadline.
    pub(crate) fn new(deadline: Instant) -> Self {
        Self { deadline }
    }

    /// Whether the deadline has passed.
    pub fn should_stop(&self) -> bool {
        Instant::now() >= self.deadline
    }

    /// Milliseconds remaining before the deadline. Returns 0 if past.
    pub fn time_remaining_ms(&self) -> u64 {
        self.deadline
            .checked_duration_since(Instant::now())
            .map_or(0, |d| d.as_millis() as u64)
    }
}

/// Trait for a single-player bot.
///
/// Implement `think()` to return a direction each turn.
/// `preprocess()` and `on_game_over()` are optional.
pub trait Bot: Options {
    /// Choose a direction for this turn.
    fn think(&mut self, state: &GameState, ctx: &Context) -> Direction;

    /// Called once before the first turn, with a longer timeout.
    fn preprocess(&mut self, _state: &GameState, _ctx: &Context) {}

    /// Called when the game ends.
    fn on_game_over(&mut self, _result: GameResult, _scores: (f32, f32)) {}
}

/// Trait for a hivemind bot controlling both players.
///
/// Same lifecycle as `Bot`, but returns two actions per turn.
pub trait Hivemind: Options {
    /// Choose directions for both players.
    fn think(&mut self, state: &GameState, ctx: &Context) -> [(Player, Direction); 2];

    /// Called once before the first turn.
    fn preprocess(&mut self, _state: &GameState, _ctx: &Context) {}

    /// Called when the game ends.
    fn on_game_over(&mut self, _result: GameResult, _scores: (f32, f32)) {}
}
