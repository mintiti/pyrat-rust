"""Core types for the PyRat game engine.

This module re-exports types from the compiled Rust module and provides
utility functions for working with directions.
"""

from typing import Optional

# Import the compiled module directly
import pyrat_engine._core as _impl

# Re-export all type classes
Coordinates = _impl.types.Coordinates
Direction = _impl.types.Direction
Wall = _impl.types.Wall
Mud = _impl.types.Mud

# Direction name mapping (canonical source of truth)
_DIRECTION_TO_NAME = {
    0: "UP",
    1: "RIGHT",
    2: "DOWN",
    3: "LEFT",
    4: "STAY",
}

_NAME_TO_DIRECTION = {
    "UP": Direction.UP,
    "RIGHT": Direction.RIGHT,
    "DOWN": Direction.DOWN,
    "LEFT": Direction.LEFT,
    "STAY": Direction.STAY,
}


def direction_to_name(direction: int) -> str:
    """Convert a Direction value to its string name.

    Args:
        direction: Direction value (Direction.UP, Direction.DOWN, etc.)

    Returns:
        String name of the direction ("UP", "DOWN", "LEFT", "RIGHT", "STAY")
        Returns "STAY" for invalid direction values.

    Example:
        >>> direction_to_name(Direction.UP)
        'UP'
        >>> direction_to_name(Direction.STAY)
        'STAY'
    """
    return _DIRECTION_TO_NAME.get(int(direction), "STAY")


def name_to_direction(name: str) -> int:
    """Convert a direction name string to a Direction value.

    Args:
        name: Direction name string (case-sensitive uppercase: "UP", "DOWN", etc.)

    Returns:
        Direction value (Direction.UP, Direction.DOWN, etc.)
        Returns Direction.STAY for invalid names.

    Example:
        >>> name_to_direction("UP")
        0  # Direction.UP
        >>> name_to_direction("invalid")
        4  # Direction.STAY
    """
    return _NAME_TO_DIRECTION.get(name, Direction.STAY)


def is_valid_direction(direction: Optional[int]) -> bool:
    """Check if a direction value is valid.

    Args:
        direction: Direction value to validate

    Returns:
        True if the direction is valid (UP, DOWN, LEFT, RIGHT, or STAY),
        False otherwise.

    Example:
        >>> is_valid_direction(Direction.UP)
        True
        >>> is_valid_direction(999)
        False
        >>> is_valid_direction(None)
        False
    """
    if direction is None:
        return False
    try:
        return int(direction) in _DIRECTION_TO_NAME
    except (ValueError, TypeError):
        return False


__all__ = [
    "Coordinates",
    "Direction",
    "Mud",
    "Wall",
    "direction_to_name",
    "is_valid_direction",
    "name_to_direction",
]
