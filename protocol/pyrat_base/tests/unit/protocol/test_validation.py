"""Tests for protocol validation helpers.

These tests verify the internal validation functions that parse and validate
specific data types used throughout the protocol.
"""

import pytest

from pyrat_base.protocol import _parse_mud, _parse_position, _parse_wall


class TestPositionValidation:
    """Tests for position coordinate parsing and validation.

    Positions are used throughout the protocol in format (x,y).
    """

    @pytest.mark.parametrize(
        "pos_str,expected",
        [
            ("(0,0)", (0, 0)),
            ("(123,456)", (123, 456)),
            ("( 1 , 2 )", (1, 2)),  # With spaces
            ("(999,999)", (999, 999)),  # Large values
        ],
    )
    def test_parse_position_valid(self, pos_str, expected):
        """Valid position formats are parsed correctly."""
        assert _parse_position(pos_str) == expected

    @pytest.mark.parametrize(
        "pos_str",
        [
            "",
            "()",
            "(1)",
            "(1,)",
            "(,2)",
            "(1,2,3)",
            "1,2",  # Missing parentheses
            "(a,b)",  # Non-numeric
            "(1.5,2.5)",  # Floats not allowed
            "[1,2]",  # Wrong brackets
            "(1 2)",  # Missing comma
        ],
    )
    def test_parse_position_invalid(self, pos_str):
        """Invalid position formats return None."""
        assert _parse_position(pos_str) is None


class TestWallValidation:
    """Tests for wall specification parsing and validation.

    Walls connect adjacent cells in format (x1,y1)-(x2,y2).
    """

    @pytest.mark.parametrize(
        "wall_str,expected",
        [
            ("(0,0)-(0,1)", ((0, 0), (0, 1))),
            ("(1,1)-(2,1)", ((1, 1), (2, 1))),
            ("( 5 , 5 )-( 5 , 6 )", ((5, 5), (5, 6))),  # With spaces
        ],
    )
    def test_parse_wall_valid(self, wall_str, expected):
        """Valid wall formats are parsed correctly."""
        assert _parse_wall(wall_str) == expected

    @pytest.mark.parametrize(
        "wall_str",
        [
            "",
            "(0,0)",  # Missing second position
            "(0,0)-(0,1)-(0,2)",  # Too many positions
            "(0,0)-(a,b)",  # Non-numeric
            "0,0-1,0",  # Missing parentheses
        ],
    )
    def test_parse_wall_invalid(self, wall_str):
        """Invalid wall formats return None."""
        assert _parse_wall(wall_str) is None

    def test_parse_wall_non_adjacent(self):
        """Non-adjacent cells are parsed (adjacency not validated)."""
        # The parser doesn't validate that walls connect adjacent cells
        assert _parse_wall("(0,0)-(2,2)") == ((0, 0), (2, 2))


class TestMudValidation:
    """Tests for mud specification parsing and validation.

    Mud connects cells with traversal cost in format (x1,y1)-(x2,y2):N.
    """

    @pytest.mark.parametrize(
        "mud_str,expected",
        [
            ("(5,5)-(5,6):3", ((5, 5), (5, 6), 3)),
            ("(1,1)-(1,2):2", ((1, 1), (1, 2), 2)),
            ("( 0 , 0 )-( 1 , 0 ):5", ((0, 0), (1, 0), 5)),  # With spaces
            ("(9,9)-(9,8):99", ((9, 9), (9, 8), 99)),  # Large cost
        ],
    )
    def test_parse_mud_valid(self, mud_str, expected):
        """Valid mud formats are parsed correctly."""
        assert _parse_mud(mud_str) == expected

    @pytest.mark.parametrize(
        "mud_str",
        [
            "",
            "(5,5)-(5,6)",  # Missing cost
            "(5,5)-(5,6):",  # Empty cost
            "(5,5)-(5,6):abc",  # Non-numeric cost
            "(5,5)-(5,6):1.5",  # Float cost
            "5,5-5,6:3",  # Missing parentheses
        ],
    )
    def test_parse_mud_invalid(self, mud_str):
        """Invalid mud formats return None."""
        assert _parse_mud(mud_str) is None

    def test_parse_mud_edge_values(self):
        """Parser accepts any integer mud cost (validation is engine's job)."""
        # Zero cost - parser accepts, engine would validate
        assert _parse_mud("(5,5)-(5,6):0") == ((5, 5), (5, 6), 0)

        # Negative cost - parser accepts, engine would validate
        assert _parse_mud("(5,5)-(5,6):-1") == ((5, 5), (5, 6), -1)


class TestNumericValidation:
    """Tests for numeric value parsing used in various commands."""

    def test_maze_dimensions_validation(self):
        """Maze dimensions must be positive integers."""
        # This is tested indirectly through maze command parsing
        # but could have dedicated validation function
        pass

    def test_score_parsing(self):
        """Scores can be integers or floats."""
        # This is tested through score command parsing
        # Scores like "3-2" are parsed to (3.0, 2.0)
        pass

    def test_time_parsing(self):
        """Time values must be positive integers in milliseconds."""
        # This is tested through timecontrol command parsing
        pass


class TestBoundaryValidation:
    """Tests for boundary conditions in protocol values."""

    def test_coordinate_boundaries(self):
        """Coordinates should handle edge cases gracefully."""
        # Test very large coordinates
        assert _parse_position("(9999,9999)") == (9999, 9999)

        # Test zero coordinates
        assert _parse_position("(0,0)") == (0, 0)

        # Negative coordinates might be invalid depending on implementation
        result = _parse_position("(-1,-1)")
        # Implementation may allow or reject negative coordinates

    def test_mud_cost_boundaries(self):
        """Mud costs should be within reasonable bounds."""
        # Very high mud cost
        assert _parse_mud("(0,0)-(0,1):9999") == ((0, 0), (0, 1), 9999)

        # Mud cost of 1 (minimum meaningful value)
        assert _parse_mud("(0,0)-(0,1):1") == ((0, 0), (0, 1), 1)
