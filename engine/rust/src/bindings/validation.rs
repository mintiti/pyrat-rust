//! Validation utilities for Python bindings
//!
//! This module provides validation for data coming from Python, converting
//! from Python's signed integers to Rust's unsigned types with proper error handling.

use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;

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
pub fn validate_wall(wall: PyWall, width: u8, height: u8) -> PyResult<((u8, u8), (u8, u8))> {
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

    Ok((validated_pos1, validated_pos2))
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
