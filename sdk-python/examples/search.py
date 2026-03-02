"""Search bot — best-response tree search with iterative deepening.

Uses GameSim (make_move / unmake_move) for efficient game-tree exploration.
Both players independently maximize their own score — the opponent doesn't
try to hurt us, they try to help themselves.
"""

import random

from pyrat_sdk import Bot, Context, Direction, GameSim, GameState, Player, Spin


class Search(Bot):
    name = "Search"
    author = "PyRat SDK"
    max_depth = Spin(default=6, min=1, max=12)

    def think(self, state: GameState, ctx: Context) -> Direction:
        self._am_player1 = state.my_player == Player.PLAYER1
        self._nodes = 0
        sim = state.simulate()

        best_move = Direction.STAY
        best_score = -float("inf")

        # IDDFS: only update best_move when a depth completes fully.
        for depth in range(1, self.max_depth + 1):
            if ctx.should_stop():
                break

            result = self._search_root(sim, depth, state, ctx)
            if result is None:
                break  # timed out mid-search — keep previous best

            move, score = result
            best_move = move
            best_score = score

            ctx.send_info(
                depth=depth,
                nodes=self._nodes,
                score=best_score,
                message=f"depth {depth}: {best_move.name} ({best_score:.1f})",
            )

        return best_move

    def _search_root(
        self, sim: GameSim, depth: int, state: GameState, ctx: Context
    ) -> tuple[Direction, float] | None:
        """Find the best move at the root. Returns (direction, our_score) or None on timeout."""
        best_move = Direction.STAY
        best_score = -float("inf")

        my_pos = sim.player1_position if self._am_player1 else sim.player2_position
        opp_pos = sim.player2_position if self._am_player1 else sim.player1_position

        my_moves = state.get_effective_moves(my_pos)
        random.shuffle(my_moves)

        for my_dir in my_moves:
            if ctx.should_stop():
                return None

            # Opponent picks the move that maximizes THEIR score.
            opp_moves = state.get_effective_moves(opp_pos)
            best_opp_score = -float("inf")
            our_score_vs_opp_best = -float("inf")

            for opp_dir in opp_moves:
                p1_dir, p2_dir = self._assign_moves(my_dir, opp_dir)
                undo = sim.make_move(int(p1_dir), int(p2_dir))
                self._nodes += 1

                if depth <= 1 or sim.is_game_over:
                    our, opp = self._evaluate(sim)
                else:
                    pair = self._search(sim, depth - 1, state, ctx)
                    if pair is None:
                        sim.unmake_move(undo)
                        return None
                    our, opp = pair

                sim.unmake_move(undo)

                if opp > best_opp_score:
                    best_opp_score = opp
                    our_score_vs_opp_best = our

            if our_score_vs_opp_best > best_score:
                best_score = our_score_vs_opp_best
                best_move = my_dir

        return best_move, best_score

    def _search(
        self, sim: GameSim, depth: int, state: GameState, ctx: Context
    ) -> tuple[float, float] | None:
        """Recursive search. Returns (our_score, opp_score) or None on timeout."""
        if depth == 0 or sim.is_game_over:
            return self._evaluate(sim)

        if ctx.should_stop():
            return None

        my_pos = sim.player1_position if self._am_player1 else sim.player2_position
        opp_pos = sim.player2_position if self._am_player1 else sim.player1_position

        my_moves = state.get_effective_moves(my_pos)

        best_our = -float("inf")
        best_opp_at_our_best = 0.0

        for my_dir in my_moves:
            if ctx.should_stop():
                return None

            opp_moves = state.get_effective_moves(opp_pos)
            best_opp_score = -float("inf")
            our_when_opp_best = -float("inf")

            for opp_dir in opp_moves:
                p1_dir, p2_dir = self._assign_moves(my_dir, opp_dir)
                undo = sim.make_move(int(p1_dir), int(p2_dir))
                self._nodes += 1

                if depth <= 1 or sim.is_game_over:
                    our, opp = self._evaluate(sim)
                else:
                    pair = self._search(sim, depth - 1, state, ctx)
                    if pair is None:
                        sim.unmake_move(undo)
                        return None
                    our, opp = pair

                sim.unmake_move(undo)

                if opp > best_opp_score:
                    best_opp_score = opp
                    our_when_opp_best = our

            if our_when_opp_best > best_our:
                best_our = our_when_opp_best
                best_opp_at_our_best = best_opp_score

        return best_our, best_opp_at_our_best

    def _evaluate(self, sim: GameSim) -> tuple[float, float]:
        """Return (our_score, opponent_score) from the simulation."""
        if self._am_player1:
            return sim.player1_score, sim.player2_score
        return sim.player2_score, sim.player1_score

    def _assign_moves(
        self, my_dir: Direction, opp_dir: Direction
    ) -> tuple[Direction, Direction]:
        """Map perspective moves to objective (p1, p2) order."""
        if self._am_player1:
            return my_dir, opp_dir
        return opp_dir, my_dir


if __name__ == "__main__":
    Search().run()
