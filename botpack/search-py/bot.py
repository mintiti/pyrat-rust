"""Search bot: simultaneous-game tree search with iterative deepening and PV ordering.

Both players independently maximize their own score. Since the game is constant-sum
(cheese collected by one player is unavailable to the other), this is equivalent to
minimax: maximizing our score and maximizing opponent's score produces the same
equilibrium as maximizing ours while minimizing theirs.

Iterative deepening with no depth cap. The search cooperates with should_stop() and
deepens until time runs out. Tied PVs (principal variations) from the previous depth
guide move ordering at every level of the tree: preferred moves are explored first,
giving deeper searches better chances of finding strong lines early.

SDK features: GameSim, effective_moves, should_stop, send_info.
"""

import itertools
import random

from pyrat_sdk import Bot, Context, Direction, GameSim, GameState, Player


def order_moves(
    moves: list[Direction], pv_suffixes: list[list[Direction]]
) -> list[Direction]:
    """Order moves: PV-preferred first (shuffled), then rest (shuffled)."""
    if not pv_suffixes:
        shuffled = list(moves)
        random.shuffle(shuffled)
        return shuffled

    pv_firsts: set[Direction] = set()
    for suffix in pv_suffixes:
        if suffix:
            pv_firsts.add(suffix[0])

    preferred = [d for d in moves if d in pv_firsts]
    rest = [d for d in moves if d not in pv_firsts]
    random.shuffle(preferred)
    random.shuffle(rest)
    return preferred + rest


class Search(Bot):
    name = "Search.py"
    author = "mintiti"

    def think(self, state: GameState, ctx: Context) -> Direction:
        self._am_player1 = state.my_player == Player.PLAYER1
        self._nodes = 0
        sim = state.to_sim()

        best_move = Direction.STAY
        best_score = -float("inf")
        pvs: list[list[Direction]] = []

        for depth in itertools.count(1):
            if ctx.should_stop():
                break

            move, score, new_pvs = self._search_root(sim, depth, state, ctx, pvs)

            if score > best_score:
                best_move = move
                best_score = score

            pvs = new_pvs

        return best_move

    def _search_root(
        self,
        sim: GameSim,
        depth: int,
        state: GameState,
        ctx: Context,
        pvs: list[list[Direction]],
    ) -> tuple[Direction, float, list[list[Direction]]]:
        """Find the best move at the root. Always returns a result (may be partial)."""
        best_move = Direction.STAY
        best_score = -float("inf")
        tied_pvs: list[list[Direction]] = []

        my_pos = sim.player1_position if self._am_player1 else sim.player2_position
        opp_pos = sim.player2_position if self._am_player1 else sim.player1_position

        my_moves = order_moves(state.effective_moves(my_pos), pvs)
        opp_moves = state.effective_moves(opp_pos)

        for my_dir in my_moves:
            if ctx.should_stop():
                break

            child_suffixes = (
                [pv[1:] for pv in pvs if pv and pv[0] == my_dir] if pvs else []
            )

            result = self._opponent_best_response(
                sim, my_dir, opp_moves, depth, state, ctx, child_suffixes
            )
            if result is None:
                break
            our_score_vs_opp_best, _, pv_vs_opp_best = result

            pv = [my_dir, *pv_vs_opp_best]

            if our_score_vs_opp_best > best_score:
                best_score = our_score_vs_opp_best
                best_move = my_dir
                ctx.send_info(
                    player=state.my_player,
                    multipv=1,
                    depth=depth,
                    nodes=self._nodes,
                    score=best_score,
                    pv=pv,
                    message=f"depth {depth}: {best_move.name} ({best_score:.1f})",
                )
                tied_pvs = [pv]
            elif our_score_vs_opp_best == best_score:
                tied_pvs.append(pv)
                ctx.send_info(
                    player=state.my_player,
                    multipv=len(tied_pvs),
                    depth=depth,
                    nodes=self._nodes,
                    score=best_score,
                    pv=pv,
                    message=f"depth {depth}: {pv[0].name} ({best_score:.1f}) [pv {len(tied_pvs)}]",
                )

        return best_move, best_score, tied_pvs

    def _search(
        self,
        sim: GameSim,
        depth: int,
        state: GameState,
        ctx: Context,
        pv_suffixes: list[list[Direction]],
    ) -> tuple[float, float, list[Direction]] | None:
        """Recursive search. Returns (our_score, opp_score, pv) or None on timeout."""
        if depth == 0 or sim.is_game_over:
            return *self._evaluate(sim), []

        if ctx.should_stop():
            return None

        my_pos = sim.player1_position if self._am_player1 else sim.player2_position
        opp_pos = sim.player2_position if self._am_player1 else sim.player1_position

        my_moves = order_moves(state.effective_moves(my_pos), pv_suffixes)
        opp_moves = state.effective_moves(opp_pos)

        best_our = -float("inf")
        best_opp_at_our_best = 0.0
        best_pv: list[Direction] = []

        for my_dir in my_moves:
            if ctx.should_stop():
                return None

            child_suffixes = (
                [s[1:] for s in pv_suffixes if s and s[0] == my_dir]
                if pv_suffixes
                else []
            )

            result = self._opponent_best_response(
                sim, my_dir, opp_moves, depth, state, ctx, child_suffixes
            )
            if result is None:
                return None
            our_when_opp_best, best_opp_score, pv_when_opp_best = result

            if our_when_opp_best > best_our:
                best_our = our_when_opp_best
                best_opp_at_our_best = best_opp_score
                best_pv = [my_dir, *pv_when_opp_best]

        return best_our, best_opp_at_our_best, best_pv

    def _opponent_best_response(
        self,
        sim: GameSim,
        my_dir: Direction,
        opp_moves: list[Direction],
        depth: int,
        state: GameState,
        ctx: Context,
        child_suffixes: list[list[Direction]],
    ) -> tuple[float, float, list[Direction]] | None:
        """Find the opponent's best response to our move. Returns None on timeout."""
        best_opp_score = -float("inf")
        our_when_opp_best = -float("inf")
        pv_when_opp_best: list[Direction] = []

        for opp_dir in opp_moves:
            p1_dir, p2_dir = self._assign_moves(my_dir, opp_dir)
            undo = sim.make_move(int(p1_dir), int(p2_dir))
            self._nodes += 1

            if depth <= 1 or sim.is_game_over:
                our, opp = self._evaluate(sim)
                child_pv: list[Direction] = []
            else:
                result = self._search(sim, depth - 1, state, ctx, child_suffixes)
                if result is None:
                    sim.unmake_move(undo)
                    return None
                our, opp, child_pv = result

            sim.unmake_move(undo)

            if opp > best_opp_score:
                best_opp_score = opp
                our_when_opp_best = our
                pv_when_opp_best = child_pv

        return our_when_opp_best, best_opp_score, pv_when_opp_best

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
