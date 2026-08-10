use crate::{Coordinates, Direction, Wall};
use std::collections::HashMap;
use std::sync::Arc;

/// Pre-computed valid moves lookup table with packed storage
#[derive(Clone)]
pub struct MoveTable {
    // Each byte stores moves for two positions
    // Bits 0-3: moves for position 2n
    // Bits 4-7: moves for position 2n+1
    valid_moves: Arc<[u8]>,
    width: u8,
}

impl MoveTable {
    #[must_use]
    pub fn new(width: u8, height: u8, walls: &HashMap<Coordinates, Vec<Coordinates>>) -> Self {
        let size = (width as usize * height as usize).div_ceil(2);
        let mut valid_moves = vec![0u8; size];

        // Precompute all valid moves for each position
        for y in 0..height {
            for x in 0..width {
                let pos = Coordinates::new(x, y);
                let mut moves = 0u8;

                // Check each direction with correct bounds for our coordinate system
                if y < height - 1 && !has_wall(walls, pos, Direction::Up) {
                    // Up is towards height-1
                    moves |= 1;
                }
                if x < width - 1 && !has_wall(walls, pos, Direction::Right) {
                    moves |= 2;
                }
                if y > 0 && !has_wall(walls, pos, Direction::Down) {
                    // Down is towards 0
                    moves |= 4;
                }
                if x > 0 && !has_wall(walls, pos, Direction::Left) {
                    moves |= 8;
                }

                // Pack moves into the correct half-byte
                let idx = pos.to_index(width);
                let byte_idx = idx / 2;
                if idx.is_multiple_of(2) {
                    // Even index - use lower 4 bits
                    valid_moves[byte_idx] |= moves;
                } else {
                    // Odd index - use upper 4 bits
                    valid_moves[byte_idx] |= moves << 4;
                }
            }
        }

        Self {
            valid_moves: valid_moves.into(),
            width,
        }
    }

    /// Pack one four-direction movement mask per cell into the runtime table.
    #[must_use]
    pub(crate) fn from_cell_masks(width: u8, height: u8, masks: &[u8]) -> Self {
        let cell_count = usize::from(width) * usize::from(height);
        assert_eq!(
            masks.len(),
            cell_count,
            "movement mask count must match board dimensions"
        );
        debug_assert!(
            masks.iter().all(|mask| mask & !0x0f == 0),
            "cell movement masks must use only the low four bits"
        );

        let mut valid_moves = Vec::with_capacity(cell_count.div_ceil(2));
        for pair in masks.chunks(2) {
            let low = pair[0];
            let high = pair.get(1).copied().unwrap_or(0);
            valid_moves.push(low | (high << 4));
        }

        Self {
            valid_moves: valid_moves.into(),
            width,
        }
    }

    /// Build the packed movement masks for a board with no internal walls.
    #[must_use]
    pub(crate) fn open(width: u8, height: u8) -> Self {
        let size = (width as usize * height as usize).div_ceil(2);
        let mut valid_moves = vec![0u8; size];

        for y in 0..height {
            for x in 0..width {
                let mut moves = 0u8;
                if y < height - 1 {
                    moves |= 1;
                }
                if x < width - 1 {
                    moves |= 2;
                }
                if y > 0 {
                    moves |= 4;
                }
                if x > 0 {
                    moves |= 8;
                }

                let idx = Coordinates::new(x, y).to_index(width);
                let byte_idx = idx / 2;
                if idx.is_multiple_of(2) {
                    valid_moves[byte_idx] |= moves;
                } else {
                    valid_moves[byte_idx] |= moves << 4;
                }
            }
        }

        Self {
            valid_moves: valid_moves.into(),
            width,
        }
    }

    /// Check if a move is valid for a given position
    #[inline(always)]
    #[must_use]
    pub fn is_move_valid(&self, pos: Coordinates, direction: Direction) -> bool {
        let idx = pos.to_index(self.width);
        let byte_idx = idx / 2;
        let moves = self.valid_moves[byte_idx];

        // Extract the correct 4 bits based on whether index is even or odd
        let position_moves = if idx.is_multiple_of(2) {
            moves & 0x0F
        } else {
            moves >> 4
        };

        position_moves & (1 << direction as u8) != 0
    }

    /// Raw byte slice of the packed move table, for hashing.
    #[must_use]
    #[inline(always)]
    pub fn bytes(&self) -> &[u8] {
        self.valid_moves.as_ref()
    }

    /// Iterator of valid cardinal directions from a position.
    pub fn valid_directions(&self, pos: Coordinates) -> impl Iterator<Item = Direction> + '_ {
        let mask = self.get_valid_moves(pos);
        Direction::CARDINALS
            .into_iter()
            .filter(move |&dir| mask & (1 << dir as u8) != 0)
    }

    /// Bulk check of all valid moves for a position
    /// Returns a bitmask of valid moves
    #[inline(always)]
    #[must_use]
    pub fn get_valid_moves(&self, pos: Coordinates) -> u8 {
        let idx = pos.to_index(self.width);
        let byte_idx = idx / 2;
        let moves = self.valid_moves[byte_idx];

        if idx.is_multiple_of(2) {
            moves & 0x0F
        } else {
            moves >> 4
        }
    }

    /// Reconstruct each internal wall once from the packed movement masks.
    pub(crate) fn wall_entries(&self, width: u8, height: u8) -> Vec<Wall> {
        debug_assert_eq!(self.width, width);

        let mut walls = Vec::new();
        for y in 0..height {
            for x in 0..width {
                let pos = Coordinates::new(x, y);
                if x + 1 < width && !self.is_move_valid(pos, Direction::Right) {
                    walls.push(Wall {
                        pos1: pos,
                        pos2: Coordinates::new(x + 1, y),
                    });
                }
                if y + 1 < height && !self.is_move_valid(pos, Direction::Up) {
                    walls.push(Wall {
                        pos1: pos,
                        pos2: Coordinates::new(x, y + 1),
                    });
                }
            }
        }
        walls
    }

    #[cfg(test)]
    pub(crate) fn shares_storage_with(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.valid_moves, &other.valid_moves)
    }
}

/// Check if there's a wall between two adjacent positions
#[inline(always)]
fn has_wall(
    walls: &HashMap<Coordinates, Vec<Coordinates>>,
    from: Coordinates,
    direction: Direction,
) -> bool {
    // First check if the position has any walls at all
    if let Some(blocked_cells) = walls.get(&from) {
        // Then check if the destination in that direction is blocked
        let to = direction.apply_to(from);
        if blocked_cells.contains(&to) {
            return true;
        }
        // Also check from the other direction
        if let Some(blocked_from) = walls.get(&to) {
            return blocked_from.contains(&from);
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Creates walls in a 2x2 maze with a vertical wall in the middle:
    /// ```text
    ///  0,1  |  1,1
    ///       |
    ///  0,0  |  1,0
    /// ```
    fn create_vertical_wall() -> HashMap<Coordinates, Vec<Coordinates>> {
        let mut walls = HashMap::new();

        // Vertical wall between (0,0) and (1,0), and between (0,1) and (1,1)
        walls.insert(Coordinates::new(0, 0), vec![Coordinates::new(1, 0)]);
        walls.insert(Coordinates::new(1, 0), vec![Coordinates::new(0, 0)]);
        walls.insert(Coordinates::new(0, 1), vec![Coordinates::new(1, 1)]);
        walls.insert(Coordinates::new(1, 1), vec![Coordinates::new(0, 1)]);

        walls
    }

    /// Creates walls in a 2x2 maze with a horizontal wall in the middle:
    /// ```text
    ///  0,1   1,1
    ///  -------
    ///  0,0   1,0
    /// ```
    fn create_horizontal_wall() -> HashMap<Coordinates, Vec<Coordinates>> {
        let mut walls = HashMap::new();

        // Horizontal wall between (0,0) and (0,1), and between (1,0) and (1,1)
        walls.insert(Coordinates::new(0, 0), vec![Coordinates::new(0, 1)]);
        walls.insert(Coordinates::new(0, 1), vec![Coordinates::new(0, 0)]);
        walls.insert(Coordinates::new(1, 0), vec![Coordinates::new(1, 1)]);
        walls.insert(Coordinates::new(1, 1), vec![Coordinates::new(1, 0)]);

        walls
    }

    #[test]
    fn test_empty_2x2() {
        let width = 2;
        let height = 2;
        let walls = HashMap::new(); // No walls
        let move_table = MoveTable::new(width, height, &walls);

        // Test all positions in empty 2x2 grid
        // Bottom-left (0,0)
        assert!(move_table.is_move_valid(Coordinates::new(0, 0), Direction::Up));
        assert!(move_table.is_move_valid(Coordinates::new(0, 0), Direction::Right));
        assert!(!move_table.is_move_valid(Coordinates::new(0, 0), Direction::Down)); // Grid boundary
        assert!(!move_table.is_move_valid(Coordinates::new(0, 0), Direction::Left)); // Grid boundary

        // Bottom-right (1,0)
        assert!(move_table.is_move_valid(Coordinates::new(1, 0), Direction::Up));
        assert!(!move_table.is_move_valid(Coordinates::new(1, 0), Direction::Right)); // Grid boundary
        assert!(!move_table.is_move_valid(Coordinates::new(1, 0), Direction::Down)); // Grid boundary
        assert!(move_table.is_move_valid(Coordinates::new(1, 0), Direction::Left));

        // Top-left (0,1)
        assert!(!move_table.is_move_valid(Coordinates::new(0, 1), Direction::Up)); // Grid boundary
        assert!(move_table.is_move_valid(Coordinates::new(0, 1), Direction::Right));
        assert!(move_table.is_move_valid(Coordinates::new(0, 1), Direction::Down));
        assert!(!move_table.is_move_valid(Coordinates::new(0, 1), Direction::Left)); // Grid boundary

        // Top-right (1,1)
        assert!(!move_table.is_move_valid(Coordinates::new(1, 1), Direction::Up)); // Grid boundary
        assert!(!move_table.is_move_valid(Coordinates::new(1, 1), Direction::Right)); // Grid boundary
        assert!(move_table.is_move_valid(Coordinates::new(1, 1), Direction::Down));
        assert!(move_table.is_move_valid(Coordinates::new(1, 1), Direction::Left));
    }

    #[test]
    fn open_constructor_matches_empty_wall_compilation() {
        let walls = HashMap::new();

        for (width, height) in [(2, 2), (3, 4), (21, 15), (41, 31)] {
            let compiled = MoveTable::new(width, height, &walls);
            let open = MoveTable::open(width, height);

            assert_eq!(open.bytes(), compiled.bytes(), "{width}x{height}");
        }
    }

    #[test]
    fn test_vertical_wall() {
        let width = 2;
        let height = 2;
        let walls = create_vertical_wall();
        let move_table = MoveTable::new(width, height, &walls);

        // Test positions with vertical wall in middle
        // Bottom-left (0,0)
        assert!(move_table.is_move_valid(Coordinates::new(0, 0), Direction::Up));
        assert!(!move_table.is_move_valid(Coordinates::new(0, 0), Direction::Right)); // Wall
        assert!(!move_table.is_move_valid(Coordinates::new(0, 0), Direction::Down)); // Grid boundary
        assert!(!move_table.is_move_valid(Coordinates::new(0, 0), Direction::Left)); // Grid boundary

        // Bottom-right (1,0)
        assert!(move_table.is_move_valid(Coordinates::new(1, 0), Direction::Up));
        assert!(!move_table.is_move_valid(Coordinates::new(1, 0), Direction::Right)); // Grid boundary
        assert!(!move_table.is_move_valid(Coordinates::new(1, 0), Direction::Down)); // Grid boundary
        assert!(!move_table.is_move_valid(Coordinates::new(1, 0), Direction::Left)); // Wall

        // Verify wall blocks both directions
        assert!(!move_table.is_move_valid(Coordinates::new(0, 0), Direction::Right));
        assert!(!move_table.is_move_valid(Coordinates::new(1, 0), Direction::Left));
    }

    #[test]
    fn test_horizontal_wall() {
        let width = 2;
        let height = 2;
        let walls = create_horizontal_wall();
        let move_table = MoveTable::new(width, height, &walls);

        // Test positions with horizontal wall in middle
        // Bottom-left (0,0)
        assert!(!move_table.is_move_valid(Coordinates::new(0, 0), Direction::Up)); // Wall
        assert!(move_table.is_move_valid(Coordinates::new(0, 0), Direction::Right));
        assert!(!move_table.is_move_valid(Coordinates::new(0, 0), Direction::Down)); // Grid boundary
        assert!(!move_table.is_move_valid(Coordinates::new(0, 0), Direction::Left)); // Grid boundary

        // Top-left (0,1)
        assert!(!move_table.is_move_valid(Coordinates::new(0, 1), Direction::Up)); // Grid boundary
        assert!(move_table.is_move_valid(Coordinates::new(0, 1), Direction::Right));
        assert!(!move_table.is_move_valid(Coordinates::new(0, 1), Direction::Down)); // Wall
        assert!(!move_table.is_move_valid(Coordinates::new(0, 1), Direction::Left)); // Grid boundary

        // Verify wall blocks both directions
        assert!(!move_table.is_move_valid(Coordinates::new(0, 0), Direction::Up));
        assert!(!move_table.is_move_valid(Coordinates::new(0, 1), Direction::Down));
    }

    #[test]
    fn test_intersecting_walls() {
        let width = 2;
        let height = 2;
        let mut walls = HashMap::new();

        // Add horizontal and vertical walls making a T shape
        // Vertical wall between (0,0) and (1,0)
        walls.insert(Coordinates::new(0, 0), vec![Coordinates::new(1, 0)]);
        walls.insert(Coordinates::new(1, 0), vec![Coordinates::new(0, 0)]);

        // Horizontal wall between (0,0) and (0,1)
        walls.insert(Coordinates::new(0, 0), vec![Coordinates::new(0, 1)]);
        walls.insert(Coordinates::new(0, 1), vec![Coordinates::new(0, 0)]);

        let move_table = MoveTable::new(width, height, &walls);

        // Check bottom-left corner (0,0) - should be blocked in two directions
        let pos = Coordinates::new(0, 0);
        assert!(!move_table.is_move_valid(pos, Direction::Right)); // Wall
        assert!(!move_table.is_move_valid(pos, Direction::Up)); // Wall
        assert!(!move_table.is_move_valid(pos, Direction::Left)); // Grid boundary
        assert!(!move_table.is_move_valid(pos, Direction::Down)); // Grid boundary
    }
}
