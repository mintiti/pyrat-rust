"""Smart random bot — picks a random non-wall direction each turn."""

import random

from pyrat_sdk import Bot, Context, Direction, GameState


class SmartRandom(Bot):
    name = "SmartRandom"
    author = "PyRat SDK"

    def think(self, state: GameState, ctx: Context) -> Direction:
        # get_effective_moves() returns Direction values for non-wall moves
        # from the current position (UP, RIGHT, DOWN, LEFT — never STAY).
        moves = state.get_effective_moves()
        if moves:
            return random.choice(moves)
        return Direction.STAY


if __name__ == "__main__":
    SmartRandom().run()
