//! Minimal fixture bot: always stays. Deterministic, finishes a tiny
//! game in `max_turns` turns. Used by the `flags_only_runs_through`
//! CLI test to exercise `--bot id=working_dir` shorthand against a bot
//! that's *not* in the root workspace (so `cargo run --release` here
//! doesn't resolve to `pyrat-eval`).

use pyrat_sdk::{Bot, Context, Direction, GameState, Options};

struct AlwaysStay;

impl Options for AlwaysStay {}

impl Bot for AlwaysStay {
    fn think(&mut self, _: &GameState, _: &Context) -> Direction {
        Direction::Stay
    }
}

fn main() {
    pyrat_sdk::run(AlwaysStay, "fixture-bot-a", "tests");
}
