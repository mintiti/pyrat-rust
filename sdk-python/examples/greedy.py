"""Greedy bot — always moves toward the nearest cheese."""

from pyrat_sdk import Bot, Context, Direction, GameState


class Greedy(Bot):
    name = "Greedy"
    author = "PyRat SDK"

    def think(self, state: GameState, ctx: Context) -> Direction:
        # nearest_cheese() returns NearestCheeseResult(position, directions, cost)
        # or None if no cheese remains.
        result = state.nearest_cheese()
        if result is not None and result.directions:
            # directions[0] is the first step of the shortest path.
            return result.directions[0]
        return Direction.STAY


if __name__ == "__main__":
    Greedy().run()
