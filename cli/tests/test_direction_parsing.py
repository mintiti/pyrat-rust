"""Unit tests for direction parsing functions."""

import pytest
from pyrat_engine.game import Direction

from pyrat_runner.ai_process import get_direction_name, parse_direction
from pyrat_runner.display import get_direction_name as display_get_direction_name


class TestDirectionNameMapping:
    """Test direction to name conversion."""

    def test_get_direction_name_up(self):
        assert get_direction_name(Direction.UP) == "UP"

    def test_get_direction_name_down(self):
        assert get_direction_name(Direction.DOWN) == "DOWN"

    def test_get_direction_name_left(self):
        assert get_direction_name(Direction.LEFT) == "LEFT"

    def test_get_direction_name_right(self):
        assert get_direction_name(Direction.RIGHT) == "RIGHT"

    def test_get_direction_name_stay(self):
        assert get_direction_name(Direction.STAY) == "STAY"

    def test_get_direction_name_invalid_defaults_to_stay(self):
        # Test with a value that shouldn't exist
        class FakeDirection:
            def __int__(self):
                return 999

        assert get_direction_name(FakeDirection()) == "STAY"

    def test_display_get_direction_name_none(self):
        """Display module should handle None gracefully."""
        assert display_get_direction_name(None) == "NONE"


class TestDirectionParsing:
    """Test parsing direction names to Direction enum."""

    def test_parse_direction_up(self):
        assert parse_direction("UP") == Direction.UP

    def test_parse_direction_down(self):
        assert parse_direction("DOWN") == Direction.DOWN

    def test_parse_direction_left(self):
        assert parse_direction("LEFT") == Direction.LEFT

    def test_parse_direction_right(self):
        assert parse_direction("RIGHT") == Direction.RIGHT

    def test_parse_direction_stay(self):
        assert parse_direction("STAY") == Direction.STAY

    def test_parse_direction_invalid_defaults_to_stay(self):
        """Invalid direction names should default to STAY."""
        assert parse_direction("INVALID") == Direction.STAY
        assert parse_direction("") == Direction.STAY
        assert parse_direction("up") == Direction.STAY  # Case sensitive

    def test_parse_direction_roundtrip(self):
        """Test that parsing and converting back works."""
        for direction in [Direction.UP, Direction.DOWN, Direction.LEFT, Direction.RIGHT, Direction.STAY]:
            name = get_direction_name(direction)
            parsed = parse_direction(name)
            assert int(parsed) == int(direction)
