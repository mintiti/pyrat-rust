//! Immutable maze topology that can be shared by independent games.

use super::board::MoveTable;
use super::types::{MudMap, Wall};
use super::zobrist;
use crate::Coordinates;
use std::collections::HashMap;

/// A generated and compiled maze that can be reused across games.
///
/// `MazeLayout` contains only static board topology. Cloning it shares its
/// packed movement table and mud storage; each created [`GameState`](crate::GameState)
/// still owns independent players, cheese, scores, turns, and undo state.
#[derive(Clone)]
pub struct MazeLayout {
    width: u8,
    height: u8,
    move_table: MoveTable,
    mud: MudMap,
    topology_hash: u64,
}

impl MazeLayout {
    pub(crate) fn new(
        width: u8,
        height: u8,
        move_table: MoveTable,
        mud: MudMap,
        topology_hash: u64,
    ) -> Self {
        let mud = mud.freeze(width, height);
        Self {
            width,
            height,
            move_table,
            mud,
            topology_hash,
        }
    }

    pub(crate) fn from_walls(
        width: u8,
        height: u8,
        walls: &HashMap<Coordinates, Vec<Coordinates>>,
        mud: MudMap,
    ) -> Self {
        let move_table = MoveTable::new(width, height, walls);
        let topology_hash = zobrist::maze_hash(&move_table, &mud, width, height);
        Self::new(width, height, move_table, mud, topology_hash)
    }

    /// Board width in cells.
    #[must_use]
    pub const fn width(&self) -> u8 {
        self.width
    }

    /// Board height in cells.
    #[must_use]
    pub const fn height(&self) -> u8 {
        self.height
    }

    /// Packed valid-movement table for this maze.
    #[must_use]
    pub const fn move_table(&self) -> &MoveTable {
        &self.move_table
    }

    /// Mud passages and their traversal costs.
    #[must_use]
    pub const fn mud_positions(&self) -> &MudMap {
        &self.mud
    }

    /// Reconstruct each internal wall once from the packed movement table.
    #[must_use]
    pub fn wall_entries(&self) -> Vec<Wall> {
        self.move_table.wall_entries(self.width, self.height)
    }

    /// Content-addressable hash of the static maze topology.
    #[must_use]
    pub const fn topology_hash(&self) -> u64 {
        self.topology_hash
    }

    pub(crate) fn into_parts(self) -> (u8, u8, MoveTable, MudMap, u64) {
        (
            self.width,
            self.height,
            self.move_table,
            self.mud,
            self.topology_hash,
        )
    }

    #[cfg(test)]
    pub(crate) fn shares_storage_with(&self, other: &Self) -> bool {
        self.move_table.shares_storage_with(&other.move_table)
            && self.mud.shares_storage_with(&other.mud)
    }
}
