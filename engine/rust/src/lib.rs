//! `PyRat` game engine implementation in Rust
//! Provides high-performance game logic for the `PyRat` maze game
#![allow(clippy::module_name_repetitions)] // Allow game_logic etc module names
#![allow(clippy::inline_always)] // We want aggressive inlining for performance
#![allow(clippy::trivially_copy_pass_by_ref)] // Trust the optimizer for small types
#![allow(clippy::redundant_pub_crate)] // Suppress warning about pub(crate) in pymodule
#![allow(clippy::must_use_candidate)]
#![allow(clippy::cargo_common_metadata)]

#[cfg(feature = "python")]
mod bindings;
pub mod game;

// Re-export commonly used items for Rust users
pub use game::{
    board::MoveTable,
    cheese_board::CheeseBoard,
    game_logic::GameState,
    maze_generation::{CheeseConfig, MazeConfig},
    types::{Coordinates, Direction},
};

// Export the Python module
#[cfg(feature = "python")]
use pyo3::prelude::*;

/// Python module for PyRat game
#[cfg(feature = "python")]
#[pymodule]
fn _rust(_py: Python<'_>, m: &PyModule) -> PyResult<()> {
    // Register all Python-facing types and functions
    bindings::register_module(m)?;
    Ok(())
}
