"""Tests for unified position/coordinate types between Rust and Python.

Tests the new unified type system including:
- Coordinates type
- Wall type
- Mud type
- Tuple input support
- Type conversions
"""
# ruff: noqa: PLR2004

import pytest
from pyrat_engine import PyRat
from pyrat_engine.core.types import Coordinates, Direction, Mud, Wall


class TestCoordinatesType:
    """Test the Coordinates type from Rust."""

    def test_create_coordinates(self):
        """Test basic creation of Coordinates."""
        coords = Coordinates(5, 10)
        assert coords.x == 5
        assert coords.y == 10

    def test_coordinates_validation(self):
        """Test validation of coordinate values."""
        # Negative values should fail
        with pytest.raises(ValueError, match="cannot be negative"):
            Coordinates(-1, 0)

        with pytest.raises(ValueError, match="cannot be negative"):
            Coordinates(0, -1)

        # Values too large should fail
        with pytest.raises(ValueError, match="cannot exceed 255"):
            Coordinates(256, 0)

        with pytest.raises(ValueError, match="cannot exceed 255"):
            Coordinates(0, 256)

    def test_coordinates_methods(self):
        """Test methods on Coordinates."""
        coord1 = Coordinates(5, 5)

        # Test get_neighbor
        up = coord1.get_neighbor(0)  # Direction.Up
        assert up.x == 5
        assert up.y == 6

        right = coord1.get_neighbor(1)  # Direction.Right
        assert right.x == 6
        assert right.y == 5

        down = coord1.get_neighbor(2)  # Direction.Down
        assert down.x == 5
        assert down.y == 4

        left = coord1.get_neighbor(3)  # Direction.Left
        assert left.x == 4
        assert left.y == 5

        stay = coord1.get_neighbor(4)  # Direction.Stay
        assert stay.x == 5
        assert stay.y == 5

        # Invalid direction
        with pytest.raises(ValueError):
            coord1.get_neighbor(5)

    def test_is_adjacent_to(self):
        """Test adjacency checking."""
        coord1 = Coordinates(5, 5)

        # Adjacent positions
        assert coord1.is_adjacent_to(Coordinates(5, 6))  # Up
        assert coord1.is_adjacent_to(Coordinates(5, 4))  # Down
        assert coord1.is_adjacent_to(Coordinates(4, 5))  # Left
        assert coord1.is_adjacent_to(Coordinates(6, 5))  # Right

        # Non-adjacent positions
        assert not coord1.is_adjacent_to(Coordinates(5, 5))  # Same position
        assert not coord1.is_adjacent_to(Coordinates(6, 6))  # Diagonal
        assert not coord1.is_adjacent_to(Coordinates(7, 5))  # Too far

    def test_manhattan_distance(self):
        """Test Manhattan distance calculation."""
        coord1 = Coordinates(0, 0)
        coord2 = Coordinates(3, 4)

        assert coord1.manhattan_distance(coord2) == 7
        assert coord2.manhattan_distance(coord1) == 7  # Symmetric
        assert coord1.manhattan_distance(coord1) == 0  # Same position

    def test_string_representations(self):
        """Test string representations."""
        coord = Coordinates(5, 10)

        assert str(coord) == "(5, 10)"
        assert repr(coord) == "Coordinates(5, 10)"

    def test_iteration(self):
        """Test that Coordinates can be iterated/unpacked."""
        coord = Coordinates(5, 10)
        x, y = coord
        assert x == 5
        assert y == 10

    def test_hashable(self):
        """Test that Coordinates are hashable."""
        coord1 = Coordinates(5, 10)
        coord2 = Coordinates(5, 10)
        coord3 = Coordinates(10, 5)

        # Can be used in sets
        coord_set = {coord1, coord2, coord3}
        assert len(coord_set) == 2  # coord1 and coord2 are equal

        # Can be used as dict keys
        coord_dict = {coord1: "test"}
        assert coord_dict[coord2] == "test"


class TestWallType:
    """Test the Wall type from Rust."""

    def test_create_wall(self):
        """Test basic creation of Wall."""
        pos1 = Coordinates(0, 0)
        pos2 = Coordinates(0, 1)

        wall = Wall(pos1, pos2)
        assert wall.pos1 == pos1
        assert wall.pos2 == pos2

    def test_wall_validation(self):
        """Test wall validation rules."""
        # Non-adjacent positions should fail
        pos1 = Coordinates(0, 0)
        pos2 = Coordinates(2, 2)

        with pytest.raises(ValueError, match="must be adjacent"):
            Wall(pos1, pos2)

    def test_wall_normalization(self):
        """Test that walls are normalized (smaller position first)."""
        pos1 = Coordinates(1, 0)
        pos2 = Coordinates(0, 0)

        wall1 = Wall(pos1, pos2)
        wall2 = Wall(pos2, pos1)

        # Both should have the same normalized order
        assert wall1.pos1 == wall2.pos1
        assert wall1.pos2 == wall2.pos2
        assert wall1.pos1.x == 0 and wall1.pos1.y == 0
        assert wall1.pos2.x == 1 and wall1.pos2.y == 0

    def test_blocks_movement(self):
        """Test blocks_movement method."""
        wall = Wall(Coordinates(0, 0), Coordinates(0, 1))

        # Should block movement between the two positions
        assert wall.blocks_movement(Coordinates(0, 0), Coordinates(0, 1))
        assert wall.blocks_movement(Coordinates(0, 1), Coordinates(0, 0))

        # Should not block unrelated movements
        assert not wall.blocks_movement(Coordinates(1, 0), Coordinates(1, 1))
        assert not wall.blocks_movement(Coordinates(0, 0), Coordinates(1, 0))

    def test_wall_repr(self):
        """Test wall string representation."""
        wall = Wall(Coordinates(0, 0), Coordinates(0, 1))
        assert repr(wall) == "Wall(Coordinates(0, 0), Coordinates(0, 1))"


class TestMudType:
    """Test the Mud type from Rust."""

    def test_create_mud(self):
        """Test basic creation of Mud."""
        pos1 = Coordinates(0, 0)
        pos2 = Coordinates(0, 1)

        mud = Mud(pos1, pos2, 3)
        assert mud.pos1 == pos1
        assert mud.pos2 == pos2
        assert mud.value == 3

    def test_mud_validation(self):
        """Test mud validation rules."""
        pos1 = Coordinates(0, 0)
        pos2 = Coordinates(0, 1)

        # Mud value must be at least 2
        with pytest.raises(ValueError, match="at least 2"):
            Mud(pos1, pos2, 1)

        # Non-adjacent positions should fail
        pos3 = Coordinates(2, 2)
        with pytest.raises(ValueError, match="must be adjacent"):
            Mud(pos1, pos3, 3)

    def test_mud_normalization(self):
        """Test that mud positions are normalized."""
        pos1 = Coordinates(1, 0)
        pos2 = Coordinates(0, 0)

        mud1 = Mud(pos1, pos2, 3)
        mud2 = Mud(pos2, pos1, 3)

        # Both should have the same normalized order
        assert mud1.pos1 == mud2.pos1
        assert mud1.pos2 == mud2.pos2
        assert mud1.value == mud2.value

    def test_affects_movement(self):
        """Test affects_movement method."""
        mud = Mud(Coordinates(0, 0), Coordinates(0, 1), 3)

        # Should affect movement between the two positions
        assert mud.affects_movement(Coordinates(0, 0), Coordinates(0, 1))
        assert mud.affects_movement(Coordinates(0, 1), Coordinates(0, 0))

        # Should not affect unrelated movements
        assert not mud.affects_movement(Coordinates(1, 0), Coordinates(1, 1))

    def test_mud_repr(self):
        """Test mud string representation."""
        mud = Mud(Coordinates(0, 0), Coordinates(0, 1), 3)
        assert repr(mud) == "Mud(Coordinates(0, 0), Coordinates(0, 1), value=3)"


class TestTupleInputSupport:
    """Test that tuple input is supported for backward compatibility."""

    def test_game_accepts_tuple_walls(self):
        """Test that PyRat.create_custom accepts tuple walls."""
        # Should be able to pass tuples which get converted to Wall objects
        walls = [((0, 0), (0, 1)), ((1, 1), (2, 1))]

        game = PyRat.create_custom(
            width=5,
            height=5,
            walls=walls,
            cheese=[(2, 2)],
            max_turns=100,
            symmetric=False,
        )

        assert game is not None
        assert game.width == 5
        assert game.height == 5

    def test_game_accepts_coordinates_walls(self):
        """Test that PyRat.create_custom accepts Coordinates-based walls."""
        # Can also pass Wall objects directly
        walls = [
            Wall(Coordinates(0, 0), Coordinates(0, 1)),
            Wall(Coordinates(1, 1), Coordinates(2, 1)),
        ]

        # This should fail for now since create_custom expects tuples
        # We'll update this test when we implement the FromPyObject trait
        with pytest.raises(TypeError):
            PyRat.create_custom(
                width=5, height=5, walls=walls, cheese=[(2, 2)], max_turns=100
            )

    def test_create_from_maze_with_tuples(self):
        """Test create_from_maze accepts tuple walls."""
        walls = [((0, 0), (0, 1)), ((1, 1), (2, 1))]

        game = PyRat.create_from_maze(
            width=5, height=5, walls=walls, seed=42, max_turns=100, symmetric=False
        )

        assert game is not None
        assert game.width == 5
        assert game.height == 5


class TestDirectionEnum:
    """Test the Direction IntEnum."""

    def test_direction_values(self):
        """Test that Direction enum is accessible."""
        # Direction is now a Python IntEnum
        assert Direction.UP == 0
        assert Direction.RIGHT == 1
        assert Direction.DOWN == 2
        assert Direction.LEFT == 3
        assert Direction.STAY == 4


class TestIntegrationWithGame:
    """Test integration of unified types with the game."""

    def test_game_returns_coordinates(self):
        """Test that game methods return Coordinates objects."""
        # Use even cheese count with even dimensions
        game = PyRat(width=10, height=10, cheese_count=6)

        # Get player positions - should return Coordinates objects
        p1_pos = game.player1_position
        p2_pos = game.player2_position

        assert isinstance(p1_pos, Coordinates)
        assert isinstance(p2_pos, Coordinates)
        assert p1_pos.x == 0
        assert p1_pos.y == 0
        assert p2_pos.x == 9
        assert p2_pos.y == 9

        # Test that they can still be unpacked like tuples
        x1, y1 = p1_pos
        assert x1 == 0
        assert y1 == 0

    def test_cheese_positions_are_coordinates(self):
        """Test that cheese positions return Coordinates objects."""
        # Use even cheese count with even dimensions
        game = PyRat(width=10, height=10, cheese_count=6)

        cheese_positions = game.cheese_positions()
        assert len(cheese_positions) == 6

        for pos in cheese_positions:
            assert isinstance(pos, Coordinates)
            assert 0 <= pos.x < 10
            assert 0 <= pos.y < 10

            # Test unpacking still works
            x, y = pos
            assert x == pos.x
            assert y == pos.y


class TestCoordinatesArithmetic:
    """Test Coordinates arithmetic operations."""

    def test_add_tuple_positive(self):
        """Test adding positive tuple delta."""
        coord = Coordinates(5, 5)
        result = coord + (2, 3)  # noqa: RUF005 - this is delta addition, not tuple concatenation
        assert result.x == 7
        assert result.y == 8

    def test_add_tuple_negative(self):
        """Test adding negative tuple delta."""
        coord = Coordinates(5, 5)
        result = coord + (-2, -3)  # noqa: RUF005 - this is delta addition, not tuple concatenation
        assert result.x == 3
        assert result.y == 2

    def test_add_tuple_saturates_at_zero(self):
        """Test that subtraction saturates at 0."""
        coord = Coordinates(5, 5)
        result = coord + (-10, -10)  # noqa: RUF005 - this is delta addition, not tuple concatenation
        assert result.x == 0
        assert result.y == 0

    def test_add_tuple_saturates_at_max(self):
        """Test that addition saturates at 255."""
        coord = Coordinates(250, 250)
        result = coord + (10, 10)  # noqa: RUF005 - this is delta addition, not tuple concatenation
        assert result.x == 255
        assert result.y == 255

    def test_add_direction(self):
        """Test adding Direction."""
        coord = Coordinates(5, 5)

        # Using Direction enum
        assert (coord + Direction.UP).y == 6
        assert (coord + Direction.DOWN).y == 4
        assert (coord + Direction.LEFT).x == 4
        assert (coord + Direction.RIGHT).x == 6
        assert (coord + Direction.STAY) == coord

    def test_add_direction_at_boundary(self):
        """Test Direction addition at boundaries saturates."""
        # At bottom boundary
        coord = Coordinates(5, 0)
        result = coord + Direction.DOWN
        assert result.y == 0  # Saturates

        # At top boundary
        coord = Coordinates(5, 255)
        result = coord + Direction.UP
        assert result.y == 255  # Saturates

    def test_add_invalid_direction_raises(self):
        """Test that invalid direction value raises ValueError."""
        coord = Coordinates(5, 5)
        with pytest.raises(ValueError, match="Invalid direction"):
            coord + 5  # 5 is not a valid direction

    def test_sub_coordinates(self):
        """Test subtracting Coordinates returns signed tuple."""
        coord1 = Coordinates(5, 5)
        coord2 = Coordinates(3, 8)

        delta = coord1 - coord2
        assert delta == (2, -3)

        delta = coord2 - coord1
        assert delta == (-2, 3)

    def test_sub_tuple(self):
        """Test subtracting tuple from Coordinates."""
        coord = Coordinates(5, 5)
        delta = coord - (3, 8)
        assert delta == (2, -3)

    def test_sub_returns_tuple_not_coordinates(self):
        """Verify subtraction returns tuple, not Coordinates."""
        coord1 = Coordinates(5, 5)
        coord2 = Coordinates(3, 3)
        result = coord1 - coord2
        assert isinstance(result, tuple)
        assert len(result) == 2


class TestWallIteration:
    """Test Wall iteration support."""

    def test_wall_unpacking(self):
        """Test that Wall can be unpacked."""
        wall = Wall(Coordinates(0, 0), Coordinates(0, 1))
        pos1, pos2 = wall

        assert isinstance(pos1, Coordinates)
        assert isinstance(pos2, Coordinates)
        assert pos1 == Coordinates(0, 0)
        assert pos2 == Coordinates(0, 1)

    def test_wall_len(self):
        """Test that len(wall) returns 2."""
        wall = Wall(Coordinates(0, 0), Coordinates(0, 1))
        assert len(wall) == 2

    def test_wall_iteration(self):
        """Test iterating over Wall."""
        wall = Wall(Coordinates(0, 0), Coordinates(0, 1))
        positions = list(wall)

        assert len(positions) == 2
        assert positions[0] == Coordinates(0, 0)
        assert positions[1] == Coordinates(0, 1)


class TestMudIteration:
    """Test Mud iteration support."""

    def test_mud_unpacking(self):
        """Test that Mud can be unpacked."""
        mud = Mud(Coordinates(0, 0), Coordinates(0, 1), 3)
        pos1, pos2, value = mud

        assert isinstance(pos1, Coordinates)
        assert isinstance(pos2, Coordinates)
        assert isinstance(value, int)
        assert pos1 == Coordinates(0, 0)
        assert pos2 == Coordinates(0, 1)
        assert value == 3

    def test_mud_len(self):
        """Test that len(mud) returns 3."""
        mud = Mud(Coordinates(0, 0), Coordinates(0, 1), 3)
        assert len(mud) == 3

    def test_mud_iteration(self):
        """Test iterating over Mud."""
        mud = Mud(Coordinates(0, 0), Coordinates(0, 1), 5)
        items = list(mud)

        assert len(items) == 3
        assert items[0] == Coordinates(0, 0)
        assert items[1] == Coordinates(0, 1)
        assert items[2] == 5
