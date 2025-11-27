"""Unit tests for direction parsing in CLI.

Tests that Direction IntEnum is used correctly for parsing direction
strings in the CLI context.
"""

import pytest
from pyrat_engine.core.types import Direction


class TestDirectionNameMapping:
    """Test direction to name conversion using IntEnum."""

    @pytest.mark.parametrize(
        "direction,expected_name",
        [
            (Direction.UP, "UP"),
            (Direction.DOWN, "DOWN"),
            (Direction.LEFT, "LEFT"),
            (Direction.RIGHT, "RIGHT"),
            (Direction.STAY, "STAY"),
        ],
    )
    def test_direction_to_name(self, direction, expected_name):
        """Test direction enum .name property."""
        assert direction.name == expected_name

    def test_int_to_name(self):
        """Test converting integer to direction name via Direction(int).name."""
        assert Direction(0).name == "UP"
        assert Direction(1).name == "RIGHT"
        assert Direction(2).name == "DOWN"
        assert Direction(3).name == "LEFT"
        assert Direction(4).name == "STAY"

    def test_invalid_int_raises_error(self):
        """Invalid direction integer values should raise ValueError."""
        with pytest.raises(ValueError):
            Direction(999)


class TestDirectionParsing:
    """Test parsing direction names to Direction enum."""

    @pytest.mark.parametrize(
        "name,expected_direction",
        [
            ("UP", Direction.UP),
            ("DOWN", Direction.DOWN),
            ("LEFT", Direction.LEFT),
            ("RIGHT", Direction.RIGHT),
            ("STAY", Direction.STAY),
        ],
    )
    def test_name_to_direction(self, name, expected_direction):
        """Test string to direction enum conversion via Direction[name]."""
        assert Direction[name] == expected_direction

    @pytest.mark.parametrize(
        "invalid_name",
        [
            "INVALID",
            "",
            "up",  # Case sensitive
            "down",
            "123",
            "North",
        ],
    )
    def test_invalid_name_raises_key_error(self, invalid_name):
        """Invalid direction names should raise KeyError."""
        with pytest.raises(KeyError):
            Direction[invalid_name]

    @pytest.mark.parametrize(
        "direction",
        [
            Direction.UP,
            Direction.DOWN,
            Direction.LEFT,
            Direction.RIGHT,
            Direction.STAY,
        ],
    )
    def test_direction_roundtrip(self, direction):
        """Test that direction -> name -> direction roundtrip works."""
        name = direction.name
        parsed = Direction[name]
        assert parsed == direction
        assert int(parsed) == int(direction)
