//! PyRat game engine implementation in Rust
//! Provides high-performance game logic for the PyRat maze game

pub mod types;
pub mod board;
pub mod game;
pub mod cheese_board;

// Re-export commonly used items
pub use types::{Coordinates, Direction};
pub use game::{GameState, TurnResult};
pub use board::{MoveTable};
pub use cheese_board::{CheeseBoard};