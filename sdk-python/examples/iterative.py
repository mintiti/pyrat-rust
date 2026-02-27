"""Iterative bot — precomputes distances, uses time-aware search each turn."""

from pyrat_sdk import Bot, Context, Direction, GameState, Spin


class Iterative(Bot):
    name = "Iterative"
    author = "PyRat SDK"
    max_depth = Spin(default=5, min=1, max=20)

    def preprocess(self, state: GameState, ctx: Context) -> None:
        # Precompute distance table from our starting position.
        # distances_from() runs Dijkstra once — reusable across turns.
        self.distances = state.distances_from()

    def think(self, state: GameState, ctx: Context) -> Direction:
        # Recompute distances from current position each turn.
        self.distances = state.distances_from()

        best_dir = Direction.STAY
        best_cost = float("inf")

        # Search for nearest cheese, respecting time budget.
        for cx, cy in state.cheese:
            if ctx.should_stop():
                break
            cost = self.distances.get((cx, cy))
            if cost is not None and cost < best_cost:
                best_cost = cost
                result = state.shortest_path(state.my_position, (cx, cy))
                if result is not None and result.directions:
                    best_dir = result.directions[0]

        # Report search info to the GUI.
        ctx.send_info(
            nodes=len(state.cheese),
            score=best_cost if best_cost < float("inf") else 0.0,
            message=f"checked {len(state.cheese)} cheese targets",
        )

        return best_dir


if __name__ == "__main__":
    Iterative().run()
