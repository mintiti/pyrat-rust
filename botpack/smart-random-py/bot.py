"""Smart Random bot: picks a random valid direction each turn.

Baseline opponent. Won't win, but won't walk into walls either.

SDK features: effective_moves, send_info.
"""

import random

from pyrat_sdk import Bot, Context, Direction, GameState


class SmartRandom(Bot):
    name = "SmartRandom"
    author = "PyRat SDK"

    def think(self, state: GameState, ctx: Context) -> Direction:
        moves = state.effective_moves()
        if moves:
            chosen = random.choice(moves)
            ctx.send_info(
                player=state.my_player,
                pv=[chosen],
                message=chosen.name,
            )
            return chosen
        return Direction.STAY


if __name__ == "__main__":
    SmartRandom().run()
