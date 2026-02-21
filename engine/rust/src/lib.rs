//! `PyRat` game engine implementation in Rust
//! Provides high-performance game logic for the `PyRat` maze game
#![allow(clippy::module_name_repetitions)] // Allow game_logic etc module names
#![allow(clippy::inline_always)] // We want aggressive inlining for performance
#![allow(clippy::trivially_copy_pass_by_ref)] // Trust the optimizer for small types
#![allow(clippy::redundant_pub_crate)] // pyo3 #[pymethods] generates pub(crate) items in bindings
#![allow(clippy::must_use_candidate)]
#![allow(non_local_definitions)] // pyo3 #[pymethods] in bindings expands to non-local impls

pub mod bench_scenarios;
#[cfg(feature = "python")]
pub mod bindings;
pub mod game;

// Re-export commonly used items for Rust users
#[cfg(feature = "python")]
pub use bindings::game::PyRat;
pub use game::{
    board::MoveTable,
    builder::{GameBuilder, GameConfig, MazeParams},
    cheese_board::CheeseBoard,
    game_logic::GameState,
    maze_generation::{CheeseConfig, MazeConfig},
    types::{Coordinates, Direction, Mud, Wall},
};
