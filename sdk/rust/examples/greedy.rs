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
                let mut pos = state.my_position();
                let path: Vec<(u8, u8)> = result
                    .path
                    .iter()
                    .map(|d| {
                        pos = d.apply_to(pos);
                        (pos.x, pos.y)
                    })
                    .collect();
                ctx.send_info(
                    Some(target),
                    0,
                    0,
                    0.0,
                    &path,
                    &format!("target ({}, {}), {} steps", target.0, target.1, path.len()),
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
