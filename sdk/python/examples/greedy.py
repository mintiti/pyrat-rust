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
            if result.path:
                # Walk directions to build coordinate path
                x, y = state.my_position
                path = []
                for d in result.path:
                    if d == Direction.UP:
                        y += 1
                    elif d == Direction.DOWN:
                        y -= 1
                    elif d == Direction.RIGHT:
                        x += 1
                    elif d == Direction.LEFT:
                        x -= 1
                    path.append((x, y))
                ctx.send_info(
                    target=result.target,
                    path=path,
                    message=f"target {result.target}, {len(path)} steps",
                )
                return result.path[0]
        return Direction.STAY


if __name__ == "__main__":
    Greedy().run()
