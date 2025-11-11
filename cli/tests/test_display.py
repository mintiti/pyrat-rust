"""Unit tests for display logic."""

import pytest
from unittest.mock import Mock
from typing import List, Tuple, Dict

from pyrat_runner.display import (
    Display,
    RAT, PYTHON, RAT_AND_PYTHON, CHEESE,
    RAT_AND_CHEESE, PYTHON_AND_CHEESE, RAT_AND_PYTHON_AND_CHEESE, EMPTY,
    VERTICAL_WALL, VERTICAL_MUD, VERTICAL_NOTHING,
    HORIZONTAL_WALL, HORIZONTAL_MUD, HORIZONTAL_NOTHING
)


def create_mock_coordinate(x: int, y: int):
    """Create a mock coordinate that supports indexing."""
    coord = Mock()
    coord.__getitem__ = lambda self, i: [x, y][i]
    coord.__iter__ = lambda self: iter([x, y])
    return coord


@pytest.fixture
def mock_game():
    """Create a controlled mock game with known dimensions."""
    game = Mock()
    game._game = Mock()
    game._game.width = 5
    game._game.height = 5
    return game


@pytest.fixture
def empty_game(mock_game):
    """Game with no walls, mud, or cheese."""
    mock_game._game.wall_entries = Mock(return_value=[])
    mock_game.mud_positions = {}
    mock_game.player1_pos = create_mock_coordinate(0, 0)
    mock_game.player2_pos = create_mock_coordinate(4, 4)
    mock_game.cheese_positions = []
    return mock_game


@pytest.fixture
def game_with_walls(mock_game):
    """Game with specific walls configured."""
    # Vertical wall between (1,1) and (2,1)
    # Horizontal wall between (3,2) and (3,3)
    mock_game._game.wall_entries = Mock(return_value=[
        ((1, 1), (2, 1)),  # Vertical wall
        ((3, 2), (3, 3)),  # Horizontal wall
    ])
    mock_game.mud_positions = {}
    mock_game.player1_pos = create_mock_coordinate(0, 0)
    mock_game.player2_pos = create_mock_coordinate(4, 4)
    mock_game.cheese_positions = []
    return mock_game


@pytest.fixture
def game_with_mud(mock_game):
    """Game with specific mud configured."""
    mock_game._game.wall_entries = Mock(return_value=[])
    # Vertical mud between (1,2) and (2,2)
    # Horizontal mud between (3,1) and (3,2)
    mock_game.mud_positions = {
        (create_mock_coordinate(1, 2), create_mock_coordinate(2, 2)): 3,
        (create_mock_coordinate(3, 1), create_mock_coordinate(3, 2)): 2,
    }
    mock_game.player1_pos = create_mock_coordinate(0, 0)
    mock_game.player2_pos = create_mock_coordinate(4, 4)
    mock_game.cheese_positions = []
    return mock_game


class TestCellContent:
    """Test cell content determination logic."""

    @pytest.mark.parametrize("x,y,rat_pos,python_pos,cheese_set,expected", [
        # Empty cell - no players, no cheese
        (2, 2, (0, 0), (4, 4), set(), EMPTY),
        # Rat only at (1, 1)
        (1, 1, (1, 1), (4, 4), set(), RAT),
        # Python only at (3, 3)
        (3, 3, (0, 0), (3, 3), set(), PYTHON),
        # Cheese only at (2, 2)
        (2, 2, (0, 0), (4, 4), {(2, 2)}, CHEESE),
        # Rat and cheese at (1, 1)
        (1, 1, (1, 1), (4, 4), {(1, 1)}, RAT_AND_CHEESE),
        # Python and cheese at (3, 3)
        (3, 3, (0, 0), (3, 3), {(3, 3)}, PYTHON_AND_CHEESE),
        # Both players at (2, 2), no cheese
        (2, 2, (2, 2), (2, 2), set(), RAT_AND_PYTHON),
        # Both players and cheese at (2, 2)
        (2, 2, (2, 2), (2, 2), {(2, 2)}, RAT_AND_PYTHON_AND_CHEESE),
    ])
    def test_cell_content_combinations(self, empty_game, x, y, rat_pos, python_pos, cheese_set, expected):
        """Test all combinations of cell occupancy."""
        # Configure the game with specific positions
        empty_game.player1_pos = create_mock_coordinate(*rat_pos)
        empty_game.player2_pos = create_mock_coordinate(*python_pos)

        display = Display(empty_game, delay=0)
        content = display._get_cell_content(x, y, cheese_set)

        assert content == expected


class TestSeparators:
    """Test wall and mud separator determination logic."""

    @pytest.mark.parametrize("x,y,expected", [
        # Wall position from fixture: vertical wall between (1,1) and (2,1)
        # Stored at min_x = 1
        (1, 1, VERTICAL_WALL),
        # Position with no wall or mud
        (0, 0, VERTICAL_NOTHING),
        (3, 3, VERTICAL_NOTHING),
    ])
    def test_vertical_separator_with_walls(self, game_with_walls, x, y, expected):
        """Test vertical separators with known wall positions."""
        display = Display(game_with_walls, delay=0)
        assert display._get_vertical_separator(x, y) == expected

    @pytest.mark.parametrize("x,y,expected", [
        # Mud position from fixture: vertical mud between (1,2) and (2,2)
        # Stored at min_x = 1
        (1, 2, VERTICAL_MUD),
        # Position with no wall or mud
        (0, 0, VERTICAL_NOTHING),
        (4, 4, VERTICAL_NOTHING),
    ])
    def test_vertical_separator_with_mud(self, game_with_mud, x, y, expected):
        """Test vertical separators with known mud positions."""
        display = Display(game_with_mud, delay=0)
        assert display._get_vertical_separator(x, y) == expected

    @pytest.mark.parametrize("x,y,expected", [
        # Wall position from fixture: horizontal wall between (3,2) and (3,3)
        (3, 2, HORIZONTAL_WALL),
        # Position with no wall or mud
        (0, 0, HORIZONTAL_NOTHING),
        (1, 1, HORIZONTAL_NOTHING),
    ])
    def test_horizontal_separator_with_walls(self, game_with_walls, x, y, expected):
        """Test horizontal separators with known wall positions."""
        display = Display(game_with_walls, delay=0)
        assert display._get_horizontal_separator(x, y) == expected

    @pytest.mark.parametrize("x,y,expected", [
        # Mud position from fixture: horizontal mud between (3,1) and (3,2)
        (3, 1, HORIZONTAL_MUD),
        # Position with no wall or mud
        (0, 0, HORIZONTAL_NOTHING),
        (2, 2, HORIZONTAL_NOTHING),
    ])
    def test_horizontal_separator_with_mud(self, game_with_mud, x, y, expected):
        """Test horizontal separators with known mud positions."""
        display = Display(game_with_mud, delay=0)
        assert display._get_horizontal_separator(x, y) == expected


class TestMazeStructureBuilding:
    """Test that walls and mud are correctly parsed into display structures."""

    def test_horizontal_wall_parsing(self, game_with_walls):
        """Horizontal walls (same x, different y) should be added to h_walls."""
        # Wall between (3, 2) and (3, 3) - same column, different rows
        display = Display(game_with_walls, delay=0)
        # From fixture: ((3,2), (3,3)) is horizontal wall
        assert (3, 2) in display.h_walls, "Horizontal wall should be at (3, 2)"

    def test_vertical_wall_parsing(self, game_with_walls):
        """Vertical walls (different x, same y) should be added to v_walls."""
        # Wall between (1, 1) and (2, 1) - different columns, same row
        display = Display(game_with_walls, delay=0)
        # From fixture: ((1,1), (2,1)) is vertical wall
        assert (1, 1) in display.v_walls, "Vertical wall should be at (1, 1)"

    def test_horizontal_mud_parsing(self, game_with_mud):
        """Horizontal mud (same x, different y) should be added to h_mud."""
        # Mud between (3, 1) and (3, 2) - same column, different rows
        display = Display(game_with_mud, delay=0)
        # From fixture: mud between (3,1) and (3,2)
        assert (3, 1) in display.h_mud, "Horizontal mud should be at (3, 1)"

    def test_vertical_mud_parsing(self, game_with_mud):
        """Vertical mud (different x, same y) should be added to v_mud."""
        # Mud between (1, 2) and (2, 2) - different columns, same row
        display = Display(game_with_mud, delay=0)
        # From fixture: mud between (1,2) and (2,2)
        assert (1, 2) in display.v_mud, "Vertical mud should be at (1, 2)"

    def test_wall_order_independence(self, mock_game):
        """Wall parsing should work regardless of coordinate order."""
        # Test that ((x1, y1), (x2, y2)) and ((x2, y2), (x1, y1)) produce same result
        mock_game._game.wall_entries = Mock(return_value=[
            ((2, 1), (1, 1)),  # Reversed order vertical wall
        ])
        mock_game.mud_positions = {}

        display = Display(mock_game, delay=0)
        # Should still be stored with min x
        assert (1, 1) in display.v_walls, "Wall should be normalized to (1, 1)"

    def test_empty_maze_structures(self, empty_game):
        """Display with no walls or mud should have empty structure sets."""
        display = Display(empty_game, delay=0)

        assert len(display.h_walls) == 0, "Should have no horizontal walls"
        assert len(display.v_walls) == 0, "Should have no vertical walls"
        assert len(display.h_mud) == 0, "Should have no horizontal mud"
        assert len(display.v_mud) == 0, "Should have no vertical mud"
