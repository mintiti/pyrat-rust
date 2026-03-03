use pyrat_sdk::{Bot, Context, Direction, GameState, Options};

struct Greedy;

impl Options for Greedy {}

impl Bot for Greedy {
    fn think(&mut self, state: &GameState, _ctx: &Context) -> Direction {
        match state.nearest_cheese(None) {
            Some(result) if !result.path.is_empty() => result.path[0],
            _ => Direction::Stay,
        }
    }
}

fn main() {
    pyrat_sdk::run(Greedy, "Greedy", "PyRat");
}
