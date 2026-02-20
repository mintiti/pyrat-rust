"""PyRat Engine - High-performance PyRat game engine with Python bindings

This package provides a fast implementation of the PyRat game engine written in Rust
with Python bindings. It features both a raw game engine interface and a
PettingZoo-compatible environment.

Example:
    >>> from pyrat_engine import GameConfig, Direction
    >>> game = GameConfig.classic(21, 15, 41).create(seed=42)
    >>> game_over, collected = game.step(Direction.RIGHT, Direction.LEFT)
    >>> print(f"Player 1 score: {game.player1_score}")
"""

from pyrat_engine.core import GameBuilder, GameConfig, MoveUndo, PyRat
from pyrat_engine.core.types import (
    Coordinates,
    Direction,
    Mud,
    Wall,
)
from pyrat_engine.game import GameResult

__version__ = "0.2.0"
__all__ = [
    "Coordinates",
    "Direction",
    "GameBuilder",
    "GameConfig",
    "GameResult",
    "MoveUndo",
    "Mud",
    "PyRat",
    "Wall",
]
