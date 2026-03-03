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

// ── Private Runner trait for turn-loop unification ────

/// Unified lifecycle used by the generic `run_async` / `turn_loop`.
/// Not exported — Bot and Hivemind are the public API.
pub(crate) trait Runner: Options {
    type Actions: IntoIterator<Item = (Player, Direction)>;

    fn runner_preprocess(&mut self, state: &GameState, ctx: &Context);
    fn runner_think(&mut self, state: &GameState, ctx: &Context) -> Self::Actions;
    fn runner_stay(state: &GameState) -> Self::Actions;
    fn runner_on_game_over(&mut self, result: GameResult, scores: (f32, f32));
}

/// Newtype wrapper so Bot can implement Runner without blanket-impl conflicts.
pub(crate) struct BotRunner<'a, B: Bot>(pub &'a mut B);

impl<B: Bot> Options for BotRunner<'_, B> {
    fn option_defs(&self) -> Vec<crate::options::SdkOptionDef> {
        self.0.option_defs()
    }
    fn apply_option(&mut self, name: &str, value: &str) -> Result<(), String> {
        self.0.apply_option(name, value)
    }
}

impl<B: Bot> Runner for BotRunner<'_, B> {
    type Actions = [(Player, Direction); 1];

    fn runner_preprocess(&mut self, state: &GameState, ctx: &Context) {
        self.0.preprocess(state, ctx);
    }

    fn runner_think(&mut self, state: &GameState, ctx: &Context) -> Self::Actions {
        [(state.my_player(), self.0.think(state, ctx))]
    }

    fn runner_stay(state: &GameState) -> Self::Actions {
        [(state.my_player(), Direction::Stay)]
    }

    fn runner_on_game_over(&mut self, result: GameResult, scores: (f32, f32)) {
        self.0.on_game_over(result, scores);
    }
}

/// Newtype wrapper so Hivemind can implement Runner without blanket-impl conflicts.
pub(crate) struct HivemindRunner<'a, H: Hivemind>(pub &'a mut H);

impl<H: Hivemind> Options for HivemindRunner<'_, H> {
    fn option_defs(&self) -> Vec<crate::options::SdkOptionDef> {
        self.0.option_defs()
    }
    fn apply_option(&mut self, name: &str, value: &str) -> Result<(), String> {
        self.0.apply_option(name, value)
    }
}

impl<H: Hivemind> Runner for HivemindRunner<'_, H> {
    type Actions = [(Player, Direction); 2];

    fn runner_preprocess(&mut self, state: &GameState, ctx: &Context) {
        self.0.preprocess(state, ctx);
    }

    fn runner_think(&mut self, state: &GameState, ctx: &Context) -> Self::Actions {
        self.0.think(state, ctx)
    }

    fn runner_stay(_state: &GameState) -> Self::Actions {
        [
            (Player::Player1, Direction::Stay),
            (Player::Player2, Direction::Stay),
        ]
    }

    fn runner_on_game_over(&mut self, result: GameResult, scores: (f32, f32)) {
        self.0.on_game_over(result, scores);
    }
}
