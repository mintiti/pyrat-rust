//! Sibling of `fixture-bot-a` — same behavior, distinct crate name so
//! the two `--bot` slots in the flags-only e2e test have separate
//! identities.

use pyrat_sdk::{Bot, Context, Direction, GameState, Options};

struct AlwaysStay;

impl Options for AlwaysStay {}

impl Bot for AlwaysStay {
    fn think(&mut self, _: &GameState, _: &Context) -> Direction {
        Direction::Stay
    }
}

fn main() {
    pyrat_sdk::run(AlwaysStay, "fixture-bot-b", "tests");
}
