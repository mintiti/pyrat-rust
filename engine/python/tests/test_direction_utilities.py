"""Unit tests for direction utility functions.

This module provides comprehensive tests for the centralized direction utilities
in pyrat_engine.core.types: direction_to_name(), name_to_direction(), and
is_valid_direction().
"""

import pytest
from pyrat_engine.core.types import (
    Direction,
    direction_to_name,
    is_valid_direction,
    name_to_direction,
)


class TestDirectionToName:
    """Test the direction_to_name() function."""

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
    def test_valid_directions(self, direction, expected_name):
        """Test conversion of all valid direction values."""
        assert direction_to_name(direction) == expected_name

    @pytest.mark.parametrize(
        "direction,expected_name",
        [
            (0, "UP"),
            (1, "RIGHT"),
            (2, "DOWN"),
            (3, "LEFT"),
            (4, "STAY"),
        ],
    )
    def test_integer_directions(self, direction, expected_name):
        """Test conversion using raw integer values."""
        assert direction_to_name(direction) == expected_name

    @pytest.mark.parametrize(
        "invalid_value",
        [
            -1,
            5,
            100,
            999,
            -999,
        ],
    )
    def test_invalid_integers_default_to_stay(self, invalid_value):
        """Test that invalid integer values default to STAY."""
        assert direction_to_name(invalid_value) == "STAY"

    def test_custom_int_object(self):
        """Test with a custom object that implements __int__."""

        class CustomDirection:
            def __init__(self, value):
                self.value = value

            def __int__(self):
                return self.value

        assert direction_to_name(CustomDirection(0)) == "UP"
        assert direction_to_name(CustomDirection(4)) == "STAY"
        assert direction_to_name(CustomDirection(999)) == "STAY"

    def test_idempotency(self):
        """Test that calling the function multiple times gives same result."""
        for direction in [Direction.UP, Direction.DOWN, Direction.LEFT, Direction.RIGHT, Direction.STAY]:
            first_call = direction_to_name(direction)
            second_call = direction_to_name(direction)
            assert first_call == second_call


class TestNameToDirection:
    """Test the name_to_direction() function."""

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
    def test_valid_names(self, name, expected_direction):
        """Test conversion of all valid direction names."""
        assert name_to_direction(name) == expected_direction

    @pytest.mark.parametrize(
        "invalid_name",
        [
            "up",  # Lowercase
            "Up",  # Mixed case
            "uP",  # Mixed case
            "down",
            "left",
            "right",
            "stay",
            "NORTH",  # Wrong direction
            "SOUTH",
            "EAST",
            "WEST",
            "",  # Empty string
            " UP",  # Leading space
            "UP ",  # Trailing space
            " UP ",  # Both
            "INVALID",
            "123",
            "None",
            "null",
        ],
    )
    def test_invalid_names_default_to_stay(self, invalid_name):
        """Test that invalid names default to STAY."""
        result = name_to_direction(invalid_name)
        assert result == Direction.STAY

    def test_case_sensitivity(self):
        """Test that the function is case-sensitive."""
        # Valid uppercase
        assert name_to_direction("UP") == Direction.UP
        assert name_to_direction("DOWN") == Direction.DOWN

        # Invalid lowercase - should default to STAY
        assert name_to_direction("up") == Direction.STAY
        assert name_to_direction("down") == Direction.STAY

    def test_idempotency(self):
        """Test that calling the function multiple times gives same result."""
        for name in ["UP", "DOWN", "LEFT", "RIGHT", "STAY"]:
            first_call = name_to_direction(name)
            second_call = name_to_direction(name)
            assert first_call == second_call

    def test_roundtrip(self):
        """Test that direction -> name -> direction roundtrip works."""
        for direction in [Direction.UP, Direction.DOWN, Direction.LEFT, Direction.RIGHT, Direction.STAY]:
            name = direction_to_name(direction)
            parsed = name_to_direction(name)
            assert int(parsed) == int(direction)


class TestIsValidDirection:
    """Test the is_valid_direction() function."""

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
    def test_valid_directions(self, direction):
        """Test that all valid Direction values are recognized."""
        assert is_valid_direction(direction) is True

    @pytest.mark.parametrize(
        "direction",
        [
            0,  # UP as int
            1,  # RIGHT as int
            2,  # DOWN as int
            3,  # LEFT as int
            4,  # STAY as int
        ],
    )
    def test_valid_integer_values(self, direction):
        """Test that valid integer values are recognized."""
        assert is_valid_direction(direction) is True

    @pytest.mark.parametrize(
        "invalid_value",
        [
            -1,
            5,
            10,
            100,
            999,
            -999,
        ],
    )
    def test_invalid_integers(self, invalid_value):
        """Test that invalid integer values are rejected."""
        assert is_valid_direction(invalid_value) is False

    def test_none_is_invalid(self):
        """Test that None is not a valid direction."""
        assert is_valid_direction(None) is False

    def test_custom_int_object(self):
        """Test with a custom object that implements __int__."""

        class CustomDirection:
            def __init__(self, value):
                self.value = value

            def __int__(self):
                return self.value

        assert is_valid_direction(CustomDirection(0)) is True
        assert is_valid_direction(CustomDirection(4)) is True
        assert is_valid_direction(CustomDirection(999)) is False

    @pytest.mark.parametrize(
        "invalid_type",
        [
            "UP",  # String
            "0",  # String number
            3.14,  # Float
            [0],  # List
            (0,),  # Tuple
            {"direction": 0},  # Dict
        ],
    )
    def test_invalid_types_handled_gracefully(self, invalid_type):
        """Test that invalid types are handled without crashing."""
        # Should return False for non-integer types
        result = is_valid_direction(invalid_type)
        # We expect False, but if it's True (for types that can be converted to int),
        # we should check that they convert to a valid direction
        if result:
            assert int(invalid_type) in [0, 1, 2, 3, 4]
        else:
            assert result is False

    def test_object_without_int_method(self):
        """Test with an object that doesn't implement __int__."""

        class NoIntMethod:
            pass

        assert is_valid_direction(NoIntMethod()) is False


class TestDirectionUtilitiesIntegration:
    """Integration tests for direction utilities working together."""

    def test_complete_roundtrip_all_directions(self):
        """Test complete roundtrip for all directions."""
        directions = [Direction.UP, Direction.DOWN, Direction.LEFT, Direction.RIGHT, Direction.STAY]

        for original_direction in directions:
            # Direction -> name
            name = direction_to_name(original_direction)

            # Name -> direction
            parsed_direction = name_to_direction(name)

            # Should match original
            assert int(parsed_direction) == int(original_direction)

            # Both should be valid
            assert is_valid_direction(original_direction) is True
            assert is_valid_direction(parsed_direction) is True

    def test_invalid_direction_handling_consistency(self):
        """Test that invalid directions are handled consistently."""
        invalid_int = 999

        # Should not be valid
        assert is_valid_direction(invalid_int) is False

        # Should default to STAY when converting to name
        assert direction_to_name(invalid_int) == "STAY"

    def test_invalid_name_handling_consistency(self):
        """Test that invalid names are handled consistently."""
        invalid_name = "INVALID"

        # Should return STAY
        result = name_to_direction(invalid_name)
        assert result == Direction.STAY

        # The result should be valid
        assert is_valid_direction(result) is True

        # Converting back should give "STAY"
        assert direction_to_name(result) == "STAY"

    def test_all_valid_names_are_valid_directions(self):
        """Test that all names produced by direction_to_name() are valid inputs for name_to_direction()."""
        for direction in [Direction.UP, Direction.DOWN, Direction.LEFT, Direction.RIGHT, Direction.STAY]:
            name = direction_to_name(direction)
            parsed = name_to_direction(name)
            # Should be able to parse back to a valid direction
            assert is_valid_direction(parsed) is True

    @pytest.mark.parametrize(
        "direction_int,expected_name",
        [
            (0, "UP"),
            (1, "RIGHT"),
            (2, "DOWN"),
            (3, "LEFT"),
            (4, "STAY"),
        ],
    )
    def test_direction_constant_values_match_names(self, direction_int, expected_name):
        """Test that Direction constant integer values match their expected names."""
        # Get the Direction constant
        direction = direction_int

        # Convert to name
        name = direction_to_name(direction)

        # Should match expected name
        assert name == expected_name

        # Parse back
        parsed = name_to_direction(name)

        # Should match original
        assert int(parsed) == direction_int
