use pyrat_sdk::{Bot, Context, Direction, GameState, Options};
use rand::prelude::IndexedRandom;

struct Greedy;

impl Options for Greedy {}

impl Bot for Greedy {
    fn think(&mut self, state: &GameState, ctx: &Context) -> Direction {
        let candidates = state.nearest_cheeses(None);
        let chosen = candidates.choose(&mut rand::rng());
        match chosen {
            Some(result) if !result.path.is_empty() => {
                let target = (result.target.x, result.target.y);
                ctx.send_info(
                    state.my_player(),
                    1,
                    Some(target),
                    0,
                    0,
                    0.0,
                    &result.path,
                    &format!(
                        "target ({}, {}), {} steps",
                        target.0,
                        target.1,
                        result.path.len()
                    ),
                );
                result.path[0]
            },
            _ => Direction::Stay,
        }
    }
}

fn main() {
    pyrat_sdk::run(Greedy, "Greedy", "PyRat");
}
