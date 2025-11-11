"""Test validation directly with PyGameState."""

from pyrat_engine.core.game import GameState as PyGameState
from pyrat_engine.core.types import Coordinates


def test_negative_positions():
    """Test that negative positions give clear error messages."""
    # Test negative cheese position
    try:
        PyGameState.create_custom(
            width=10,
            height=10,
            cheese=[(-1, 0)],
        )
        raise AssertionError("Should have raised ValueError")
    except ValueError as e:
        assert "Cheese position (-1, 0) cannot be negative" in str(e)

    # Test negative player position
    try:
        PyGameState.create_custom(
            width=10,
            height=10,
            cheese=[(5, 5)],
            player1_pos=(-1, 0),
        )
        raise AssertionError("Should have raised ValueError")
    except ValueError as e:
        assert "Player 1 position (-1, 0) cannot be negative" in str(e)


def test_negative_mud():
    """Test that negative mud values give clear error messages."""
    try:
        PyGameState.create_custom(
            width=10,
            height=10,
            cheese=[(5, 5)],
            mud=[((0, 0), (0, 1), -1)],
        )
        raise AssertionError("Should have raised ValueError")
    except ValueError as e:
        assert "Mud value -1 cannot be negative" in str(e)


def test_out_of_bounds():
    """Test that out of bounds positions give clear error messages."""
    try:
        PyGameState.create_custom(
            width=10,
            height=10,
            cheese=[(10, 10)],  # Equal to width/height is out of bounds
        )
        raise AssertionError("Should have raised ValueError")
    except ValueError as e:
        assert "Cheese position (10, 10) is outside maze bounds (10x10)" in str(e)


def test_valid_creation():
    """Test that valid game creation still works."""
    game = PyGameState.create_custom(
        width=10,
        height=10,
        walls=[((0, 0), (0, 1)), ((1, 1), (1, 2))],
        mud=[((2, 2), (2, 3), 3)],
        cheese=[(5, 5), (7, 7)],
        player1_pos=(0, 0),
        player2_pos=(9, 9),
    )
    expected_width = 10
    expected_height = 10
    assert game.width == expected_width
    assert game.height == expected_height
    assert game.player1_position == Coordinates(0, 0)
    assert game.player2_position == Coordinates(9, 9)
