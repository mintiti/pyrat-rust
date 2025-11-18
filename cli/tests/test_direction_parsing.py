"""Unit tests for direction parsing functions."""

import pytest
from pyrat_engine.core import Direction
from pyrat_engine.core.types import direction_to_name, name_to_direction


class TestDirectionNameMapping:
    """Test direction to name conversion."""

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
        """Test direction enum to string conversion."""
        assert direction_to_name(direction) == expected_name

    def test_direction_to_name_invalid_defaults_to_stay(self):
        """Invalid direction values should default to STAY."""

        class FakeDirection:
            def __int__(self):
                return 999

        assert direction_to_name(FakeDirection()) == "STAY"


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
        """Test string to direction enum conversion."""
        assert name_to_direction(name) == expected_direction

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
    def test_name_to_direction_invalid_defaults_to_stay(self, invalid_name):
        """Invalid direction names should default to STAY."""
        assert name_to_direction(invalid_name) == Direction.STAY

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
        name = direction_to_name(direction)
        parsed = name_to_direction(name)
        assert int(parsed) == int(direction)
