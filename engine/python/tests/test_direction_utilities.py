"""Unit tests for Direction IntEnum.

This module tests that the Direction IntEnum behaves correctly:
- Enum members have correct integer values
- Name lookup works via Direction["NAME"]
- Integer conversion works via Direction(value)
- Standard IntEnum properties work (.name, .value, iteration)
"""

import pytest
from pyrat_engine.core.types import Direction

# Number of directions in the Direction enum
DIRECTION_COUNT = 5


class TestDirectionValues:
    """Test Direction enum integer values."""

    @pytest.mark.parametrize(
        "direction,expected_value",
        [
            (Direction.UP, 0),
            (Direction.RIGHT, 1),
            (Direction.DOWN, 2),
            (Direction.LEFT, 3),
            (Direction.STAY, 4),
        ],
    )
    def test_direction_values(self, direction, expected_value):
        """Test that Direction enum members have correct integer values."""
        assert direction == expected_value
        assert int(direction) == expected_value
        assert direction.value == expected_value

    def test_direction_is_int(self):
        """Test that Direction members are integers."""
        for direction in Direction:
            assert isinstance(direction, int)


class TestDirectionNameLookup:
    """Test Direction name to enum conversion."""

    @pytest.mark.parametrize(
        "name,expected_direction",
        [
            ("UP", Direction.UP),
            ("RIGHT", Direction.RIGHT),
            ("DOWN", Direction.DOWN),
            ("LEFT", Direction.LEFT),
            ("STAY", Direction.STAY),
        ],
    )
    def test_name_lookup(self, name, expected_direction):
        """Test string to direction enum conversion via Direction["NAME"]."""
        assert Direction[name] == expected_direction

    @pytest.mark.parametrize(
        "invalid_name",
        [
            "up",  # Lowercase
            "INVALID",
            "NORTH",
            "",
        ],
    )
    def test_invalid_name_raises_key_error(self, invalid_name):
        """Test that invalid names raise KeyError."""
        with pytest.raises(KeyError):
            Direction[invalid_name]


class TestDirectionIntConversion:
    """Test Direction integer to enum conversion."""

    @pytest.mark.parametrize(
        "value,expected_direction",
        [
            (0, Direction.UP),
            (1, Direction.RIGHT),
            (2, Direction.DOWN),
            (3, Direction.LEFT),
            (4, Direction.STAY),
        ],
    )
    def test_int_conversion(self, value, expected_direction):
        """Test integer to direction enum conversion via Direction(value)."""
        assert Direction(value) == expected_direction

    @pytest.mark.parametrize(
        "invalid_value",
        [-1, 5, 100, 999],
    )
    def test_invalid_int_raises_value_error(self, invalid_value):
        """Test that invalid integers raise ValueError."""
        with pytest.raises(ValueError):
            Direction(invalid_value)


class TestDirectionNameProperty:
    """Test Direction .name property."""

    @pytest.mark.parametrize(
        "direction,expected_name",
        [
            (Direction.UP, "UP"),
            (Direction.RIGHT, "RIGHT"),
            (Direction.DOWN, "DOWN"),
            (Direction.LEFT, "LEFT"),
            (Direction.STAY, "STAY"),
        ],
    )
    def test_direction_name(self, direction, expected_name):
        """Test direction enum .name property."""
        assert direction.name == expected_name

    def test_name_via_int_conversion(self):
        """Test getting name from integer via Direction(int).name."""
        assert Direction(0).name == "UP"
        assert Direction(1).name == "RIGHT"
        assert Direction(2).name == "DOWN"
        assert Direction(3).name == "LEFT"
        assert Direction(4).name == "STAY"


class TestDirectionIteration:
    """Test Direction enum iteration."""

    def test_iteration(self):
        """Test that Direction can be iterated."""
        directions = list(Direction)
        assert len(directions) == DIRECTION_COUNT
        assert Direction.UP in directions
        assert Direction.RIGHT in directions
        assert Direction.DOWN in directions
        assert Direction.LEFT in directions
        assert Direction.STAY in directions

    def test_iteration_order(self):
        """Test that Direction iteration order matches value order."""
        directions = list(Direction)
        assert directions[0] == Direction.UP
        assert directions[1] == Direction.RIGHT
        assert directions[2] == Direction.DOWN
        assert directions[3] == Direction.LEFT
        assert directions[4] == Direction.STAY


class TestDirectionRoundtrip:
    """Test Direction roundtrip conversions."""

    def test_name_roundtrip(self):
        """Test direction -> name -> direction roundtrip."""
        for direction in Direction:
            name = direction.name
            parsed = Direction[name]
            assert parsed == direction

    def test_value_roundtrip(self):
        """Test direction -> value -> direction roundtrip."""
        for direction in Direction:
            value = int(direction)
            parsed = Direction(value)
            assert parsed == direction
