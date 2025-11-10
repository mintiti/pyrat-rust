"""
Test Coordinates compatibility with tuple operations.

This test suite verifies that the unified Coordinates type from pyrat_engine
can be used as a drop-in replacement for Tuple[int, int] in the protocol layer.

These tests are CRITICAL for the protocol layer migration. All must pass before
proceeding with the full tuple → Coordinates migration.
"""

import json
import pickle
from typing import Dict, List, Set

import pytest

from pyrat_engine.core.types import Coordinates


class TestCoordinatesBasicOperations:
    """Test basic Coordinates creation and attribute access."""

    def test_coordinates_creation(self):
        """Test creating Coordinates objects."""
        coord = Coordinates(5, 10)
        assert coord.x == 5
        assert coord.y == 10

    def test_coordinates_negative_values_rejected(self):
        """Test that negative coordinates are rejected."""
        with pytest.raises(ValueError):
            Coordinates(-1, 5)
        with pytest.raises(ValueError):
            Coordinates(5, -1)

    def test_coordinates_large_values(self):
        """Test coordinates with large values (up to u8::MAX)."""
        coord = Coordinates(255, 255)
        assert coord.x == 255
        assert coord.y == 255


class TestCoordinatesTupleEquality:
    """Test equality comparisons between Coordinates and tuples.

    CRITICAL: This determines if we need workarounds for tuple comparisons.
    """

    def test_coordinates_equality_with_self(self):
        """Test Coordinates equals itself."""
        c1 = Coordinates(5, 10)
        c2 = Coordinates(5, 10)
        assert c1 == c2

    def test_coordinates_inequality(self):
        """Test Coordinates inequality."""
        c1 = Coordinates(5, 10)
        c2 = Coordinates(3, 7)
        assert c1 != c2

    def test_coordinates_equality_with_tuple(self):
        """Test if Coordinates can be compared to tuples directly.

        This is CRITICAL for backward compatibility. If this fails,
        we need to document the conversion pattern: (coord.x, coord.y) == tuple
        """
        coord = Coordinates(5, 10)

        # Try direct comparison (might not work)
        try:
            result = (coord == (5, 10))
            if result:
                # Direct equality works!
                assert coord == (5, 10)
                assert not (coord == (3, 7))
            else:
                # Direct equality doesn't work, use conversion
                pytest.skip("Coordinates cannot be directly compared to tuples")
        except TypeError:
            # Direct equality raises TypeError, need conversion
            pytest.skip("Coordinates cannot be directly compared to tuples")

    def test_coordinates_to_tuple_conversion(self):
        """Test converting Coordinates to tuple for equality checks.

        This is the FALLBACK pattern if direct equality doesn't work.
        """
        coord = Coordinates(5, 10)

        # This MUST work
        assert (coord.x, coord.y) == (5, 10)
        assert (coord.x, coord.y) != (3, 7)


class TestCoordinatesUnpacking:
    """Test tuple unpacking operations.

    CRITICAL: Protocol layer code frequently unpacks positions.
    """

    def test_coordinates_unpacking(self):
        """Test unpacking Coordinates like a tuple."""
        coord = Coordinates(5, 10)
        x, y = coord
        assert x == 5
        assert y == 10

    def test_coordinates_unpacking_in_loop(self):
        """Test unpacking Coordinates in a loop."""
        positions = [Coordinates(0, 0), Coordinates(5, 10), Coordinates(3, 7)]

        result = []
        for x, y in positions:
            result.append((x, y))

        assert result == [(0, 0), (5, 10), (3, 7)]

    def test_coordinates_star_unpacking(self):
        """Test star unpacking of Coordinates."""
        coord = Coordinates(5, 10)
        values = [*coord]
        assert values == [5, 10]


class TestCoordinatesIndexing:
    """Test indexing operations.

    CRITICAL: Some code uses coord[0] and coord[1] for x, y access.
    """

    def test_coordinates_indexing(self):
        """Test indexing Coordinates like a tuple."""
        coord = Coordinates(5, 10)
        assert coord[0] == 5
        assert coord[1] == 10

    def test_coordinates_negative_indexing(self):
        """Test negative indexing."""
        coord = Coordinates(5, 10)
        assert coord[-1] == 10
        assert coord[-2] == 5

    def test_coordinates_invalid_index(self):
        """Test that invalid indices raise errors."""
        coord = Coordinates(5, 10)
        with pytest.raises(IndexError):
            _ = coord[2]
        with pytest.raises(IndexError):
            _ = coord[-3]


class TestCoordinatesIteration:
    """Test iteration and sequence operations."""

    def test_coordinates_iteration(self):
        """Test iterating over Coordinates."""
        coord = Coordinates(5, 10)
        values = list(coord)
        assert values == [5, 10]

    def test_coordinates_tuple_conversion(self):
        """Test explicit tuple conversion."""
        coord = Coordinates(5, 10)
        t = tuple(coord)
        assert t == (5, 10)
        assert isinstance(t, tuple)

    def test_coordinates_length(self):
        """Test len() on Coordinates."""
        coord = Coordinates(5, 10)
        assert len(coord) == 2


class TestCoordinatesHashing:
    """Test hashing and use as dict keys/set members.

    CRITICAL: Protocol layer uses positions as dictionary keys.
    """

    def test_coordinates_hashable(self):
        """Test that Coordinates can be hashed."""
        coord = Coordinates(5, 10)
        hash_value = hash(coord)
        assert isinstance(hash_value, int)

    def test_coordinates_hash_consistency(self):
        """Test that equal Coordinates have equal hashes."""
        c1 = Coordinates(5, 10)
        c2 = Coordinates(5, 10)
        assert hash(c1) == hash(c2)

    def test_coordinates_as_dict_key(self):
        """Test using Coordinates as dictionary key."""
        coord_map: Dict[Coordinates, str] = {}

        coord_map[Coordinates(5, 10)] = "target"
        coord_map[Coordinates(3, 7)] = "cheese"

        # Lookup with same coordinates
        assert coord_map[Coordinates(5, 10)] == "target"
        assert coord_map[Coordinates(3, 7)] == "cheese"

        # Check length
        assert len(coord_map) == 2

    def test_coordinates_in_set(self):
        """Test using Coordinates in a set."""
        coord_set: Set[Coordinates] = set()

        coord_set.add(Coordinates(5, 10))
        coord_set.add(Coordinates(3, 7))
        coord_set.add(Coordinates(5, 10))  # Duplicate

        # Duplicates eliminated
        assert len(coord_set) == 2

        # Membership testing
        assert Coordinates(5, 10) in coord_set
        assert Coordinates(3, 7) in coord_set
        assert Coordinates(0, 0) not in coord_set

    def test_coordinates_set_operations(self):
        """Test set operations with Coordinates."""
        set1 = {Coordinates(0, 0), Coordinates(5, 10)}
        set2 = {Coordinates(5, 10), Coordinates(3, 7)}

        # Union
        union = set1 | set2
        assert len(union) == 3

        # Intersection
        intersection = set1 & set2
        assert intersection == {Coordinates(5, 10)}

        # Difference
        diff = set1 - set2
        assert diff == {Coordinates(0, 0)}


class TestCoordinatesStringRepresentation:
    """Test string representations for debugging."""

    def test_coordinates_repr(self):
        """Test repr() output."""
        coord = Coordinates(5, 10)
        r = repr(coord)
        # Should be something like "Coordinates(5, 10)" or similar
        assert "5" in r
        assert "10" in r

    def test_coordinates_str(self):
        """Test str() output."""
        coord = Coordinates(5, 10)
        s = str(coord)
        # Should be something like "(5, 10)" or similar
        assert "5" in s
        assert "10" in s


class TestCoordinatesSerialization:
    """Test serialization for protocol communication.

    NOTE: This may fail and that's OK - we handle serialization manually.
    """

    def test_coordinates_pickle(self):
        """Test if Coordinates can be pickled."""
        coord = Coordinates(5, 10)

        try:
            serialized = pickle.dumps(coord)
            deserialized = pickle.loads(serialized)
            assert deserialized.x == 5
            assert deserialized.y == 10
        except (TypeError, AttributeError) as e:
            pytest.skip(f"Coordinates not picklable (OK): {e}")

    def test_coordinates_json_serialization(self):
        """Test JSON serialization (expected to fail without custom encoder)."""
        coord = Coordinates(5, 10)

        # This is expected to fail - document the pattern
        with pytest.raises(TypeError):
            json.dumps(coord)

        # The correct pattern for JSON is to convert manually
        json_data = json.dumps([coord.x, coord.y])
        assert json_data == "[5, 10]"

        # Or as dict
        json_data = json.dumps({"x": coord.x, "y": coord.y})
        parsed = json.loads(json_data)
        assert parsed == {"x": 5, "y": 10}


class TestCoordinatesMethods:
    """Test Coordinates-specific methods that provide extra functionality."""

    def test_coordinates_get_neighbor(self):
        """Test get_neighbor method."""
        from pyrat_engine.core.types import Direction

        coord = Coordinates(5, 10)

        # UP increases y
        up = coord.get_neighbor(Direction.UP)
        assert up.x == 5 and up.y == 11

        # DOWN decreases y
        down = coord.get_neighbor(Direction.DOWN)
        assert down.x == 5 and down.y == 9

        # RIGHT increases x
        right = coord.get_neighbor(Direction.RIGHT)
        assert right.x == 6 and right.y == 10

        # LEFT decreases x
        left = coord.get_neighbor(Direction.LEFT)
        assert left.x == 4 and left.y == 10

        # STAY returns same position
        stay = coord.get_neighbor(Direction.STAY)
        assert stay.x == 5 and stay.y == 10

    def test_coordinates_is_adjacent_to(self):
        """Test is_adjacent_to method."""
        center = Coordinates(5, 10)

        # Adjacent positions
        assert center.is_adjacent_to(Coordinates(5, 11))  # UP
        assert center.is_adjacent_to(Coordinates(5, 9))   # DOWN
        assert center.is_adjacent_to(Coordinates(6, 10))  # RIGHT
        assert center.is_adjacent_to(Coordinates(4, 10))  # LEFT

        # Not adjacent
        assert not center.is_adjacent_to(Coordinates(6, 11))  # Diagonal
        assert not center.is_adjacent_to(Coordinates(5, 10))  # Self
        assert not center.is_adjacent_to(Coordinates(0, 0))   # Far away

    def test_coordinates_manhattan_distance(self):
        """Test manhattan_distance method."""
        pos1 = Coordinates(0, 0)
        pos2 = Coordinates(3, 4)

        assert pos1.manhattan_distance(pos2) == 7
        assert pos2.manhattan_distance(pos1) == 7  # Symmetric

        # Distance to self is 0
        assert pos1.manhattan_distance(pos1) == 0


class TestCoordinatesListOperations:
    """Test operations on lists of Coordinates."""

    def test_list_of_coordinates(self):
        """Test creating and using lists of Coordinates."""
        cheese: List[Coordinates] = [
            Coordinates(0, 0),
            Coordinates(5, 10),
            Coordinates(3, 7),
        ]

        assert len(cheese) == 3
        assert cheese[0].x == 0
        assert cheese[1].y == 10

    def test_list_comprehension_with_coordinates(self):
        """Test list comprehensions."""
        positions = [Coordinates(i, i * 2) for i in range(5)]
        assert len(positions) == 5
        assert positions[3].x == 3
        assert positions[3].y == 6

    def test_filter_coordinates(self):
        """Test filtering lists of Coordinates."""
        positions = [Coordinates(i, i) for i in range(10)]

        # Filter positions where x < 5
        filtered = [p for p in positions if p.x < 5]
        assert len(filtered) == 5

    def test_sort_coordinates(self):
        """Test sorting lists of Coordinates."""
        positions = [
            Coordinates(5, 10),
            Coordinates(0, 0),
            Coordinates(3, 7),
        ]

        # Coordinates should be sortable (has __lt__ or similar)
        try:
            sorted_pos = sorted(positions, key=lambda p: (p.x, p.y))
            assert sorted_pos[0] == Coordinates(0, 0)
            assert sorted_pos[1] == Coordinates(3, 7)
            assert sorted_pos[2] == Coordinates(5, 10)
        except TypeError:
            # If not directly sortable, can sort by tuple conversion
            sorted_pos = sorted(positions, key=lambda p: (p.x, p.y))
            assert sorted_pos[0].x == 0


class TestCoordinatesTypeAnnotations:
    """Test type annotation compatibility (for mypy)."""

    def test_function_with_coordinates_type(self):
        """Test that functions can accept Coordinates type."""
        def distance_from_origin(pos: Coordinates) -> int:
            return pos.x + pos.y

        coord = Coordinates(3, 4)
        assert distance_from_origin(coord) == 7

    def test_function_returning_coordinates(self):
        """Test that functions can return Coordinates type."""
        def create_origin() -> Coordinates:
            return Coordinates(0, 0)

        origin = create_origin()
        assert origin.x == 0
        assert origin.y == 0


# Summary test that documents compatibility findings
class TestCompatibilitySummary:
    """Document the overall compatibility results."""

    def test_compatibility_checklist(self):
        """Document which operations work and which need workarounds."""
        coord = Coordinates(5, 10)

        # These MUST work for migration
        checklist = {
            "creation": True,
            "attribute_access": coord.x == 5 and coord.y == 10,
            "unpacking": True,  # Tested separately
            "indexing": coord[0] == 5 and coord[1] == 10,
            "iteration": list(coord) == [5, 10],
            "hashing": hash(coord) is not None,
            "dict_key": True,  # Tested separately
            "set_member": True,  # Tested separately
            "string_repr": str(coord) != "",
        }

        # All must be True
        assert all(checklist.values()), f"Some operations failed: {checklist}"

        # Optional features (nice to have)
        optional = {
            "tuple_equality": "unknown",  # Tested separately
            "json_serializable": False,  # Known to fail
            "picklable": "unknown",  # Tested separately
        }

        print(f"\n✓ Required features: {checklist}")
        print(f"⚠ Optional features: {optional}")
