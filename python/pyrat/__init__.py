"""PyRat - High-performance PyRat game environment

This package provides a fast implementation of the PyRat game engine written in Rust
with Python bindings. It features both a raw game engine interface and a
PettingZoo-compatible environment.

Key Features:
    - High-performance Rust core engine
    - PettingZoo Parallel environment interface
    - Support for move undo/redo
    - Symmetric and asymmetric maze generation
    - Customizable game parameters

Example:
    >>> from pyrat import PyRat, Direction
    >>> game = PyRat(width=15, height=15)
    >>> # Make moves
    >>> result = game.step(Direction.RIGHT, Direction.LEFT)
    >>> # Check game state
    >>> print(f"Player 1 score: {game.scores[0]}")
"""

from pyrat.game import Direction, GameResult, Position, PyRat

__version__ = "0.1.0"
__all__ = ["PyRat", "Direction", "Position", "GameResult"]
