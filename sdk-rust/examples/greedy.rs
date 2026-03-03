use pyrat_sdk::{Bot, Context, Direction, GameState, Options};
use rand::prelude::IndexedRandom;

struct Greedy;

impl Options for Greedy {}

impl Bot for Greedy {
    fn think(&mut self, state: &GameState, _ctx: &Context) -> Direction {
        let candidates = state.nearest_cheeses(None);
        let chosen = candidates.choose(&mut rand::rng());
        match chosen {
            Some(result) if !result.path.is_empty() => result.path[0],
            _ => Direction::Stay,
        }
    }
}

fn main() {
    pyrat_sdk::run(Greedy, "Greedy", "PyRat");
}
