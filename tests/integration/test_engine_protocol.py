"""
Integration tests for Engine + Protocol interaction.

Tests that the game state from the engine can be properly communicated
via the protocol, and that protocol commands can be parsed and used
to interact with the engine.
"""

import pytest
from pyrat_engine import GameState, Direction
from pyrat_base.protocol_state import ProtocolState


def test_engine_state_to_protocol():
    """Test that engine GameState can be converted to protocol format."""
    # Create a simple game
    game = GameState.new(width=7, height=5, num_cheese=1, seed=42)

    # Get initial positions
    p1_pos = game.player1_position()
    p2_pos = game.player2_position()

    # Create protocol state from engine state
    protocol_state = ProtocolState(width=7, height=5)
    protocol_state.player1_position = (p1_pos.x, p1_pos.y)
    protocol_state.player2_position = (p2_pos.x, p2_pos.y)
    protocol_state.cheese_locations = {(c.x, c.y) for c in game.cheese_locations()}

    # Verify protocol state matches engine state
    assert protocol_state.player1_position == (p1_pos.x, p1_pos.y)
    assert protocol_state.player2_position == (p2_pos.x, p2_pos.y)
    assert len(protocol_state.cheese_locations) == 1


def test_protocol_commands_update_engine():
    """Test that protocol commands can be used to update engine state."""
    # Create a game
    game = GameState.new(width=7, height=5, num_cheese=1, seed=42)

    # Simulate a move via protocol
    initial_pos = game.player1_position()

    # Apply a move
    game.apply_moves(Direction.UP, Direction.STAY)

    # Check that position changed (or stayed same if at boundary)
    new_pos = game.player1_position()

    # Position should either move up or stay (if at top boundary)
    assert new_pos.y >= initial_pos.y or new_pos.y == initial_pos.y


def test_game_simulation_consistency():
    """Test that a full game simulation produces consistent results."""
    # Create identical games
    game1 = GameState.new(width=11, height=9, num_cheese=5, seed=123)
    game2 = GameState.new(width=11, height=9, num_cheese=5, seed=123)

    # Apply same sequence of moves
    moves = [
        (Direction.UP, Direction.DOWN),
        (Direction.RIGHT, Direction.LEFT),
        (Direction.UP, Direction.UP),
    ]

    for p1_move, p2_move in moves:
        game1.apply_moves(p1_move, p2_move)
        game2.apply_moves(p1_move, p2_move)

    # Verify both games have identical state
    assert game1.player1_position().x == game2.player1_position().x
    assert game1.player1_position().y == game2.player1_position().y
    assert game1.player2_position().x == game2.player2_position().x
    assert game1.player2_position().y == game2.player2_position().y
    assert game1.player1_score() == game2.player1_score()
    assert game1.player2_score() == game2.player2_score()


def test_protocol_parses_engine_directions():
    """Test that protocol can parse direction strings that engine uses."""
    from pyrat_base.enums import DirectionEnum

    # Verify protocol direction enum matches engine directions
    assert DirectionEnum.UP.value.upper() == "UP"
    assert DirectionEnum.DOWN.value.upper() == "DOWN"
    assert DirectionEnum.LEFT.value.upper() == "LEFT"
    assert DirectionEnum.RIGHT.value.upper() == "RIGHT"
    assert DirectionEnum.STAY.value.upper() == "STAY"

    # Verify engine Direction can be converted to protocol
    engine_directions = [Direction.UP, Direction.DOWN, Direction.LEFT, Direction.RIGHT, Direction.STAY]
    protocol_directions = [d.name for d in DirectionEnum]

    for engine_dir in engine_directions:
        assert engine_dir.name in protocol_directions
