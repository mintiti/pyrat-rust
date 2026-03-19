//! Smart Random bot: picks a random valid direction each turn.
//!
//! Baseline opponent. Won't win, but won't walk into walls either.
//!
//! SDK features: `effective_moves`, `send_info`.

use pyrat_sdk::{Bot, Context, Direction, GameState, InfoParams, Options};
use rand::prelude::IndexedRandom;

struct SmartRandom;

impl Options for SmartRandom {}

impl Bot for SmartRandom {
    fn think(&mut self, state: &GameState, ctx: &Context) -> Direction {
        let moves = state.effective_moves(None);
        let chosen = *moves.choose(&mut rand::rng()).unwrap_or(&Direction::Stay);
        ctx.send_info(&InfoParams {
            pv: &[chosen],
            message: &format!("{chosen:?}"),
            ..InfoParams::for_player(state.my_player())
        });
        chosen
    }
}

fn main() {
    pyrat_sdk::run(SmartRandom, "Smart Random", "PyRat");
}
