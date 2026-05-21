//! Fixture bot used by `tests/cli_run_one.rs`. Picks a random direction each
//! turn. Going through the SDK (instead of hand-rolling a subprocess bot)
//! keeps the test honest: anything that breaks the SDK breaks this test, and
//! vice versa.
//!
//! The bot's PRNG is seeded from `PYRAT_TEST_BOT_SEED` so the fixture tests
//! get deterministic action streams. Without the env var the bot falls back
//! to OS entropy.

use pyrat_sdk::{Bot, Context, Direction, GameState, Options};
use rand::prelude::IndexedRandom;
use rand::rngs::StdRng;
use rand::SeedableRng;

const DIRS: [Direction; 5] = [
    Direction::Up,
    Direction::Right,
    Direction::Down,
    Direction::Left,
    Direction::Stay,
];

struct TestBot {
    rng: StdRng,
}

impl Options for TestBot {}

impl Bot for TestBot {
    fn think(&mut self, _state: &GameState, _ctx: &Context) -> Direction {
        *DIRS.choose(&mut self.rng).unwrap_or(&Direction::Stay)
    }
}

fn main() {
    let rng: StdRng = std::env::var("PYRAT_TEST_BOT_SEED")
        .ok()
        .and_then(|s| s.parse::<u64>().ok())
        .map_or_else(rand::make_rng, StdRng::seed_from_u64);
    pyrat_sdk::run(TestBot { rng }, "TestBot", "test");
}
