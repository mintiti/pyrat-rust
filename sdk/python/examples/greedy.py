"""Greedy bot — always moves toward the nearest cheese."""

import random

from pyrat_sdk import Bot, Context, Direction, GameState


class Greedy(Bot):
    name = "Greedy"
    author = "PyRat SDK"

    def think(self, state: GameState, ctx: Context) -> Direction:
        candidates = state.nearest_cheeses()
        if candidates:
            result = random.choice(candidates)
            if result.directions:
                return result.directions[0]
        return Direction.STAY


if __name__ == "__main__":
    Greedy().run()
