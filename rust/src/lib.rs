//! PyRat game engine implementation in Rust
//! Provides high-performance game logic for the PyRat maze game

pub mod game;
mod bindings;

// Re-export commonly used items for Rust users
pub use game::{
    board::MoveTable,
    cheese_board::CheeseBoard,
    game::GameState,
    types::{Coordinates, Direction},
    maze_generation::{MazeConfig, CheeseConfig},
};
// Export the Python module
use pyo3::prelude::*;

/// Python module for PyRat game
#[pymodule]
fn _rust(_py: Python<'_>, m: &PyModule) -> PyResult<()> {
    // Register all Python-facing types and functions
    bindings::env::register_module(m)?;
    Ok(())
}