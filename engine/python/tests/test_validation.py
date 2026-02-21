"""Test input validation for PyRat engine via GameBuilder."""

import pytest
from pyrat_engine import GameBuilder


class TestPositionValidation:
    """Test position validation with proper error messages."""

    def test_negative_position_cheese(self):
        """Test that negative cheese positions give clear error messages."""
        with pytest.raises(ValueError, match="cannot be negative"):
            (
                GameBuilder(10, 10)
                .with_open_maze()
                .with_corner_positions()
                .with_custom_cheese([(-1, 0)])
                .build()
            )

    def test_negative_position_player1(self):
        """Test that negative player1 position gives clear error message."""
        with pytest.raises(ValueError, match="cannot be negative"):
            (
                GameBuilder(10, 10)
                .with_open_maze()
                .with_custom_positions((-1, 5), (9, 9))
                .with_custom_cheese([(5, 5)])
                .build()
            )

    def test_negative_position_player2(self):
        """Test that negative player2 position gives clear error message."""
        with pytest.raises(ValueError, match="cannot be negative"):
            (
                GameBuilder(10, 10)
                .with_open_maze()
                .with_custom_positions((0, 0), (5, -1))
                .with_custom_cheese([(5, 5)])
                .build()
            )

    def test_position_out_of_bounds_cheese(self):
        """Test that out-of-bounds cheese positions give clear error messages."""
        with pytest.raises(ValueError, match="outside board bounds"):
            (
                GameBuilder(10, 10)
                .with_open_maze()
                .with_corner_positions()
                .with_custom_cheese([(10, 10)])
                .build()
            )

    def test_position_out_of_bounds_player(self):
        """Test that out-of-bounds player positions give clear error messages."""
        with pytest.raises(ValueError, match="outside board bounds"):
            (
                GameBuilder(10, 10)
                .with_open_maze()
                .with_custom_positions((10, 0), (9, 9))
                .with_custom_cheese([(5, 5)])
                .build()
            )


class TestWallValidation:
    """Test wall validation with proper error messages."""

    def test_negative_wall_position(self):
        """Test that negative wall positions give clear error messages."""
        with pytest.raises(ValueError, match="cannot be negative"):
            (
                GameBuilder(10, 10)
                .with_custom_maze(walls=[((-1, 0), (0, 0))])
                .with_corner_positions()
                .with_custom_cheese([(5, 5)])
                .build()
            )

    def test_wall_non_adjacent(self):
        """Test that non-adjacent wall positions give clear error messages."""
        with pytest.raises(ValueError, match="must be between adjacent cells"):
            (
                GameBuilder(10, 10)
                .with_custom_maze(walls=[((0, 0), (2, 0))])
                .with_corner_positions()
                .with_custom_cheese([(5, 5)])
                .build()
            )

    def test_duplicate_walls(self):
        """Test that duplicate walls are detected."""
        with pytest.raises(ValueError, match="Duplicate wall"):
            (
                GameBuilder(10, 10)
                .with_custom_maze(
                    walls=[
                        ((0, 0), (0, 1)),
                        ((0, 1), (0, 0)),  # Same wall, different order
                    ]
                )
                .with_corner_positions()
                .with_custom_cheese([(5, 5)])
                .build()
            )


class TestMudValidation:
    """Test mud validation with proper error messages."""

    def test_negative_mud_value(self):
        """Test that negative mud values give clear error messages."""
        with pytest.raises(ValueError, match="cannot be negative"):
            (
                GameBuilder(10, 10)
                .with_custom_maze(walls=[], mud=[((0, 0), (0, 1), -1)])
                .with_corner_positions()
                .with_custom_cheese([(5, 5)])
                .build()
            )

    def test_negative_mud_position(self):
        """Test that negative mud positions give clear error messages."""
        with pytest.raises(ValueError, match="cannot be negative"):
            (
                GameBuilder(10, 10)
                .with_custom_maze(walls=[], mud=[((-1, 0), (0, 0), 3)])
                .with_corner_positions()
                .with_custom_cheese([(5, 5)])
                .build()
            )

    def test_mud_value_too_small(self):
        """Test that mud value < 2 gives clear error message."""
        with pytest.raises(ValueError, match="at least 2 turns"):
            (
                GameBuilder(10, 10)
                .with_custom_maze(walls=[], mud=[((0, 0), (0, 1), 1)])
                .with_corner_positions()
                .with_custom_cheese([(5, 5)])
                .build()
            )

    def test_mud_value_too_large(self):
        """Test that mud value > 255 gives clear error message."""
        with pytest.raises(ValueError, match="too large"):
            (
                GameBuilder(10, 10)
                .with_custom_maze(walls=[], mud=[((0, 0), (0, 1), 256)])
                .with_corner_positions()
                .with_custom_cheese([(5, 5)])
                .build()
            )

    def test_mud_non_adjacent(self):
        """Test that non-adjacent mud positions give clear error messages."""
        with pytest.raises(ValueError, match="must be between adjacent cells"):
            (
                GameBuilder(10, 10)
                .with_custom_maze(walls=[], mud=[((0, 0), (2, 0), 3)])
                .with_corner_positions()
                .with_custom_cheese([(5, 5)])
                .build()
            )


class TestCheeseValidation:
    """Test cheese validation with proper error messages."""

    def test_empty_cheese_list(self):
        """Test that empty cheese list gives clear error message."""
        with pytest.raises(ValueError, match="at least one cheese"):
            (
                GameBuilder(10, 10)
                .with_open_maze()
                .with_corner_positions()
                .with_custom_cheese([])
                .build()
            )

    def test_duplicate_cheese(self):
        """Test that duplicate cheese positions are detected."""
        with pytest.raises(ValueError, match="Duplicate cheese position"):
            (
                GameBuilder(10, 10)
                .with_open_maze()
                .with_corner_positions()
                .with_custom_cheese([(5, 5), (5, 5)])
                .build()
            )


class TestValidInputs:
    """Test that valid inputs work correctly."""

    def test_valid_game_creation(self):
        """Test that valid game creation works."""
        config = (
            GameBuilder(10, 10)
            .with_max_turns(100)
            .with_custom_maze(
                walls=[((0, 0), (0, 1)), ((1, 1), (1, 2))],
                mud=[((2, 2), (2, 3), 3), ((3, 3), (3, 4), 5)],
            )
            .with_custom_positions((0, 0), (9, 9))
            .with_custom_cheese([(5, 5), (7, 7)])
            .build()
        )
        game = config.create()

        assert game.width == 10  # noqa: PLR2004
        assert game.height == 10  # noqa: PLR2004
        assert game.player1_position.x == 0
        assert game.player1_position.y == 0
        assert game.player2_position.x == 9  # noqa: PLR2004
        assert game.player2_position.y == 9  # noqa: PLR2004
        assert len(game.cheese_positions()) == 2  # noqa: PLR2004
        assert game.max_turns == 100  # noqa: PLR2004

    def test_edge_positions(self):
        """Test that positions at the edges work correctly."""
        config = (
            GameBuilder(10, 10)
            .with_open_maze()
            .with_custom_positions((0, 0), (9, 9))
            .with_custom_cheese([(0, 0), (9, 9), (0, 9), (9, 0)])
            .build()
        )
        game = config.create()
        assert len(game.cheese_positions()) == 4  # noqa: PLR2004

    def test_maximum_mud_value(self):
        """Test that maximum mud value (255) works."""
        config = (
            GameBuilder(10, 10)
            .with_custom_maze(walls=[], mud=[((0, 0), (0, 1), 255)])
            .with_corner_positions()
            .with_custom_cheese([(5, 5)])
            .build()
        )
        game = config.create()
        mud_entries = game.mud_entries()
        assert any(m.value == 255 for m in mud_entries)  # noqa: PLR2004
