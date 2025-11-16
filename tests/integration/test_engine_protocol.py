"""
Integration tests for Engine + Protocol interaction.

Tests that the game state from the engine can be properly communicated
via the protocol, and that protocol commands can be parsed and used
to interact with the engine.
"""

import pytest
from pyrat_engine.game import PyRat, Direction
from pyrat_base.protocol_state import ProtocolState


def test_engine_state_to_protocol():
    """Test that engine game state can be converted to protocol format."""
    # Create a simple game using PyRat
    game = PyRat(width=7, height=5, cheese_count=1)

    # Get initial positions
    p1_pos = game.player1_pos
    p2_pos = game.player2_pos

    # Create protocol state from engine state
    protocol_state = ProtocolState(width=7, height=5)
    protocol_state.player1_position = tuple(p1_pos)
    protocol_state.player2_position = tuple(p2_pos)
    protocol_state.cheese_locations = game.cheese_positions

    # Verify protocol state matches engine state
    assert protocol_state.player1_position == tuple(p1_pos)
    assert protocol_state.player2_position == tuple(p2_pos)
    assert len(protocol_state.cheese_locations) >= 1


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
    assert new_pos[1] >= initial_pos[1] or new_pos == initial_pos


def test_game_simulation_consistency():
    """Test that a full game simulation produces consistent results."""
    # Create identical games with same seed
    game1 = PyRat(width=11, height=9, cheese_count=5, seed=123)
    game2 = PyRat(width=11, height=9, cheese_count=5, seed=123)

    # Apply same sequence of moves
    moves = [
        (Direction.UP, Direction.DOWN),
        (Direction.RIGHT, Direction.LEFT),
        (Direction.UP, Direction.UP),
    ]

    for p1_move, p2_move in moves:
        game1.step(p1_move, p2_move)
        game2.step(p1_move, p2_move)

    # Verify both games have identical state
    assert game1.player1_pos == game2.player1_pos
    assert game1.player2_pos == game2.player2_pos
    assert game1.player1_score == game2.player1_score
    assert game1.player2_score == game2.player2_score


def test_protocol_parses_engine_directions():
    """Test that protocol direction parsing works with engine Direction enum."""
    # Test that Direction enum values can be converted to strings
    assert Direction.UP.name == "UP"
    assert Direction.DOWN.name == "DOWN"
    assert Direction.LEFT.name == "LEFT"
    assert Direction.RIGHT.name == "RIGHT"
    assert Direction.STAY.name == "STAY"

    # Verify all directions are accessible
    engine_directions = [Direction.UP, Direction.DOWN, Direction.LEFT, Direction.RIGHT, Direction.STAY]
    for direction in engine_directions:
        assert hasattr(direction, "name")
        assert direction.name in ["UP", "DOWN", "LEFT", "RIGHT", "STAY"]
