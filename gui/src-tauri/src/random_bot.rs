//! In-process random bot for `__random__` GUI slots.
//!
//! Implements [`EmbeddedBot`] so it slots into a `Match` next to a real
//! `TcpPlayer` via [`EmbeddedPlayer::accept`]. Picks a uniformly random
//! cardinal direction every turn (no Stay).

use pyrat::Direction;
use pyrat_host::player::{EmbeddedBot, EmbeddedCtx, Options};
use pyrat_protocol::HashedTurnState;

const MOVES: [Direction; 4] = [
    Direction::Up,
    Direction::Right,
    Direction::Down,
    Direction::Left,
];

pub struct RandomBot;

impl Options for RandomBot {}

impl EmbeddedBot for RandomBot {
    fn think(&mut self, _state: &HashedTurnState, _ctx: &EmbeddedCtx) -> Direction {
        MOVES[fastrand::usize(..MOVES.len())]
    }
}
