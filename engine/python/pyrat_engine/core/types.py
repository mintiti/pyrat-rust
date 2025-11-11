"""Core types for the PyRat game engine.

This module re-exports types from the compiled Rust module.
"""

# Import the compiled module directly
import pyrat_engine._core as _impl

# Re-export all type classes
Coordinates = _impl.types.Coordinates
Direction = _impl.types.Direction
Wall = _impl.types.Wall
Mud = _impl.types.Mud

__all__ = ["Coordinates", "Direction", "Wall", "Mud"]
