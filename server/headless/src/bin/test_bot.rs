//! Minimal SDK-based test bot: picks a random direction each turn.
//!
//! Used by `tests/e2e.rs` as the live e2e smoke target. The new wire protocol
//! requires hash-verified handshakes, so a hand-rolled subprocess bot would
//! reimplement most of the SDK. Using the SDK here keeps the test honest:
//! anything that breaks the SDK breaks this test, and vice versa.

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
