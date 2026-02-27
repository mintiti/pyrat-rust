//! High-level interface for the PyRat game engine.
//!
//! This crate sits between the raw engine (`pyrat`) and SDK consumers (Rust bots,
//! Python bots via FFI). It provides:
//!
//! - [`Maze`] — borrow-bundle for the static maze topology (walls, mud, dimensions)
//!   with graph query methods (neighbors, edge costs, valid moves).
//! - [`GameView`] — SDK-facing wrapper over a [`GameState`] that delegates graph and
//!   pathfinding queries. Use [`GameView::snapshot`] to get a mutable copy for
//!   simulation without affecting the view.
//! - [`pathfinding`] — Dijkstra-based shortest paths and distance maps, all taking
//!   `&Maze`.
//!
//! ## When to use what
//!
//! | Need | Use |
//! |------|-----|
//! | Graph queries on current game | [`GameView`] delegation methods |
//! | Graph queries outside a game context | [`Maze`] directly |
//! | Point-to-point pathfinding | [`shortest_path`] or [`GameView::shortest_path`] |
//! | Nearest cheese search | [`nearest_cheeses`] or [`GameView::nearest_cheeses`] |
//! | Check if game ended | [`GameView::is_game_over`] |
//! | Check for cheese at a cell | [`GameView::cheese_at`] |
//! | Simulate future moves | [`GameView::snapshot`] → `make_move` / `unmake_move` |

pub mod maze;
pub mod pathfinding;
pub mod view;

// Re-export engine types that SDKs need
pub use pyrat::{Coordinates, Direction, GameState, MoveUndo, Mud, Wall};

// Re-export interface types
pub use maze::Maze;
pub use pathfinding::{distances_from, nearest_cheeses, shortest_path, PathResult};
pub use view::{GameView, PlayerSnapshot};
