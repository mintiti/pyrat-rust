use std::hash::{Hash, Hasher};

use crate::Coordinates;

/// Efficient cheese tracking using bitboards
pub struct CheeseBoard {
    // Each u64 represents 64 cells
    bits: Vec<u64>,
    width: u8,
    initial_cheese_count: u16,   // Total number of cheese pieces at start
    remaining_cheese_count: u16, // Current number of remaining pieces
}

impl CheeseBoard {
    #[must_use]
    #[inline(always)]
    pub fn new(width: u8, height: u8) -> Self {
        let total_cells = width as usize * height as usize;
        let size = total_cells.div_ceil(64); // Round up to nearest 64 cells
        Self {
            bits: vec![0; size],
            width,
            initial_cheese_count: 0,
            remaining_cheese_count: 0,
        }
    }

    #[must_use]
    #[inline(always)]
    pub fn has_cheese(&self, pos: Coordinates) -> bool {
        let idx = pos.to_index(self.width);
        let word_idx = idx / 64;
        let bit_idx = idx % 64;
        (self.bits[word_idx] & (1u64 << bit_idx)) != 0
    }

    /// Places a new cheese piece, returning true if successful
    #[inline]
    pub fn place_cheese(&mut self, pos: Coordinates) -> bool {
        let idx = pos.to_index(self.width);
        let word_idx = idx / 64;
        let bit_idx = idx % 64;
        let mask = 1u64 << bit_idx;

        // Check if cheese already exists
        if (self.bits[word_idx] & mask) != 0 {
            return false;
        }

        self.bits[word_idx] |= mask;
        self.initial_cheese_count += 1;
        self.remaining_cheese_count += 1;
        true
    }

    /// Special method for restoring cheese during unmake operations
    /// This only affects `remaining_cheese_count`, not `initial_cheese_count`
    #[inline]
    pub fn restore_cheese(&mut self, pos: Coordinates) -> bool {
        let idx = pos.to_index(self.width);
        let word_idx = idx / 64;
        let bit_idx = idx % 64;
        let mask = 1u64 << bit_idx;

        // Check if cheese already exists
        if (self.bits[word_idx] & mask) != 0 {
            return false;
        }

        self.bits[word_idx] |= mask;
        self.remaining_cheese_count += 1;
        true
    }

    #[inline]
    pub fn take_cheese(&mut self, pos: Coordinates) -> bool {
        let idx = pos.to_index(self.width);
        let word_idx = idx / 64;
        let bit_idx = idx % 64;
        let mask = 1u64 << bit_idx;
        let had_cheese = (self.bits[word_idx] & mask) != 0;

        if had_cheese {
            self.bits[word_idx] &= !mask;
            self.remaining_cheese_count -= 1;
        }

        had_cheese
    }

    /// Returns the initial number of cheese pieces placed
    #[must_use]
    #[inline(always)]
    pub const fn total_cheese(&self) -> u16 {
        self.initial_cheese_count
    }

    /// Returns the current number of cheese pieces remaining
    #[must_use]
    #[inline(always)]
    pub const fn remaining_cheese(&self) -> u16 {
        self.remaining_cheese_count
    }

    /// Returns a vector of all cheese positions.
    ///
    /// # Panics
    /// May panic if internal index calculations overflow u8 bounds.
    /// This should never happen with valid board dimensions.
    #[must_use]
    pub fn get_all_cheese_positions(&self) -> Vec<Coordinates> {
        let mut positions = Vec::with_capacity(self.remaining_cheese_count as usize);

        for word_idx in 0..self.bits.len() {
            let mut word = self.bits[word_idx];
            if word == 0 {
                continue;
            } // Skip empty words

            let base_idx = word_idx * 64;
            while word != 0 {
                let trailing_zeros = word.trailing_zeros() as usize;
                let idx = base_idx + trailing_zeros;
                let x = u8::try_from(idx % self.width as usize).unwrap();
                let y = u8::try_from(idx / self.width as usize).unwrap();

                positions.push(Coordinates::new(x, y));

                // Clear the processed bit and continue
                word &= !(1u64 << trailing_zeros);
            }
        }

        positions
    }

    /// Count cheese in a specific area - useful for heuristics
    #[must_use]
    pub fn count_cheese_in_area(&self, top_left: Coordinates, bottom_right: Coordinates) -> u16 {
        let mut count = 0;

        for y in top_left.y..=bottom_right.y {
            for x in top_left.x..=bottom_right.x {
                if self.has_cheese(Coordinates::new(x, y)) {
                    count += 1;
                }
            }
        }

        count
    }

    /// Returns a reference to the raw bitboard data.
    ///
    /// Each `u64` represents 64 cells. Cell at `Coordinates { x, y }` maps to
    /// bit `(y * width + x)` in the corresponding word.
    #[must_use]
    #[inline(always)]
    pub fn bits(&self) -> &[u64] {
        &self.bits
    }

    /// Clear all cheese
    #[inline]
    pub fn clear(&mut self) {
        self.bits.fill(0);
        self.initial_cheese_count = 0;
        self.remaining_cheese_count = 0;
    }
}

impl Clone for CheeseBoard {
    fn clone(&self) -> Self {
        Self {
            bits: self.bits.clone(),
            width: self.width,
            initial_cheese_count: self.initial_cheese_count,
            remaining_cheese_count: self.remaining_cheese_count,
        }
    }
}

impl PartialEq for CheeseBoard {
    fn eq(&self, other: &Self) -> bool {
        self.width == other.width && self.bits == other.bits
    }
}

impl Eq for CheeseBoard {}

impl Hash for CheeseBoard {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.width.hash(state);
        self.bits.hash(state);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cheese_counting() {
        let mut board = CheeseBoard::new(3, 3);

        // Place three cheese pieces
        assert!(board.place_cheese(Coordinates::new(0, 0)));
        assert!(board.place_cheese(Coordinates::new(1, 1)));
        assert!(board.place_cheese(Coordinates::new(2, 2)));

        assert_eq!(board.total_cheese(), 3);
        assert_eq!(board.remaining_cheese(), 3);

        // Take one cheese
        assert!(board.take_cheese(Coordinates::new(1, 1)));
        assert_eq!(board.total_cheese(), 3); // Total should stay the same
        assert_eq!(board.remaining_cheese(), 2); // Remaining should decrease
    }

    #[test]
    fn test_cheese_placement_and_removal() {
        let mut board = CheeseBoard::new(3, 3);
        let pos = Coordinates::new(1, 1);

        // Initial placement
        assert!(board.place_cheese(pos));
        assert_eq!(board.total_cheese(), 1);
        assert_eq!(board.remaining_cheese(), 1);

        // Try placing in same spot (should fail)
        assert!(!board.place_cheese(pos));
        assert_eq!(board.total_cheese(), 1);
        assert_eq!(board.remaining_cheese(), 1);

        // Remove cheese
        assert!(board.take_cheese(pos));
        assert_eq!(board.total_cheese(), 1); // Total unchanged
        assert_eq!(board.remaining_cheese(), 0); // Remaining decremented

        // Try removing again (should fail)
        assert!(!board.take_cheese(pos));
        assert_eq!(board.total_cheese(), 1);
        assert_eq!(board.remaining_cheese(), 0);
    }
    #[test]
    fn test_cheese_restore() {
        let mut board = CheeseBoard::new(3, 3);
        let pos = Coordinates::new(1, 1);

        // Initial placement
        assert!(board.place_cheese(pos));
        assert_eq!(board.total_cheese(), 1);
        assert_eq!(board.remaining_cheese(), 1);

        // Take cheese
        assert!(board.take_cheese(pos));
        assert_eq!(board.total_cheese(), 1); // Total unchanged
        assert_eq!(board.remaining_cheese(), 0);

        // Restore cheese
        assert!(board.restore_cheese(pos));
        assert_eq!(board.total_cheese(), 1); // Total should still be unchanged
        assert_eq!(board.remaining_cheese(), 1); // Remaining should be restored

        // Try to restore again (should fail)
        assert!(!board.restore_cheese(pos));
        assert_eq!(board.total_cheese(), 1);
        assert_eq!(board.remaining_cheese(), 1);
    }
    #[test]
    fn test_multiple_cheese_operations() {
        let mut board = CheeseBoard::new(3, 3);
        let pos1 = Coordinates::new(0, 0);
        let pos2 = Coordinates::new(1, 1);

        // Place two cheese pieces
        assert!(board.place_cheese(pos1));
        assert!(board.place_cheese(pos2));
        assert_eq!(board.total_cheese(), 2);
        assert_eq!(board.remaining_cheese(), 2);

        // Take both pieces
        assert!(board.take_cheese(pos1));
        assert!(board.take_cheese(pos2));
        assert_eq!(board.total_cheese(), 2);
        assert_eq!(board.remaining_cheese(), 0);

        // Restore in reverse order
        assert!(board.restore_cheese(pos2));
        assert_eq!(board.total_cheese(), 2);
        assert_eq!(board.remaining_cheese(), 1);

        assert!(board.restore_cheese(pos1));
        assert_eq!(board.total_cheese(), 2);
        assert_eq!(board.remaining_cheese(), 2);
    }

    #[test]
    fn test_cheese_board_clear() {
        let mut board = CheeseBoard::new(3, 3);
        let pos = Coordinates::new(1, 1);

        // Place and verify
        assert!(board.place_cheese(pos));
        assert_eq!(board.total_cheese(), 1);
        assert_eq!(board.remaining_cheese(), 1);

        // Clear and verify
        board.clear();
        assert_eq!(board.total_cheese(), 0);
        assert_eq!(board.remaining_cheese(), 0);
        assert!(!board.has_cheese(pos));

        // Should be able to place cheese again
        assert!(board.place_cheese(pos));
        assert_eq!(board.total_cheese(), 1);
        assert_eq!(board.remaining_cheese(), 1);
    }

    #[test]
    fn test_clear() {
        let mut board = CheeseBoard::new(3, 3);

        // Place some cheese
        board.place_cheese(Coordinates::new(0, 0));
        board.place_cheese(Coordinates::new(1, 1));
        assert_eq!(board.total_cheese(), 2);
        assert_eq!(board.remaining_cheese(), 2);

        // Clear the board
        board.clear();
        assert_eq!(board.total_cheese(), 0);
        assert_eq!(board.remaining_cheese(), 0);
        assert!(!board.has_cheese(Coordinates::new(0, 0)));
        assert!(!board.has_cheese(Coordinates::new(1, 1)));
    }
}
