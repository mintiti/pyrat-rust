"""PyRat Game Runner - CLI tool for running AI vs AI games."""

from .game_runner import GameRunner, run_game
from .move_providers import MoveProvider, SubprocessMoveProvider

__version__ = "0.1.0"

__all__ = [
    "GameRunner",
    "run_game",
    "MoveProvider",
    "SubprocessMoveProvider",
]
