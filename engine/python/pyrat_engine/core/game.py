"""Core game state and management classes.

This module re-exports game classes from the compiled Rust module.
"""

# Import the compiled module directly
import pyrat_engine._core as _impl

# Re-export game classes with cleaner names
GameState = _impl.game.PyGameState
MoveUndo = _impl.game.PyMoveUndo

# Keep original names for backward compatibility if needed
PyGameState = GameState
PyMoveUndo = MoveUndo

__all__ = ["GameState", "MoveUndo", "PyGameState", "PyMoveUndo"]
