"""PyRat SDK — write ``think(state, ctx) -> Direction`` and play."""

__version__ = "0.1.0"

from pyrat_sdk._engine import GameSim, MoveUndo
from pyrat_sdk.bot import Bot, Context, GameResult, HivemindBot
from pyrat_sdk.options import Check, Combo, Spin, Str
from pyrat_sdk.state import (
    Direction,
    GameState,
    NearestCheeseResult,
    PathResult,
    Player,
)

__all__ = [
    "Bot",
    "Check",
    "Combo",
    "Context",
    "Direction",
    "GameResult",
    "GameSim",
    "GameState",
    "HivemindBot",
    "MoveUndo",
    "NearestCheeseResult",
    "PathResult",
    "Player",
    "Spin",
    "Str",
]
