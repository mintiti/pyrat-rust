pub mod maze;
pub mod pathfinding;
pub mod view;

// Re-export engine types that SDKs need
pub use pyrat::{Coordinates, Direction, GameState, MoveUndo, Mud, Wall};

// Re-export interface types
pub use maze::{direction_between, Maze};
pub use pathfinding::{distances_from, nearest_cheeses, shortest_path, PathResult};
pub use view::{GameView, PlayerSnapshot};
