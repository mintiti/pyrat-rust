use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug, Deserialize, Serialize)]
pub struct Coordinates {
    pub x: u8,
    pub y: u8,
}

impl Coordinates {
    #[must_use]
    #[inline(always)]
    pub const fn new(x: u8, y: u8) -> Self {
        Self { x, y }
    }

    #[must_use]
    #[inline(always)]
    pub const fn to_index(&self, width: u8) -> usize {
        (self.y as usize) * (width as usize) + (self.x as usize)
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(u8)]
pub enum Direction {
    Up = 0,
    Right = 1,
    Down = 2,
    Left = 3,
    Stay = 4, // Special case, not stored in move table
}

impl Direction {
    /// Apply move in the mathematical coordinate system where:
    /// - x increases to the right
    /// - y increases going up
    /// - (0,0) is at the bottom-left corner
    #[inline(always)]
    pub const fn apply_to(&self, pos: Coordinates) -> Coordinates {
        match self {
            Self::Up => Coordinates {
                x: pos.x,
                y: pos.y.saturating_add(1), // Up means increasing y
            },
            Self::Down => Coordinates {
                x: pos.x,
                y: pos.y.saturating_sub(1), // Down means decreasing y
            },
            Self::Left => Coordinates {
                x: pos.x.saturating_sub(1),
                y: pos.y,
            },
            Self::Right => Coordinates {
                x: pos.x.saturating_add(1),
                y: pos.y,
            },
            Self::Stay => pos,
        }
    }
}

impl TryFrom<u8> for Direction {
    type Error = &'static str;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(Self::Up),
            1 => Ok(Self::Right),
            2 => Ok(Self::Down),
            3 => Ok(Self::Left),
            4 => Ok(Self::Stay),
            _ => Err("Invalid direction value"),
        }
    }
}

/// A wrapper around HashMap that handles bidirectional mud lookups
#[derive(Clone, Default)]
pub struct MudMap {
    inner: HashMap<(Coordinates, Coordinates), u8>,
}

impl MudMap {
    pub fn new() -> Self {
        Self {
            inner: HashMap::new(),
        }
    }

    /// Insert mud between two positions (order doesn't matter)
    pub fn insert(&mut self, pos1: Coordinates, pos2: Coordinates, value: u8) {
        self.inner.insert((pos1, pos2), value);
        self.inner.insert((pos2, pos1), value);
    }

    /// Get mud value between two positions (order doesn't matter)
    pub fn get(&self, pos1: Coordinates, pos2: Coordinates) -> Option<u8> {
        self.inner
            .get(&(pos1, pos2))
            .or_else(|| self.inner.get(&(pos2, pos1)))
            .copied()
    }

    /// Returns an iterator over all unique mud positions and their values
    pub fn iter(&self) -> impl Iterator<Item = ((Coordinates, Coordinates), u8)> + '_ {
        self.inner
            .iter()
            .filter(|((pos1, pos2), _)| pos1 < pos2) // Only return one direction
            .map(|((pos1, pos2), &value)| ((*pos1, *pos2), value))
    }

    /// Clear all mud
    pub fn clear(&mut self) {
        self.inner.clear();
    }

    /// Returns the number of unique mud positions
    pub fn len(&self) -> usize {
        self.inner.len() / 2
    }

    pub fn is_empty(&self) -> bool {
        self.inner.is_empty()
    }
}

impl std::ops::Deref for MudMap {
    type Target = HashMap<(Coordinates, Coordinates), u8>;

    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mud_map() {
        let mut mud_map = MudMap::new();
        let pos1 = Coordinates::new(0, 0);
        let pos2 = Coordinates::new(0, 1);

        mud_map.insert(pos1, pos2, 2);

        // Test bidirectional lookup
        assert_eq!(mud_map.get(pos1, pos2), Some(2));
        assert_eq!(mud_map.get(pos2, pos1), Some(2));

        // Test non-existent mud
        assert_eq!(mud_map.get(pos1, Coordinates::new(1, 0)), None);
    }
    mod coordinates {
        use super::*;

        #[test]
        fn test_new_coordinates() {
            let coord = Coordinates::new(5, 10);
            assert_eq!(coord.x, 5);
            assert_eq!(coord.y, 10);
        }

        #[test]
        fn test_to_index() {
            let test_cases = [
                // (x, y, width, expected_index)
                (0, 0, 10, 0),  // Top-left corner
                (9, 0, 10, 9),  // Top-right corner
                (0, 9, 10, 90), // Bottom-left corner
                (9, 9, 10, 99), // Bottom-right corner
                (5, 5, 10, 55), // Middle
                (3, 2, 15, 33), // Non-square board
                (0, 1, 5, 5),   // Second row start
                (4, 1, 5, 9),   // Second row end
            ];

            for (x, y, width, expected) in test_cases {
                let coord = Coordinates::new(x, y);
                assert_eq!(
                    coord.to_index(width),
                    expected,
                    "Failed for x={}, y={}, width={}",
                    x,
                    y,
                    width
                );
            }
        }

        #[test]
        fn test_coordinates_equality() {
            let coord1 = Coordinates::new(1, 2);
            let coord2 = Coordinates::new(1, 2);
            let coord3 = Coordinates::new(2, 1);

            assert_eq!(coord1, coord2);
            assert_ne!(coord1, coord3);
        }

        #[test]
        fn test_coordinates_clone() {
            let coord1 = Coordinates::new(1, 2);
            let coord2 = coord1;

            assert_eq!(coord1, coord2);
            // Ensure modifying one doesn't affect the other
            let coord3 = Coordinates::new(coord2.x + 1, coord2.y);
            assert_ne!(coord1, coord3);
        }
    }

    mod direction {
        use super::*;

        #[test]
        fn test_direction_apply_to() {
            let center = Coordinates::new(5, 5);

            // Test all directions from center with mathematical coordinate system
            assert_eq!(
                Direction::Up.apply_to(center),
                Coordinates::new(5, 6), // Moving up increases y
                "Up should increase y coordinate"
            );
            assert_eq!(
                Direction::Down.apply_to(center),
                Coordinates::new(5, 4), // Moving down decreases y
                "Down should decrease y coordinate"
            );
            assert_eq!(
                Direction::Left.apply_to(center),
                Coordinates::new(4, 5),
                "Left should decrease x coordinate"
            );
            assert_eq!(
                Direction::Right.apply_to(center),
                Coordinates::new(6, 5),
                "Right should increase x coordinate"
            );
            assert_eq!(Direction::Stay.apply_to(center), center);
        }

        #[test]
        fn test_coordinate_system_edges() {
            // Test bottom edge (y = 0)
            let bottom = Coordinates::new(5, 0);
            assert_eq!(
                Direction::Down.apply_to(bottom),
                Coordinates::new(5, 0),
                "Down at bottom edge should saturate"
            );
            assert_eq!(
                Direction::Up.apply_to(bottom),
                Coordinates::new(5, 1),
                "Up from bottom should increase y"
            );

            // Test top edge (y = 255)
            let top = Coordinates::new(5, 255);
            assert_eq!(
                Direction::Up.apply_to(top),
                Coordinates::new(5, 255),
                "Up at top edge should saturate"
            );
            assert_eq!(
                Direction::Down.apply_to(top),
                Coordinates::new(5, 254),
                "Down from top should decrease y"
            );

            // Test origin behavior
            let origin = Coordinates::new(0, 0); // Bottom-left corner
            assert_eq!(
                Direction::Down.apply_to(origin),
                Coordinates::new(0, 0),
                "Down from origin should stay at origin"
            );
            assert_eq!(
                Direction::Up.apply_to(origin),
                Coordinates::new(0, 1),
                "Up from origin should increase y"
            );
        }

        #[test]
        fn test_initial_positions() {
            // Test movements from player starting positions
            let player1_start = Coordinates::new(0, 9); // Top-right in a 10x10 grid
            let player2_start = Coordinates::new(9, 0); // Bottom-left in a 10x10 grid

            assert_eq!(
                Direction::Down.apply_to(player1_start),
                Coordinates::new(0, 8),
                "Player1 moving down should decrease y"
            );
            assert_eq!(
                Direction::Right.apply_to(player2_start),
                Coordinates::new(10, 0),
                "Player2 moving right should increase x"
            );
        }

        #[test]
        fn test_saturating_behavior() {
            // Tests that the positions get saturated correctly
            let bottom_left = Coordinates::new(0, 0);
            let upper_right = Coordinates::new(255, 255);

            assert_eq!(Direction::Up.apply_to(upper_right), upper_right);
            assert_eq!(Direction::Right.apply_to(upper_right), upper_right);
            assert_eq!(Direction::Left.apply_to(bottom_left), bottom_left);
            assert_eq!(Direction::Down.apply_to(bottom_left), bottom_left);
        }

        #[test]
        fn test_direction_ordering() {
            assert_eq!(Direction::Up as u8, 0);
            assert_eq!(Direction::Right as u8, 1);
            assert_eq!(Direction::Down as u8, 2);
            assert_eq!(Direction::Left as u8, 3);
            assert_eq!(Direction::Stay as u8, 4);
        }

        #[test]
        fn test_direction_equality() {
            assert_eq!(Direction::Up, Direction::Up);
            assert_ne!(Direction::Up, Direction::Down);
            assert_ne!(Direction::Left, Direction::Right);
            assert_eq!(Direction::Stay, Direction::Stay);
        }

        #[test]
        fn test_all_directions_center() {
            let center = Coordinates::new(10, 10);
            let moves = [
                (Direction::Up, Coordinates::new(10, 11)),
                (Direction::Right, Coordinates::new(11, 10)),
                (Direction::Down, Coordinates::new(10, 9)),
                (Direction::Left, Coordinates::new(9, 10)),
                (Direction::Stay, center),
            ];

            for (direction, expected) in moves {
                assert_eq!(
                    direction.apply_to(center),
                    expected,
                    "Failed for direction {:?}",
                    direction
                );
            }
        }
    }
}
