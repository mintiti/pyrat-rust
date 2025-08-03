"""Test input validation for PyRat engine."""

import pytest
from pyrat_engine._rust import PyGameState


class TestPositionValidation:
    """Test position validation with proper error messages."""

    def test_negative_position_cheese(self):
        """Test that negative cheese positions give clear error messages."""
        with pytest.raises(ValueError) as exc_info:
            PyGameState.create_custom(
                width=10,
                height=10,
                cheese=[(-1, 0)],
            )
        assert "Cheese position (-1, 0) cannot be negative" in str(exc_info.value)

    def test_negative_position_player1(self):
        """Test that negative player1 position gives clear error message."""
        with pytest.raises(ValueError) as exc_info:
            PyGameState.create_custom(
                width=10,
                height=10,
                cheese=[(5, 5)],
                player1_pos=(-1, 5),
            )
        assert "Player 1 position (-1, 5) cannot be negative" in str(exc_info.value)

    def test_negative_position_player2(self):
        """Test that negative player2 position gives clear error message."""
        with pytest.raises(ValueError) as exc_info:
            PyGameState.create_custom(
                width=10,
                height=10,
                cheese=[(5, 5)],
                player2_pos=(5, -1),
            )
        assert "Player 2 position (5, -1) cannot be negative" in str(exc_info.value)

    def test_position_too_large(self):
        """Test that positions > 255 give clear error messages."""
        with pytest.raises(ValueError) as exc_info:
            PyGameState.create_custom(
                width=10,
                height=10,
                cheese=[(256, 0)],
            )
        assert "Cheese position (256, 0) is too large (maximum is 255)" in str(
            exc_info.value
        )

    def test_position_out_of_bounds(self):
        """Test that out-of-bounds positions give clear error messages."""
        with pytest.raises(ValueError) as exc_info:
            PyGameState.create_custom(
                width=10,
                height=10,
                cheese=[(10, 10)],  # Equal to width/height is out of bounds
            )
        assert "Cheese position (10, 10) is outside maze bounds (10x10)" in str(
            exc_info.value
        )


class TestWallValidation:
    """Test wall validation with proper error messages."""

    def test_negative_wall_position(self):
        """Test that negative wall positions give clear error messages."""
        with pytest.raises(ValueError) as exc_info:
            PyGameState.create_custom(
                width=10,
                height=10,
                walls=[((-1, 0), (0, 0))],
                cheese=[(5, 5)],
            )
        assert "Wall start position (-1, 0) cannot be negative" in str(exc_info.value)

    def test_wall_non_adjacent(self):
        """Test that non-adjacent wall positions give clear error messages."""
        with pytest.raises(ValueError) as exc_info:
            PyGameState.create_custom(
                width=10,
                height=10,
                walls=[((0, 0), (2, 0))],  # Not adjacent
                cheese=[(5, 5)],
            )
        assert "Wall between (0, 0) and (2, 0) must be between adjacent cells" in str(
            exc_info.value
        )

    def test_duplicate_walls(self):
        """Test that duplicate walls are detected."""
        with pytest.raises(ValueError) as exc_info:
            PyGameState.create_custom(
                width=10,
                height=10,
                walls=[
                    ((0, 0), (0, 1)),
                    ((0, 1), (0, 0)),  # Same wall, different order
                ],
                cheese=[(5, 5)],
            )
        assert "Duplicate wall" in str(exc_info.value)


class TestMudValidation:
    """Test mud validation with proper error messages."""

    def test_negative_mud_value(self):
        """Test that negative mud values give clear error messages."""
        with pytest.raises(ValueError) as exc_info:
            PyGameState.create_custom(
                width=10,
                height=10,
                mud=[((0, 0), (0, 1), -1)],
                cheese=[(5, 5)],
            )
        assert "Mud value -1 cannot be negative" in str(exc_info.value)

    def test_negative_mud_position(self):
        """Test that negative mud positions give clear error messages."""
        with pytest.raises(ValueError) as exc_info:
            PyGameState.create_custom(
                width=10,
                height=10,
                mud=[((-1, 0), (0, 0), 3)],
                cheese=[(5, 5)],
            )
        assert "Mud start position (-1, 0) cannot be negative" in str(exc_info.value)

    def test_mud_value_too_small(self):
        """Test that mud value < 2 gives clear error message."""
        with pytest.raises(ValueError) as exc_info:
            PyGameState.create_custom(
                width=10,
                height=10,
                mud=[((0, 0), (0, 1), 1)],
                cheese=[(5, 5)],
            )
        assert "Mud value must be at least 2 turns" in str(exc_info.value)

    def test_mud_value_too_large(self):
        """Test that mud value > 255 gives clear error message."""
        with pytest.raises(ValueError) as exc_info:
            PyGameState.create_custom(
                width=10,
                height=10,
                mud=[((0, 0), (0, 1), 256)],
                cheese=[(5, 5)],
            )
        assert "Mud value 256 is too large (maximum is 255)" in str(exc_info.value)

    def test_mud_non_adjacent(self):
        """Test that non-adjacent mud positions give clear error messages."""
        with pytest.raises(ValueError) as exc_info:
            PyGameState.create_custom(
                width=10,
                height=10,
                mud=[((0, 0), (2, 0), 3)],  # Not adjacent
                cheese=[(5, 5)],
            )
        assert "Mud between (0, 0) and (2, 0) must be between adjacent cells" in str(
            exc_info.value
        )

    def test_wall_mud_conflict(self):
        """Test that walls and mud can coexist (no longer an error)."""
        # This used to be an error, but the semantic fix clarifies that mud exists on passages
        # The maze generator ensures mud only exists on valid connections, not walls
        game = PyGameState.create_custom(
            width=10,
            height=10,
            walls=[((0, 0), (0, 1))],
            mud=[((0, 0), (0, 1), 3)],
            cheese=[(5, 5)],
        )
        assert game is not None  # Should create successfully


class TestCheeseValidation:
    """Test cheese validation with proper error messages."""

    def test_empty_cheese_list(self):
        """Test that empty cheese list gives clear error message."""
        with pytest.raises(ValueError) as exc_info:
            PyGameState.create_custom(
                width=10,
                height=10,
                cheese=[],
            )
        assert "Game must have at least one cheese" in str(exc_info.value)

    def test_duplicate_cheese(self):
        """Test that duplicate cheese positions are detected."""
        with pytest.raises(ValueError) as exc_info:
            PyGameState.create_custom(
                width=10,
                height=10,
                cheese=[(5, 5), (5, 5)],
            )
        assert "Duplicate cheese position" in str(exc_info.value)


class TestValidInputs:
    """Test that valid inputs work correctly."""

    def test_valid_game_creation(self):
        """Test that valid game creation works."""
        game = PyGameState.create_custom(
            width=10,
            height=10,
            walls=[((0, 0), (0, 1)), ((1, 1), (1, 2))],
            mud=[((2, 2), (2, 3), 3), ((3, 3), (3, 4), 5)],
            cheese=[(5, 5), (7, 7)],
            player1_pos=(0, 0),
            player2_pos=(9, 9),
            max_turns=100,
        )
        expected_width = 10
        expected_height = 10
        expected_cheese_count = 2
        expected_max_turns = 100

        assert game.width == expected_width
        assert game.height == expected_height
        assert game.player1_position.x == 0
        assert game.player1_position.y == 0
        assert game.player2_position.x == 9  # noqa: PLR2004
        assert game.player2_position.y == 9  # noqa: PLR2004
        assert len(game.cheese_positions()) == expected_cheese_count
        assert game.max_turns == expected_max_turns

    def test_edge_positions(self):
        """Test that positions at the edges work correctly."""
        game = PyGameState.create_custom(
            width=10,
            height=10,
            cheese=[(0, 0), (9, 9), (0, 9), (9, 0)],
            player1_pos=(0, 0),
            player2_pos=(9, 9),
        )
        expected_cheese_count = 4
        assert len(game.cheese_positions()) == expected_cheese_count

    def test_maximum_mud_value(self):
        """Test that maximum mud value (255) works."""
        game = PyGameState.create_custom(
            width=10,
            height=10,
            mud=[((0, 0), (0, 1), 255)],
            cheese=[(5, 5)],
        )
        mud_entries = game.mud_entries()
        max_mud_value = 255
        assert any(entry[2] == max_mud_value for entry in mud_entries)
