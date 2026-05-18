//! Minimal SDK-based test bot used by `tests/cli_run_one.rs`. Picks a random
//! direction each turn. Going through the SDK (instead of hand-rolling a
//! subprocess bot) keeps the test honest: anything that breaks the SDK
//! breaks this test, and vice versa.

use pyrat_sdk::{Bot, Context, Direction, GameState, Options};
use rand::prelude::IndexedRandom;

const DIRS: [Direction; 5] = [
    Direction::Up,
    Direction::Right,
    Direction::Down,
    Direction::Left,
    Direction::Stay,
];

struct TestBot;

impl Options for TestBot {}

impl Bot for TestBot {
    fn think(&mut self, _state: &GameState, _ctx: &Context) -> Direction {
        *DIRS.choose(&mut rand::rng()).unwrap_or(&Direction::Stay)
    }
}

fn main() {
    pyrat_sdk::run(TestBot, "TestBot", "test");
}
