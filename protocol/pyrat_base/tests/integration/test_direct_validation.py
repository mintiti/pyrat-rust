"""Test validation directly with GameBuilder."""

import pytest
from pyrat_engine import GameBuilder
from pyrat_engine.core.types import Coordinates


def test_negative_positions():
    """Test that negative positions give clear error messages."""
    # Test negative cheese position
    with pytest.raises(ValueError, match="cannot be negative"):
        (
            GameBuilder(10, 10)
            .with_open_maze()
            .with_corner_positions()
            .with_custom_cheese([(-1, 0)])
            .build()
        )

    # Test negative player position
    with pytest.raises(ValueError, match="cannot be negative"):
        (
            GameBuilder(10, 10)
            .with_open_maze()
            .with_custom_positions((-1, 0), (9, 9))
            .with_custom_cheese([(5, 5)])
            .build()
        )


def test_negative_mud():
    """Test that negative mud values give clear error messages."""
    with pytest.raises(ValueError, match="cannot be negative"):
        (
            GameBuilder(10, 10)
            .with_custom_maze(walls=[], mud=[((0, 0), (0, 1), -1)])
            .with_corner_positions()
            .with_custom_cheese([(5, 5)])
            .build()
        )


def test_out_of_bounds():
    """Test that out of bounds positions give clear error messages."""
    with pytest.raises(ValueError, match="outside board bounds"):
        (
            GameBuilder(10, 10)
            .with_open_maze()
            .with_corner_positions()
            .with_custom_cheese([(10, 10)])
            .build()
        )


def test_valid_creation():
    """Test that valid game creation still works."""
    config = (
        GameBuilder(10, 10)
        .with_custom_maze(
            walls=[((0, 0), (0, 1)), ((1, 1), (1, 2))],
            mud=[((2, 2), (2, 3), 3)],
        )
        .with_custom_positions((0, 0), (9, 9))
        .with_custom_cheese([(5, 5), (7, 7)])
        .build()
    )
    game = config.create()

    expected_width = 10
    expected_height = 10
    assert game.width == expected_width
    assert game.height == expected_height
    assert game.player1_position == Coordinates(0, 0)
    assert game.player2_position == Coordinates(9, 9)
