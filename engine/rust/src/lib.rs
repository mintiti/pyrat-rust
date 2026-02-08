//! `PyRat` game engine implementation in Rust
//! Provides high-performance game logic for the `PyRat` maze game
#![allow(clippy::module_name_repetitions)] // Allow game_logic etc module names
#![allow(clippy::inline_always)] // We want aggressive inlining for performance
#![allow(clippy::trivially_copy_pass_by_ref)] // Trust the optimizer for small types
#![allow(clippy::redundant_pub_crate)] // Suppress warning about pub(crate) in pymodule
#![allow(clippy::must_use_candidate)]
#![allow(clippy::cargo_common_metadata)]
#![allow(non_local_definitions)] // pyo3 #[pymethods] expands to non-local impls

pub mod bench_scenarios;
#[cfg(feature = "python")]
mod bindings;
pub mod game;

// Re-export commonly used items for Rust users
pub use game::{
    board::MoveTable,
    cheese_board::CheeseBoard,
    game_logic::GameState,
    maze_generation::{CheeseConfig, MazeConfig},
    types::{Coordinates, Direction, Mud, Wall},
};

// Export the Python module
#[cfg(feature = "python")]
use pyo3::prelude::*;

/// Python module for PyRat game engine core implementation
#[cfg(feature = "python")]
#[pymodule]
fn _core(m: &Bound<'_, PyModule>) -> PyResult<()> {
    // Create submodules
    let types_module = PyModule::new(m.py(), "types")?;
    bindings::register_types_module(&types_module)?;
    m.add_submodule(&types_module)?;

    let game_module = PyModule::new(m.py(), "game")?;
    bindings::register_game_module(&game_module)?;
    m.add_submodule(&game_module)?;

    let observation_module = PyModule::new(m.py(), "observation")?;
    bindings::register_observation_module(&observation_module)?;
    m.add_submodule(&observation_module)?;

    let builder_module = PyModule::new(m.py(), "builder")?;
    bindings::register_builder_module(&builder_module)?;
    m.add_submodule(&builder_module)?;

    Ok(())
}
