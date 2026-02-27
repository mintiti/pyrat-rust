"""PyRat SDK — write ``think(state, ctx) -> Direction`` and play."""

__version__ = "0.1.0"

from pyrat_sdk.bot import Bot, Context, HivemindBot
from pyrat_sdk.state import (
    Direction,
    GameState,
    NearestCheeseResult,
    PathResult,
    Player,
)

__all__ = [
    "Bot",
    "Context",
    "Direction",
    "GameState",
    "HivemindBot",
    "NearestCheeseResult",
    "PathResult",
    "Player",
]
