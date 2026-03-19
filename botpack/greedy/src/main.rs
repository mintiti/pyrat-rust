//! Greedy bot: always moves toward the nearest cheese.
//!
//! Strategy: each turn, find all cheeses at minimum distance and pick one randomly.
//! Simple and effective. A good first opponent and a baseline to beat.
//!
//! SDK features: `nearest_cheeses` (pathfinding), `send_info` (GUI visualization).

use pyrat_sdk::{Bot, Context, Direction, GameState, InfoParams, Options};
use rand::prelude::IndexedRandom;

struct Greedy;

impl Options for Greedy {}

impl Bot for Greedy {
    fn think(&mut self, state: &GameState, ctx: &Context) -> Direction {
        let candidates = state.nearest_cheeses(None);
        // Pick randomly among tied cheeses so we don't always chase the same one.
        let chosen = candidates.choose(&mut rand::rng());
        match chosen {
            Some(result) if !result.path.is_empty() => {
                let target = (result.target.x, result.target.y);
                ctx.send_info(&InfoParams {
                    multipv: 1,
                    target: Some(target),
                    score: state.my_score() + 1.0,
                    pv: &result.path,
                    message: &format!(
                        "target ({}, {}), {} steps",
                        target.0,
                        target.1,
                        result.path.len()
                    ),
                    ..InfoParams::for_player(state.my_player())
                });
                result.path[0]
            },
            _ => Direction::Stay,
        }
    }
}

fn main() {
    pyrat_sdk::run(Greedy, "Greedy", "PyRat");
}
