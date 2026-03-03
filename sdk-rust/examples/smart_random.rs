use pyrat_sdk::{Bot, Context, Direction, GameState, Options};
use rand::seq::IndexedRandom;

struct SmartRandom;

impl Options for SmartRandom {}

impl Bot for SmartRandom {
    fn think(&mut self, state: &GameState, _ctx: &Context) -> Direction {
        let moves = state.effective_moves(None);
        *moves.choose(&mut rand::rng()).unwrap_or(&Direction::Stay)
    }
}

fn main() {
    pyrat_sdk::run(SmartRandom, "Smart Random", "PyRat");
}
