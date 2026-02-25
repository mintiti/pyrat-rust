"""Greedy bot — always moves toward the nearest cheese."""

from pyrat_sdk import Bot, Context, Direction
from pyrat_sdk.state import GameState


class Greedy(Bot):
    name = "Greedy"
    author = "PyRat SDK"

    def think(self, state: GameState, ctx: Context) -> Direction:
        result = state.nearest_cheese()
        if result is not None:
            _, path, _ = result
            if path:
                return Direction(path[0])
        return Direction.STAY


if __name__ == "__main__":
    Greedy().run()
