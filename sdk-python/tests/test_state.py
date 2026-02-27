"""Tests for GameState — init, update, perspective, convenience methods."""

from __future__ import annotations

import numpy as np

from pyrat_sdk.state import (
    Direction,
    GameState,
    NearestCheeseResult,
    PathResult,
    Player,
    _build_cheese_matrix,
)


def _make_config(**overrides) -> dict:
    """Minimal config dict for GameState.__init__."""
    config = {
        "width": 3,
        "height": 3,
        "max_turns": 10,
        "walls": [],
        "mud": [],
        "cheese": [(1, 1)],
        "player1_start": (0, 0),
        "player2_start": (2, 2),
        "controlled_players": [0],
        "move_timeout_ms": 1000,
        "preprocessing_timeout_ms": 1000,
    }
    config.update(overrides)
    return config


def _make_turn_state(**overrides) -> dict:
    """Minimal TurnState dict for GameState.update."""
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
        state = GameState(_make_config())
        assert state.width == 3
        assert state.height == 3
        assert state.max_turns == 10
        assert state.turn == 0
        assert state.move_timeout_ms == 1000
        assert state.preprocessing_timeout_ms == 1000
        assert state.controlled_players == [0]

    def test_initial_positions(self):
        state = GameState(_make_config())
        assert state.player1_position == (0, 0)
        assert state.player2_position == (2, 2)

    def test_initial_scores_zero(self):
        state = GameState(_make_config())
        assert state.player1_score == 0.0
        assert state.player2_score == 0.0

    def test_maze_constructed(self):
        state = GameState(_make_config())
        # _maze should exist and be a PyMaze
        assert state._maze is not None

    def test_cheese_matrix_shape_and_values(self):
        state = GameState(_make_config(cheese=[(0, 0), (2, 1)]))
        assert state.cheese_matrix.shape == (3, 3)
        assert state.cheese_matrix.dtype == np.uint8
        assert state.cheese_matrix[0, 0] == 1
        assert state.cheese_matrix[2, 1] == 1
        assert state.cheese_matrix[1, 1] == 0

    def test_movement_matrix_shape(self):
        state = GameState(_make_config())
        assert state.movement_matrix.shape == (3, 3, 4)


# ══════════════════════════════════════════════════════════
# 2. Update
# ══════════════════════════════════════════════════════════


class TestUpdate:
    def test_all_fields_change(self):
        state = GameState(_make_config())
        ts = _make_turn_state()
        state.update(ts)

        assert state.turn == 1
        assert state.player1_position == (1, 0)
        assert state.player2_position == (2, 1)
        assert state.player1_score == 1.0
        assert state.player2_score == 0.5
        assert state.player1_mud_turns == 2
        assert state.player2_mud_turns == 0

    def test_cheese_updated(self):
        state = GameState(_make_config(cheese=[(1, 1), (0, 2)]))
        assert len(state.cheese) == 2
        state.update(_make_turn_state(cheese=[(0, 2)]))
        assert state.cheese == [(0, 2)]
        assert state.cheese_matrix[1, 1] == 0
        assert state.cheese_matrix[0, 2] == 1


# ══════════════════════════════════════════════════════════
# 3. Perspective flipping
# ══════════════════════════════════════════════════════════


class TestPerspective:
    def test_player1_perspective(self):
        state = GameState(_make_config(controlled_players=[0]))
        state.update(_make_turn_state())

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
        state = GameState(_make_config(controlled_players=[1]))
        state.update(_make_turn_state())

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
# 4. Raw last_move properties
# ══════════════════════════════════════════════════════════


class TestRawLastMove:
    def test_player1_last_move(self):
        state = GameState(_make_config())
        state.update(_make_turn_state(player1_last_move=2))
        assert state.player1_last_move == Direction.DOWN

    def test_player2_last_move(self):
        state = GameState(_make_config())
        state.update(_make_turn_state(player2_last_move=3))
        assert state.player2_last_move == Direction.LEFT

    def test_initial_last_move_is_stay(self):
        state = GameState(_make_config())
        assert state.player1_last_move == Direction.STAY
        assert state.player2_last_move == Direction.STAY


# ══════════════════════════════════════════════════════════
# 5. _build_cheese_matrix
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
# 6. Convenience methods (Layer 2-3)
# ══════════════════════════════════════════════════════════


class TestConvenienceMethods:
    def test_get_effective_moves_open_maze(self):
        """3x3 maze with no walls — center cell has all 4 directions."""
        state = GameState(_make_config(controlled_players=[0]))
        moves = state.get_effective_moves(pos=(1, 1))
        # Center of 3x3 with no walls → UP, RIGHT, DOWN, LEFT all valid.
        assert set(moves) == {
            Direction.UP,
            Direction.RIGHT,
            Direction.DOWN,
            Direction.LEFT,
        }

    def test_get_effective_moves_with_wall(self):
        """Add a wall and verify one direction is blocked."""
        # Wall between (1,1) and (1,2) blocks UP from (1,1)
        state = GameState(_make_config(walls=[((1, 1), (1, 2))]))
        moves = state.get_effective_moves(pos=(1, 1))
        assert Direction.UP not in moves
        # Other 3 directions should still be valid
        assert len(moves) == 3

    def test_shortest_path_returns_path_result(self):
        state = GameState(_make_config())
        result = state.shortest_path((0, 0), (2, 2))
        assert result is not None
        assert isinstance(result, PathResult)
        assert isinstance(result.directions, list)
        assert all(isinstance(d, Direction) for d in result.directions)
        assert isinstance(result.cost, int)
        assert result.cost > 0

    def test_shortest_path_same_cell(self):
        state = GameState(_make_config())
        result = state.shortest_path((0, 0), (0, 0))
        # Same cell → empty path, cost 0
        assert result is not None
        assert result.directions == []
        assert result.cost == 0

    def test_nearest_cheese_returns_result(self):
        state = GameState(_make_config(cheese=[(1, 1)]))
        result = state.nearest_cheese(pos=(0, 0))
        assert result is not None
        assert isinstance(result, NearestCheeseResult)
        assert result.position == (1, 1)
        assert isinstance(result.directions, list)
        assert all(isinstance(d, Direction) for d in result.directions)
        assert result.cost > 0

    def test_nearest_cheese_no_cheese(self):
        state = GameState(_make_config(cheese=[]))
        # Update to empty cheese
        state.cheese = []
        result = state.nearest_cheese(pos=(0, 0))
        assert result is None

    def test_distances_from(self):
        state = GameState(_make_config())
        dists = state.distances_from(pos=(0, 0))
        assert isinstance(dists, dict)
        # Should reach all 9 cells in a 3x3 open maze.
        assert len(dists) == 9
        assert dists[(0, 0)] == 0
        # Adjacent cell costs 1
        assert dists[(1, 0)] == 1 or dists[(0, 1)] == 1
