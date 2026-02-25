"""Smart random bot — picks a random non-wall direction each turn."""

import random

from pyrat_sdk import Bot, Context, Direction
from pyrat_sdk.state import GameState


class SmartRandom(Bot):
    name = "SmartRandom"
    author = "PyRat SDK"

    def think(self, state: GameState, ctx: Context) -> Direction:
        moves = state.get_effective_moves()
        if moves:
            return Direction(random.choice(moves))
        return Direction.STAY


if __name__ == "__main__":
    SmartRandom().run()
