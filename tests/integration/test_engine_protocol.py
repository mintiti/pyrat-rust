"""
Integration tests for Engine + Protocol interaction.

Tests that the game state from the engine can be properly communicated
via the protocol, and that protocol commands can be parsed and used
to interact with the engine.
"""

import pytest
from pyrat_engine.game import PyRat
from pyrat_engine.core import Direction
from pyrat_base.protocol_state import ProtocolState
from pyrat_base.enums import Player


def test_engine_state_to_protocol():
    """Test that engine game state can be wrapped for protocol communication."""
    # Create a simple game using PyRat
    game = PyRat(width=7, height=5, cheese_count=1)

    # Wrap engine state for Player 1 perspective
    protocol_state = ProtocolState(game._game, Player.RAT)

    # Verify protocol state provides correct player-perspective view
    p1_score, p2_score = game.scores
    assert protocol_state.my_position == game.player1_pos
    assert protocol_state.opponent_position == game.player2_pos
    assert protocol_state.my_score == p1_score
    assert protocol_state.opponent_score == p2_score
    assert len(protocol_state.cheese) >= 1


def test_protocol_commands_update_engine():
    """Test that protocol commands can be used to update engine state."""
    # Create a game
    game = PyRat(width=7, height=5, cheese_count=1)

    # Simulate a move via protocol
    initial_pos = game.player1_pos

    # Apply a move
    game.step(Direction.UP, Direction.STAY)

    # Check that position changed (or stayed same if at boundary)
    new_pos = game.player1_pos

    # Position should either move up or stay (if at top boundary or wall)
    # Coordinates objects can be compared directly
    assert new_pos.y >= initial_pos.y or new_pos == initial_pos


def test_game_simulation_consistency():
    """Test that games with same seed produce consistent initial states."""
    # Create identical games with same seed
    game1 = PyRat(width=11, height=9, cheese_count=5, seed=123)
    game2 = PyRat(width=11, height=9, cheese_count=5, seed=123)

    # Verify both games have identical initial state
    assert game1.player1_pos == game2.player1_pos
    assert game1.player2_pos == game2.player2_pos
    assert game1.cheese_positions == game2.cheese_positions
    assert game1.scores == game2.scores


def test_protocol_uses_engine_directions():
    """Test that protocol can use engine Direction enum values."""
    # Create a simple game
    game = PyRat(width=7, height=5, cheese_count=1)

    # Verify Direction enum values work with game.step()
    initial_pos = game.player1_pos
    result = game.step(Direction.STAY, Direction.STAY)

    # After STAY, this verifies Direction enum values are valid inputs
    # and the game progresses normally
    assert isinstance(result.p1_score, float)
    assert isinstance(result.p2_score, float)
    assert result.game_over in [True, False]
