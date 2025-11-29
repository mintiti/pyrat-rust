"""Tests for the ProtocolState wrapper class."""
# ruff: noqa: PLR2004

import numpy as np
import pytest
from pyrat_engine import PyRat
from pyrat_engine.core import Direction
from pyrat_engine.core.builder import GameConfigBuilder as PyGameConfigBuilder
from pyrat_engine.core.types import Coordinates

from pyrat_base import Player, ProtocolState


@pytest.fixture
def simple_game() -> PyRat:
    """Create a simple 5x5 game for testing."""
    return (
        PyGameConfigBuilder(5, 5)
        .with_cheese([(1, 1), (3, 3)])
        .with_player1_pos((0, 0))  # RAT at bottom-left
        .with_player2_pos((4, 4))  # PYTHON at top-right
        .with_max_turns(100)
        .build()
    )


class TestProtocolState:
    """Test suite for ProtocolState wrapper."""

    def test_initialization(self, simple_game):
        """Test ProtocolState initialization."""
        game = simple_game

        # Test as RAT
        state_rat = ProtocolState(game, Player.RAT)
        assert state_rat.i_am == Player.RAT
        assert state_rat._game is game
        assert state_rat._observation is None  # Not cached yet

        # Test as PYTHON
        state_python = ProtocolState(game, Player.PYTHON)
        assert state_python.i_am == Player.PYTHON
        assert state_python._game is game
        assert state_python._observation is None

    def test_direct_passthrough_properties(self, simple_game):
        """Test properties that pass through directly to PyRat."""
        game = simple_game
        state = ProtocolState(game, Player.RAT)

        # These should match the underlying game state
        assert state.width == 5
        assert state.height == 5
        assert state.turn == 0
        assert state.max_turns == 100

        # Cheese positions
        cheese = state.cheese
        assert len(cheese) == 2
        assert Coordinates(1, 1) in cheese
        assert Coordinates(3, 3) in cheese

        # Mud entries (empty in this simple game)
        assert state.mud == []

    def test_perspective_as_rat(self, simple_game):
        """Test player-perspective properties when playing as RAT."""
        game = simple_game
        state = ProtocolState(game, Player.RAT)

        # RAT perspective
        assert state.my_position == Coordinates(0, 0)
        assert state.opponent_position == Coordinates(4, 4)
        assert state.my_score == 0.0
        assert state.opponent_score == 0.0
        assert state.my_mud_turns == 0
        assert state.opponent_mud_turns == 0

    def test_perspective_as_python(self, simple_game):
        """Test player-perspective properties when playing as PYTHON."""
        game = simple_game
        state = ProtocolState(game, Player.PYTHON)

        # PYTHON perspective (swapped)
        assert state.my_position == Coordinates(4, 4)
        assert state.opponent_position == Coordinates(0, 0)
        assert state.my_score == 0.0
        assert state.opponent_score == 0.0
        assert state.my_mud_turns == 0
        assert state.opponent_mud_turns == 0

    def test_observation_caching(self, simple_game):
        """Test that observations are cached properly."""
        game = simple_game
        state = ProtocolState(game, Player.RAT)

        # First access should create observation
        assert state._observation is None
        _ = state.my_position
        assert state._observation is not None

        # Store reference to first observation
        first_obs = state._observation

        # Multiple accesses should use same cached observation
        _ = state.opponent_position
        _ = state.my_score
        assert state._observation is first_obs

        # Invalidating cache should clear it
        state.invalidate_cache()
        assert state._observation is None

        # Next access should create new observation
        _ = state.my_position
        assert state._observation is not None
        assert state._observation is not first_obs

    def test_matrix_properties(self, simple_game):
        """Test cheese_matrix and movement_matrix properties."""
        game = simple_game
        state = ProtocolState(game, Player.RAT)

        # Cheese matrix
        cheese_matrix = state.cheese_matrix
        assert isinstance(cheese_matrix, np.ndarray)
        assert cheese_matrix.shape == (5, 5)
        assert cheese_matrix[1, 1] == 1  # Cheese at (1,1)
        assert cheese_matrix[3, 3] == 1  # Cheese at (3,3)
        assert cheese_matrix[0, 0] == 0  # No cheese at player position

        # Movement matrix
        movement_matrix = state.movement_matrix
        assert isinstance(movement_matrix, np.ndarray)
        assert movement_matrix.shape == (5, 5, 4)
        # Check movement from (0,0) - should have walls on left and down
        # The third dimension corresponds to [UP, RIGHT, DOWN, LEFT]
        # Direction constants are plain ints, no .value needed
        assert movement_matrix[0, 0, Direction.DOWN] == -1  # DOWN is invalid (boundary)
        assert movement_matrix[0, 0, Direction.LEFT] == -1  # LEFT is invalid (boundary)
        assert movement_matrix[0, 0, Direction.UP] >= 0  # UP should be valid
        assert movement_matrix[0, 0, Direction.RIGHT] >= 0  # RIGHT should be valid

    def test_get_effective_moves(self, simple_game):
        """Test get_effective_moves convenience method."""
        game = simple_game
        state = ProtocolState(game, Player.RAT)

        # From (0,0), RAT can only go UP or RIGHT (plus STAY)
        effective_moves = state.get_effective_moves()
        assert Direction.STAY in effective_moves
        assert Direction.UP in effective_moves
        assert Direction.RIGHT in effective_moves
        assert Direction.DOWN not in effective_moves  # Blocked by boundary
        assert Direction.LEFT not in effective_moves  # Blocked by boundary
        assert len(effective_moves) == 3

    def test_get_move_cost(self, simple_game):
        """Test get_move_cost method."""
        game = simple_game
        state = ProtocolState(game, Player.RAT)

        # STAY always has cost 0
        assert state.get_move_cost(Direction.STAY) == 0

        # Effective moves should have cost >= 0
        assert state.get_move_cost(Direction.UP) == 0
        assert state.get_move_cost(Direction.RIGHT) == 0

        # Blocked moves should return None
        assert state.get_move_cost(Direction.DOWN) is None
        assert state.get_move_cost(Direction.LEFT) is None

    def test_with_mud(self):
        """Test protocol state with mud in the game."""
        # Create game with mud
        game = (
            PyGameConfigBuilder(5, 5)
            .with_cheese([(2, 2)])
            .with_player1_pos((0, 0))
            .with_player2_pos((4, 4))
            .with_mud([((0, 0), (1, 0), 2)])  # 2-turn mud to the right
            .build()
        )

        state = ProtocolState(game, Player.RAT)

        # Check mud entries (now returns Mud objects)
        mud_entries = state.mud
        assert len(mud_entries) == 1
        mud = mud_entries[0]
        assert (mud.pos1.x, mud.pos1.y) == (0, 0)
        assert (mud.pos2.x, mud.pos2.y) == (1, 0)
        assert mud.value == 2

        # Check movement cost reflects mud
        assert state.get_move_cost(Direction.RIGHT) == 2  # Mud cost
        assert state.get_move_cost(Direction.UP) == 0  # No mud

    def test_state_after_move(self, simple_game):
        """Test that state updates correctly after moves."""
        game = simple_game
        state_rat = ProtocolState(game, Player.RAT)
        state_python = ProtocolState(game, Player.PYTHON)

        # Initial positions
        assert state_rat.my_position == Coordinates(0, 0)
        assert state_python.my_position == Coordinates(4, 4)

        # Make a move (Direction constants are plain ints)
        game.step(Direction.RIGHT, Direction.LEFT)

        # Invalidate caches
        state_rat.invalidate_cache()
        state_python.invalidate_cache()

        # Check updated positions
        assert state_rat.my_position == Coordinates(1, 0)
        assert state_python.my_position == Coordinates(3, 4)

        # Check turn advanced
        assert state_rat.turn == 1
        assert state_python.turn == 1

    def test_score_tracking(self):
        """Test score tracking when collecting cheese."""
        # Place cheese next to starting positions
        game = (
            PyGameConfigBuilder(5, 5)
            .with_cheese([(1, 0), (3, 4)])
            .with_player1_pos((0, 0))
            .with_player2_pos((4, 4))
            .build()
        )

        state = ProtocolState(game, Player.RAT)

        # Move to collect cheese (Direction constants are plain ints)
        game.step(Direction.RIGHT, Direction.LEFT)
        state.invalidate_cache()

        # Both players should have collected cheese
        assert state.my_score == 1.0  # RAT collected cheese at (1,0)
        assert state.opponent_score == 1.0  # PYTHON collected cheese at (3,4)

        # Cheese should be gone
        assert len(state.cheese) == 0

    def test_repr(self, simple_game):
        """Test string representation."""
        game = simple_game
        state = ProtocolState(game, Player.RAT)

        repr_str = repr(state)
        assert "ProtocolState" in repr_str
        assert "turn=0/100" in repr_str
        assert "i_am=rat" in repr_str
        assert "my_pos=(0, 0)" in repr_str
        assert "opponent_pos=(4, 4)" in repr_str
        assert "my_score=0.0" in repr_str
        assert "opponent_score=0.0" in repr_str

    def test_perspective_consistency(self, simple_game):
        """Test that RAT and PYTHON perspectives are consistent."""
        game = simple_game
        state_rat = ProtocolState(game, Player.RAT)
        state_python = ProtocolState(game, Player.PYTHON)

        # Cross-check perspectives
        assert state_rat.my_position == state_python.opponent_position
        assert state_rat.opponent_position == state_python.my_position
        assert state_rat.my_score == state_python.opponent_score
        assert state_rat.opponent_score == state_python.my_score
        assert state_rat.my_mud_turns == state_python.opponent_mud_turns
        assert state_rat.opponent_mud_turns == state_python.my_mud_turns
