//! Validation utilities for Python bindings
//!
//! This module provides validation for data coming from Python, converting
//! from Python's signed integers to Rust's unsigned types with proper error handling.

use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;
use std::collections::HashSet;

/// Python-facing position type (accepts signed integers)
pub type PyPosition = (i32, i32);
/// Python-facing wall type
pub type PyWall = (PyPosition, PyPosition);
/// Python-facing mud entry type
pub type PyMudEntry = (PyPosition, PyPosition, i32);

/// Internal validated mud entry type
type ValidatedMudEntry = ((u8, u8), (u8, u8), u8);

/// Validates and converts a signed position to unsigned
pub fn validate_position(pos: PyPosition, width: u8, height: u8, name: &str) -> PyResult<(u8, u8)> {
    let (x, y) = pos;

    // Check for negative values
    if x < 0 || y < 0 {
        return Err(PyValueError::new_err(format!(
            "{name} position ({x}, {y}) cannot be negative"
        )));
    }

    // Check for overflow (values too large for u8)
    if x > 255 || y > 255 {
        return Err(PyValueError::new_err(format!(
            "{name} position ({x}, {y}) is too large (maximum is 255)"
        )));
    }

    let x_u8 = x as u8;
    let y_u8 = y as u8;

    // Check bounds
    if x_u8 >= width || y_u8 >= height {
        return Err(PyValueError::new_err(format!(
            "{name} position ({x}, {y}) is outside maze bounds ({width}x{height})"
        )));
    }

    Ok((x_u8, y_u8))
}

/// Validates and converts a wall with signed positions
pub fn validate_wall(wall: PyWall, width: u8, height: u8) -> PyResult<crate::Wall> {
    let (pos1, pos2) = wall;

    // Validate both positions
    let validated_pos1 = validate_position(pos1, width, height, "Wall start")?;
    let validated_pos2 = validate_position(pos2, width, height, "Wall end")?;

    // Check adjacency
    if !are_adjacent(validated_pos1, validated_pos2) {
        return Err(PyValueError::new_err(format!(
            "Wall between {pos1:?} and {pos2:?} must be between adjacent cells"
        )));
    }

    // Create Wall struct with normalized order
    let coord1 = crate::Coordinates::new(validated_pos1.0, validated_pos1.1);
    let coord2 = crate::Coordinates::new(validated_pos2.0, validated_pos2.1);

    // Normalize order (smaller position first)
    let (wall_pos1, wall_pos2) = if coord1 < coord2 {
        (coord1, coord2)
    } else {
        (coord2, coord1)
    };

    Ok(crate::Wall {
        pos1: wall_pos1,
        pos2: wall_pos2,
    })
}

/// Validates and converts a mud entry with signed values
pub fn validate_mud(mud: PyMudEntry, width: u8, height: u8) -> PyResult<ValidatedMudEntry> {
    let (pos1, pos2, value) = mud;

    // Validate positions
    let validated_pos1 = validate_position(pos1, width, height, "Mud start")?;
    let validated_pos2 = validate_position(pos2, width, height, "Mud end")?;

    // Check adjacency
    if !are_adjacent(validated_pos1, validated_pos2) {
        return Err(PyValueError::new_err(format!(
            "Mud between {pos1:?} and {pos2:?} must be between adjacent cells"
        )));
    }

    // Validate mud value
    if value < 0 {
        return Err(PyValueError::new_err(format!(
            "Mud value {value} cannot be negative"
        )));
    }

    if value < 2 {
        return Err(PyValueError::new_err(
            "Mud value must be at least 2 turns (1 represents normal passage)",
        ));
    }

    if value > 255 {
        return Err(PyValueError::new_err(format!(
            "Mud value {value} is too large (maximum is 255)"
        )));
    }

    Ok((validated_pos1, validated_pos2, value as u8))
}

/// Validates a list of cheese positions
pub fn validate_cheese_positions(
    positions: Vec<PyPosition>,
    width: u8,
    height: u8,
) -> PyResult<Vec<(u8, u8)>> {
    let mut validated = Vec::new();
    let mut seen = std::collections::HashSet::new();

    for pos in positions {
        let validated_pos = validate_position(pos, width, height, "Cheese")?;

        // Check for duplicates
        if !seen.insert(validated_pos) {
            return Err(PyValueError::new_err(format!(
                "Duplicate cheese position at {pos:?}"
            )));
        }

        validated.push(validated_pos);
    }

    Ok(validated)
}

/// Validates an optional position
pub fn validate_optional_position(
    pos: Option<PyPosition>,
    width: u8,
    height: u8,
    name: &str,
) -> PyResult<Option<(u8, u8)>> {
    match pos {
        Some(p) => Ok(Some(validate_position(p, width, height, name)?)),
        None => Ok(None),
    }
}

/// Helper function to check if two positions are adjacent
fn are_adjacent(pos1: (u8, u8), pos2: (u8, u8)) -> bool {
    let dx = pos1.0.abs_diff(pos2.0);
    let dy = pos1.1.abs_diff(pos2.1);
    (dx == 1 && dy == 0) || (dx == 0 && dy == 1)
}

// =============================================================================
// Symmetry Validation
// =============================================================================

/// Get the 180° rotationally symmetric position
#[inline]
pub fn get_symmetric(x: u8, y: u8, width: u8, height: u8) -> (u8, u8) {
    (width - 1 - x, height - 1 - y)
}

/// Validate that walls are symmetric (each wall has a corresponding symmetric wall)
pub fn validate_walls_symmetric(
    walls: &[crate::Wall],
    width: u8,
    height: u8,
) -> Result<(), String> {
    let mut wall_set: HashSet<((u8, u8), (u8, u8))> = HashSet::new();

    // Build set of all walls (normalized: smaller position first)
    for wall in walls {
        let (p1, p2) = if wall.pos1 < wall.pos2 {
            (wall.pos1, wall.pos2)
        } else {
            (wall.pos2, wall.pos1)
        };
        wall_set.insert(((p1.x, p1.y), (p2.x, p2.y)));
    }

    // Check each wall has its symmetric counterpart
    for wall in walls {
        let sym1 = get_symmetric(wall.pos1.x, wall.pos1.y, width, height);
        let sym2 = get_symmetric(wall.pos2.x, wall.pos2.y, width, height);

        // Normalize symmetric wall (smaller position first)
        let (sym_p1, sym_p2) = if sym1 < sym2 {
            (sym1, sym2)
        } else {
            (sym2, sym1)
        };

        // Self-symmetric walls are valid (wall equals its own symmetric)
        let orig_normalized = if wall.pos1 < wall.pos2 {
            ((wall.pos1.x, wall.pos1.y), (wall.pos2.x, wall.pos2.y))
        } else {
            ((wall.pos2.x, wall.pos2.y), (wall.pos1.x, wall.pos1.y))
        };

        if (sym_p1, sym_p2) == orig_normalized {
            continue; // Self-symmetric wall, valid
        }

        if !wall_set.contains(&(sym_p1, sym_p2)) {
            return Err(format!(
                "Wall ({}, {})-({}, {}) has no symmetric counterpart at ({}, {})-({}, {})",
                wall.pos1.x,
                wall.pos1.y,
                wall.pos2.x,
                wall.pos2.y,
                sym_p1.0,
                sym_p1.1,
                sym_p2.0,
                sym_p2.1
            ));
        }
    }

    Ok(())
}

/// Key type for mud map: ((x1, y1), (x2, y2))
type MudKey = ((u8, u8), (u8, u8));

/// Validate that mud entries are symmetric (each mud has a corresponding symmetric mud with same value)
pub fn validate_mud_symmetric(mud: &[crate::Mud], width: u8, height: u8) -> Result<(), String> {
    let mut mud_map: std::collections::HashMap<MudKey, u8> = std::collections::HashMap::new();

    // Build map of all mud entries (normalized: smaller position first)
    for m in mud {
        let (p1, p2) = if m.pos1 < m.pos2 {
            (m.pos1, m.pos2)
        } else {
            (m.pos2, m.pos1)
        };
        mud_map.insert(((p1.x, p1.y), (p2.x, p2.y)), m.value);
    }

    // Check each mud has its symmetric counterpart with same value
    for m in mud {
        let sym1 = get_symmetric(m.pos1.x, m.pos1.y, width, height);
        let sym2 = get_symmetric(m.pos2.x, m.pos2.y, width, height);

        // Normalize symmetric mud (smaller position first)
        let (sym_p1, sym_p2) = if sym1 < sym2 {
            (sym1, sym2)
        } else {
            (sym2, sym1)
        };

        // Self-symmetric mud is valid
        let orig_normalized = if m.pos1 < m.pos2 {
            ((m.pos1.x, m.pos1.y), (m.pos2.x, m.pos2.y))
        } else {
            ((m.pos2.x, m.pos2.y), (m.pos1.x, m.pos1.y))
        };

        if (sym_p1, sym_p2) == orig_normalized {
            continue; // Self-symmetric mud, valid
        }

        match mud_map.get(&(sym_p1, sym_p2)) {
            None => {
                return Err(format!(
                    "Mud ({}, {})-({}, {}) has no symmetric counterpart at ({}, {})-({}, {})",
                    m.pos1.x, m.pos1.y, m.pos2.x, m.pos2.y, sym_p1.0, sym_p1.1, sym_p2.0, sym_p2.1
                ));
            },
            Some(&sym_value) if sym_value != m.value => {
                return Err(format!(
                    "Mud ({}, {})-({}, {}) has value {} but symmetric mud has value {}",
                    m.pos1.x, m.pos1.y, m.pos2.x, m.pos2.y, m.value, sym_value
                ));
            },
            _ => {}, // Valid
        }
    }

    Ok(())
}

/// Validate that cheese positions are symmetric
pub fn validate_cheese_symmetric(
    cheese: &[crate::Coordinates],
    width: u8,
    height: u8,
) -> Result<(), String> {
    let cheese_set: HashSet<(u8, u8)> = cheese.iter().map(|c| (c.x, c.y)).collect();

    for c in cheese {
        let (sym_x, sym_y) = get_symmetric(c.x, c.y, width, height);

        // Self-symmetric position (center of odd-dimension maze) is valid alone
        if (sym_x, sym_y) == (c.x, c.y) {
            continue;
        }

        if !cheese_set.contains(&(sym_x, sym_y)) {
            return Err(format!(
                "Cheese at ({}, {}) has no symmetric counterpart at ({}, {})",
                c.x, c.y, sym_x, sym_y
            ));
        }
    }

    Ok(())
}

/// Validate that player positions are symmetric (p1 should be symmetric to p2)
pub fn validate_players_symmetric(
    p1: crate::Coordinates,
    p2: crate::Coordinates,
    width: u8,
    height: u8,
) -> Result<(), String> {
    let (sym_x, sym_y) = get_symmetric(p1.x, p1.y, width, height);

    if (sym_x, sym_y) != (p2.x, p2.y) {
        return Err(format!(
            "Player positions are not symmetric: P1 at ({}, {}), P2 at ({}, {}). \
             P2 should be at ({}, {}) for symmetry",
            p1.x, p1.y, p2.x, p2.y, sym_x, sym_y
        ));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validate_position_negative() {
        let result = validate_position((-1, 0), 10, 10, "Test");
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.to_string().contains("cannot be negative"));
    }

    #[test]
    fn test_validate_position_too_large() {
        let result = validate_position((256, 0), 10, 10, "Test");
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.to_string().contains("too large"));
    }

    #[test]
    fn test_validate_position_out_of_bounds() {
        let result = validate_position((10, 10), 10, 10, "Test");
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.to_string().contains("outside maze bounds"));
    }

    #[test]
    fn test_validate_position_valid() {
        let result = validate_position((5, 5), 10, 10, "Test");
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), (5, 5));
    }

    #[test]
    fn test_validate_mud_negative_value() {
        let result = validate_mud(((0, 0), (0, 1), -1), 10, 10);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.to_string().contains("cannot be negative"));
    }

    #[test]
    fn test_validate_mud_value_too_small() {
        let result = validate_mud(((0, 0), (0, 1), 1), 10, 10);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.to_string().contains("at least 2 turns"));
    }
}
