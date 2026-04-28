"""Tests for GameState — init, advance, snapshot loads, perspective, convenience."""

from __future__ import annotations

import numpy as np

from pyrat_sdk.state import (
    Direction,
    GameState,
    PathResult,
    Player,
    _build_cheese_matrix,
)


def _make_config(**overrides) -> dict:
    """Minimal MatchConfig dict for GameState.__init__."""
    config = {
        "width": 3,
        "height": 3,
        "max_turns": 10,
        "walls": [],
        "mud": [],
        "cheese": [(1, 1)],
        "player1_start": (0, 0),
        "player2_start": (2, 2),
        "controlled_players": [],
        "timing": 0,
        "move_timeout_ms": 1000,
        "preprocessing_timeout_ms": 1000,
    }
    config.update(overrides)
    return config


def _make_turn_state(**overrides) -> dict:
    """TurnState dict (no state_hash — that lives at the parent message)."""
    ts = {
        "turn": 1,
        "player1_position": (1, 0),
        "player2_position": (2, 1),
        "player1_score": 1.0,
        "player2_score": 0.5,
        "player1_mud_turns": 2,
        "player2_mud_turns": 0,
        "cheese": [(1, 1)],
        "player1_last_move": 0,  # UP
        "player2_last_move": 1,  # RIGHT
    }
    ts.update(overrides)
    return ts


# ══════════════════════════════════════════════════════════
# 1. Construction
# ══════════════════════════════════════════════════════════


class TestInit:
    def test_basic_fields(self):
        state = GameState(0, _make_config())
        assert state.width == 3
        assert state.height == 3
        assert state.max_turns == 10
        assert state.turn == 0
        assert state.move_timeout_ms == 1000
        assert state.preprocessing_timeout_ms == 1000
        assert state.my_player == Player.PLAYER1

    def test_initial_positions(self):
        state = GameState(0, _make_config())
        assert state.player1_position == (0, 0)
        assert state.player2_position == (2, 2)

    def test_initial_scores_zero(self):
        state = GameState(0, _make_config())
        assert state.player1_score == 0.0
        assert state.player2_score == 0.0

    def test_state_hash_is_nonzero(self):
        # Engine Zobrist combines maze + positions + cheese + turn — nonzero.
        state = GameState(0, _make_config())
        assert state.state_hash != 0

    def test_cheese_matrix_shape_and_values(self):
        state = GameState(0, _make_config(cheese=[(0, 0), (2, 1)]))
        assert state.cheese_matrix.shape == (3, 3)
        assert state.cheese_matrix.dtype == np.uint8
        assert state.cheese_matrix[0, 0] == 1
        assert state.cheese_matrix[2, 1] == 1
        assert state.cheese_matrix[1, 1] == 0

    def test_movement_matrix_shape(self):
        state = GameState(0, _make_config())
        assert state.movement_matrix.shape == (3, 3, 4)


# ══════════════════════════════════════════════════════════
# 2. apply_advance — incremental engine step
# ══════════════════════════════════════════════════════════


class TestApplyAdvance:
    def test_position_changes(self):
        state = GameState(0, _make_config(cheese=[]))
        # Player1 RIGHT (toward (1,0)), player2 STAY.
        state.apply_advance(Direction.RIGHT, Direction.STAY)
        assert state.player1_position == (1, 0)
        assert state.player2_position == (2, 2)
        assert state.turn == 1

    def test_state_hash_changes(self):
        state = GameState(0, _make_config(cheese=[]))
        h0 = state.state_hash
        state.apply_advance(Direction.RIGHT, Direction.STAY)
        assert state.state_hash != h0

    def test_returns_new_hash(self):
        state = GameState(0, _make_config(cheese=[]))
        h = state.apply_advance(Direction.STAY, Direction.STAY)
        assert h == state.state_hash

    def test_last_moves_recorded(self):
        state = GameState(0, _make_config(cheese=[]))
        state.apply_advance(Direction.UP, Direction.LEFT)
        assert state.player1_last_move == Direction.UP
        assert state.player2_last_move == Direction.LEFT

    def test_cheese_matrix_updates_on_collection(self):
        # Single cheese at (1,0); player1 starts at (0,0) and moves RIGHT.
        state = GameState(0, _make_config(cheese=[(1, 0)]))
        assert state.cheese_matrix[1, 0] == 1
        state.apply_advance(Direction.RIGHT, Direction.STAY)
        assert state.cheese_matrix[1, 0] == 0


# ══════════════════════════════════════════════════════════
# 3. load_turn_state — full snapshot load
# ══════════════════════════════════════════════════════════


class TestLoadTurnState:
    def test_all_fields_change(self):
        state = GameState(0, _make_config())
        state.load_turn_state(_make_turn_state())

        assert state.turn == 1
        assert state.player1_position == (1, 0)
        assert state.player2_position == (2, 1)
        assert state.player1_score == 1.0
        assert state.player2_score == 0.5
        assert state.player1_mud_turns == 2
        assert state.player2_mud_turns == 0

    def test_cheese_updated(self):
        state = GameState(0, _make_config(cheese=[(1, 1), (0, 2)]))
        assert len(state.cheese) == 2
        state.load_turn_state(_make_turn_state(cheese=[(0, 2)]))
        assert state.cheese == [(0, 2)]
        assert state.cheese_matrix[1, 1] == 0
        assert state.cheese_matrix[0, 2] == 1

    def test_returns_state_hash(self):
        state = GameState(0, _make_config())
        h = state.load_turn_state(_make_turn_state())
        assert h == state.state_hash


# ══════════════════════════════════════════════════════════
# 4. load_full_state — rebuild maze + state
# ══════════════════════════════════════════════════════════


class TestLoadFullState:
    def test_rebuilds_with_new_config(self):
        state = GameState(0, _make_config())
        new_cfg = _make_config(width=5, height=5, max_turns=50)
        new_ts = _make_turn_state(player1_position=(2, 2), player2_position=(4, 4))
        state.load_full_state(new_cfg, new_ts)
        assert state.width == 5
        assert state.height == 5
        assert state.max_turns == 50
        assert state.player1_position == (2, 2)
        assert state.player2_position == (4, 4)


# ══════════════════════════════════════════════════════════
# 5. Perspective flipping
# ══════════════════════════════════════════════════════════


class TestPerspective:
    def test_player1_perspective(self):
        state = GameState(0, _make_config())
        state.load_turn_state(_make_turn_state())

        assert state.my_position == (1, 0)
        assert state.opponent_position == (2, 1)
        assert state.my_score == 1.0
        assert state.opponent_score == 0.5
        assert state.my_mud_turns == 2
        assert state.opponent_mud_turns == 0
        assert state.my_last_move == Direction.UP
        assert state.opponent_last_move == Direction.RIGHT
        assert state.my_player == Player.PLAYER1

    def test_player2_perspective(self):
        state = GameState(1, _make_config())
        state.load_turn_state(_make_turn_state())

        assert state.my_position == (2, 1)
        assert state.opponent_position == (1, 0)
        assert state.my_score == 0.5
        assert state.opponent_score == 1.0
        assert state.my_mud_turns == 0
        assert state.opponent_mud_turns == 2
        assert state.my_last_move == Direction.RIGHT
        assert state.opponent_last_move == Direction.UP
        assert state.my_player == Player.PLAYER2


# ══════════════════════════════════════════════════════════
# 6. Raw last_move properties
# ══════════════════════════════════════════════════════════


class TestRawLastMove:
    def test_player1_last_move(self):
        state = GameState(0, _make_config())
        state.load_turn_state(_make_turn_state(player1_last_move=2))
        assert state.player1_last_move == Direction.DOWN

    def test_player2_last_move(self):
        state = GameState(0, _make_config())
        state.load_turn_state(_make_turn_state(player2_last_move=3))
        assert state.player2_last_move == Direction.LEFT

    def test_initial_last_move_is_stay(self):
        state = GameState(0, _make_config())
        assert state.player1_last_move == Direction.STAY
        assert state.player2_last_move == Direction.STAY


# ══════════════════════════════════════════════════════════
# 7. _build_cheese_matrix
# ══════════════════════════════════════════════════════════


class TestBuildCheeseMatrix:
    def test_shape_and_dtype(self):
        mat = _build_cheese_matrix([(0, 0)], 5, 4)
        assert mat.shape == (5, 4)
        assert mat.dtype == np.uint8

    def test_specific_cells(self):
        mat = _build_cheese_matrix([(1, 2), (3, 0)], 5, 4)
        assert mat[1, 2] == 1
        assert mat[3, 0] == 1
        assert mat[0, 0] == 0
        assert mat.sum() == 2

    def test_empty_cheese(self):
        mat = _build_cheese_matrix([], 3, 3)
        assert mat.sum() == 0


# ══════════════════════════════════════════════════════════
# 8. Convenience methods (Layer 2-3)
# ══════════════════════════════════════════════════════════


class TestConvenienceMethods:
    def test_effective_moves_open_maze(self):
        """3x3 maze with no walls — center cell has all 4 directions."""
        state = GameState(0, _make_config())
        moves = state.effective_moves(pos=(1, 1))
        assert set(moves) == {
            Direction.UP,
            Direction.RIGHT,
            Direction.DOWN,
            Direction.LEFT,
        }

    def test_effective_moves_with_wall(self):
        """Wall between (1,1) and (1,2) blocks UP from (1,1)."""
        state = GameState(0, _make_config(walls=[((1, 1), (1, 2))]))
        moves = state.effective_moves(pos=(1, 1))
        assert Direction.UP not in moves
        assert len(moves) == 3

    def test_shortest_path_returns_path_result(self):
        state = GameState(0, _make_config())
        result = state.shortest_path((0, 0), (2, 2))
        assert result is not None
        assert isinstance(result, PathResult)
        assert result.target == (2, 2)
        assert all(isinstance(d, Direction) for d in result.path)
        assert len(result.first_moves) >= 1
        assert all(isinstance(d, Direction) for d in result.first_moves)
        assert result.first_moves[0] == result.path[0]
        assert result.cost > 0

    def test_shortest_path_same_cell(self):
        state = GameState(0, _make_config())
        result = state.shortest_path((0, 0), (0, 0))
        assert result is not None
        assert result.target == (0, 0)
        assert result.path == []
        assert result.cost == 0

    def test_nearest_cheese_returns_result(self):
        state = GameState(0, _make_config(cheese=[(1, 1)]))
        result = state.nearest_cheese(pos=(0, 0))
        assert result is not None
        assert isinstance(result, PathResult)
        assert result.target == (1, 1)
        assert all(isinstance(d, Direction) for d in result.path)
        assert len(result.first_moves) >= 1
        assert result.cost > 0

    def test_nearest_cheese_no_cheese(self):
        state = GameState(0, _make_config(cheese=[]))
        result = state.nearest_cheese(pos=(0, 0))
        assert result is None

    def test_distances_from(self):
        state = GameState(0, _make_config())
        dists = state.distances_from(pos=(0, 0))
        assert isinstance(dists, dict)
        # Open 3x3 → all 9 cells reachable.
        assert len(dists) == 9
        assert dists[(0, 0)] == 0
        assert dists[(1, 0)] == 1 or dists[(0, 1)] == 1


# ══════════════════════════════════════════════════════════
# 9. Simulation (Layer 4)
# ══════════════════════════════════════════════════════════


class TestSimulation:
    def test_to_sim_returns_game_sim(self):
        from pyrat_sdk._engine import GameSim

        state = GameState(0, _make_config())
        sim = state.to_sim()
        assert isinstance(sim, GameSim)

    def test_to_sim_is_independent_copy(self):
        state = GameState(0, _make_config(cheese=[]))
        sim = state.to_sim()
        sim.make_move(1, 4)
        # Mutating the clone doesn't move the SDK's mirror.
        assert state.player1_position == (0, 0)

    def test_make_move_changes_state(self):
        state = GameState(0, _make_config(cheese=[]))
        sim = state.to_sim()

        pos_before = sim.player1_position
        undo = sim.make_move(1, 4)
        assert sim.player1_position != pos_before

        sim.unmake_move(undo)
        assert sim.player1_position == pos_before

    def test_round_trip_restores_scores(self):
        state = GameState(0, _make_config(cheese=[(1, 0)]))
        sim = state.to_sim()

        score_before = sim.player1_score
        undo = sim.make_move(1, 4)
        sim.unmake_move(undo)
        assert sim.player1_score == score_before

    def test_is_game_over_after_all_cheese_collected(self):
        state = GameState(0, _make_config(cheese=[(1, 0)]))
        sim = state.to_sim()
        assert not sim.is_game_over

        sim.make_move(1, 4)
        assert sim.is_game_over

    def test_move_undo_properties(self):
        from pyrat_sdk._engine import MoveUndo

        state = GameState(0, _make_config())
        sim = state.to_sim()
        undo = sim.make_move(4, 4)
        assert isinstance(undo, MoveUndo)
        assert undo.p1_pos == (0, 0)
        assert undo.p2_pos == (2, 2)
        assert undo.turn == 0
        sim.unmake_move(undo)


# ══════════════════════════════════════════════════════════
# 10. apply_advance + state_hash matches load_turn_state
# ══════════════════════════════════════════════════════════


def test_apply_advance_matches_load_turn_state():
    """After one advance, loading the equivalent TurnState yields the same hash."""
    state_a = GameState(0, _make_config(cheese=[]))
    state_a.apply_advance(Direction.RIGHT, Direction.STAY)

    state_b = GameState(0, _make_config(cheese=[]))
    ts = {
        "turn": 1,
        "player1_position": (1, 0),
        "player2_position": (2, 2),
        "player1_score": 0.0,
        "player2_score": 0.0,
        "player1_mud_turns": 0,
        "player2_mud_turns": 0,
        "cheese": [],
        "player1_last_move": int(Direction.RIGHT),
        "player2_last_move": int(Direction.STAY),
    }
    state_b.load_turn_state(ts)

    assert state_a.state_hash == state_b.state_hash
