"""Unit tests for display logic."""

import pytest
from unittest.mock import Mock, MagicMock
from pyrat_engine import PyRat

from pyrat_runner.display import (
    Display,
    RAT, PYTHON, RAT_AND_PYTHON, CHEESE,
    RAT_AND_CHEESE, PYTHON_AND_CHEESE, RAT_AND_PYTHON_AND_CHEESE, EMPTY,
    VERTICAL_WALL, VERTICAL_MUD, VERTICAL_NOTHING,
    HORIZONTAL_WALL, HORIZONTAL_MUD, HORIZONTAL_NOTHING
)


class TestCellContent:
    """Test cell content determination logic."""

    def setup_method(self):
        """Create a game state for testing."""
        self.game = PyRat(width=5, height=5, cheese_count=2, seed=42)
        self.display = Display(self.game, delay=0)

    def test_empty_cell(self):
        """Empty cell should return EMPTY."""
        cheese_set = set()
        # Assuming players are not at (2, 2)
        content = self.display._get_cell_content(2, 2, cheese_set)
        assert content == EMPTY

    def test_rat_only(self):
        """Cell with only rat should return RAT."""
        cheese_set = set()
        rat_pos = self.game.player1_pos
        content = self.display._get_cell_content(rat_pos[0], rat_pos[1], cheese_set)
        assert content == RAT

    def test_python_only(self):
        """Cell with only python should return PYTHON."""
        cheese_set = set()
        python_pos = self.game.player2_pos
        content = self.display._get_cell_content(python_pos[0], python_pos[1], cheese_set)
        assert content == PYTHON

    def test_cheese_only(self):
        """Cell with only cheese should return CHEESE."""
        cheese_positions = self.game.cheese_positions
        if cheese_positions:
            cheese_pos = cheese_positions[0]
            cheese_set = {(cheese_pos[0], cheese_pos[1])}
            # Make sure no player is there
            rat_pos = self.game.player1_pos
            python_pos = self.game.player2_pos
            if (cheese_pos[0] != rat_pos[0] or cheese_pos[1] != rat_pos[1]) and \
               (cheese_pos[0] != python_pos[0] or cheese_pos[1] != python_pos[1]):
                content = self.display._get_cell_content(cheese_pos[0], cheese_pos[1], cheese_set)
                assert content == CHEESE

    def test_rat_and_cheese(self):
        """Cell with rat and cheese should return RAT_AND_CHEESE."""
        rat_pos = self.game.player1_pos
        cheese_set = {(rat_pos[0], rat_pos[1])}
        content = self.display._get_cell_content(rat_pos[0], rat_pos[1], cheese_set)
        assert content == RAT_AND_CHEESE

    def test_python_and_cheese(self):
        """Cell with python and cheese should return PYTHON_AND_CHEESE."""
        python_pos = self.game.player2_pos
        cheese_set = {(python_pos[0], python_pos[1])}
        content = self.display._get_cell_content(python_pos[0], python_pos[1], cheese_set)
        assert content == PYTHON_AND_CHEESE

    def test_rat_and_python(self):
        """Cell with both players should return RAT_AND_PYTHON."""
        # Create a mock where both players are at same position
        mock_game = Mock()
        mock_game.player1_pos = (2, 2)
        mock_game.player2_pos = (2, 2)
        mock_game._game = Mock()
        mock_game._game.width = 5
        mock_game._game.height = 5
        mock_game._game.wall_entries = Mock(return_value=[])
        mock_game.mud_positions = {}

        display = Display(mock_game, delay=0)
        cheese_set = set()
        content = display._get_cell_content(2, 2, cheese_set)
        assert content == RAT_AND_PYTHON

    def test_rat_and_python_and_cheese(self):
        """Cell with both players and cheese should return RAT_AND_PYTHON_AND_CHEESE."""
        # Create a mock where both players are at same position with cheese
        mock_game = Mock()
        mock_game.player1_pos = (2, 2)
        mock_game.player2_pos = (2, 2)
        mock_game._game = Mock()
        mock_game._game.width = 5
        mock_game._game.height = 5
        mock_game._game.wall_entries = Mock(return_value=[])
        mock_game.mud_positions = {}

        display = Display(mock_game, delay=0)
        cheese_set = {(2, 2)}
        content = display._get_cell_content(2, 2, cheese_set)
        assert content == RAT_AND_PYTHON_AND_CHEESE


class TestVerticalSeparator:
    """Test vertical separator determination logic."""

    def setup_method(self):
        """Create a display with known walls and mud."""
        mock_game = Mock()
        mock_game._game = Mock()
        mock_game._game.width = 5
        mock_game._game.height = 5
        mock_game._game.wall_entries = Mock(return_value=[
            ((1, 1), (2, 1)),  # Vertical wall at x=2, y=1
        ])
        mock_game.mud_positions = {
            (Mock(x=3, y=2, __getitem__=lambda s, i: [3, 2][i]),
             Mock(x=4, y=2, __getitem__=lambda s, i: [4, 2][i])): 2  # Vertical mud at x=4, y=2
        }

        self.display = Display(mock_game, delay=0)

    def test_vertical_wall(self):
        """Position with wall should return VERTICAL_WALL."""
        self.display.v_walls.add((2, 1))
        assert self.display._get_vertical_separator(2, 1) == VERTICAL_WALL

    def test_vertical_mud(self):
        """Position with mud should return VERTICAL_MUD."""
        self.display.v_mud.add((4, 2))
        assert self.display._get_vertical_separator(4, 2) == VERTICAL_MUD

    def test_vertical_nothing(self):
        """Position with no wall or mud should return VERTICAL_NOTHING."""
        # Test a position that has no wall or mud
        assert self.display._get_vertical_separator(0, 0) == VERTICAL_NOTHING


class TestHorizontalSeparator:
    """Test horizontal separator determination logic."""

    def setup_method(self):
        """Create a display with known walls and mud."""
        mock_game = Mock()
        mock_game._game = Mock()
        mock_game._game.width = 5
        mock_game._game.height = 5
        mock_game._game.wall_entries = Mock(return_value=[
            ((1, 1), (1, 2)),  # Horizontal wall at x=1, y=1
        ])
        mock_game.mud_positions = {
            (Mock(x=3, y=2, __getitem__=lambda s, i: [3, 2][i]),
             Mock(x=3, y=3, __getitem__=lambda s, i: [3, 3][i])): 2  # Horizontal mud at x=3, y=2
        }

        self.display = Display(mock_game, delay=0)

    def test_horizontal_wall(self):
        """Position with wall should return HORIZONTAL_WALL."""
        self.display.h_walls.add((1, 1))
        assert self.display._get_horizontal_separator(1, 1) == HORIZONTAL_WALL

    def test_horizontal_mud(self):
        """Position with mud should return HORIZONTAL_MUD."""
        self.display.h_mud.add((3, 2))
        assert self.display._get_horizontal_separator(3, 2) == HORIZONTAL_MUD

    def test_horizontal_nothing(self):
        """Position with no wall or mud should return HORIZONTAL_NOTHING."""
        assert self.display._get_horizontal_separator(2, 2) == HORIZONTAL_NOTHING


class TestMazeStructureBuilding:
    """Test that walls and mud are correctly parsed into display structures."""

    def test_horizontal_wall_parsing(self):
        """Horizontal walls should be added to h_walls."""
        mock_game = Mock()
        mock_game._game = Mock()
        mock_game._game.width = 5
        mock_game._game.height = 5
        # Wall between (2, 1) and (2, 2) - same x, different y
        mock_game._game.wall_entries = Mock(return_value=[
            ((2, 1), (2, 2)),
        ])
        mock_game.mud_positions = {}

        display = Display(mock_game, delay=0)
        assert (2, 1) in display.h_walls

    def test_vertical_wall_parsing(self):
        """Vertical walls should be added to v_walls."""
        mock_game = Mock()
        mock_game._game = Mock()
        mock_game._game.width = 5
        mock_game._game.height = 5
        # Wall between (1, 2) and (2, 2) - different x, same y
        mock_game._game.wall_entries = Mock(return_value=[
            ((1, 2), (2, 2)),
        ])
        mock_game.mud_positions = {}

        display = Display(mock_game, delay=0)
        assert (1, 2) in display.v_walls

    def test_horizontal_mud_parsing(self):
        """Horizontal mud should be added to h_mud."""
        mock_game = Mock()
        mock_game._game = Mock()
        mock_game._game.width = 5
        mock_game._game.height = 5
        mock_game._game.wall_entries = Mock(return_value=[])
        # Mud between (3, 1) and (3, 2) - same x, different y
        mock_game.mud_positions = {
            (Mock(__getitem__=lambda s, i: [3, 1][i]),
             Mock(__getitem__=lambda s, i: [3, 2][i])): 2
        }

        display = Display(mock_game, delay=0)
        assert (3, 1) in display.h_mud

    def test_vertical_mud_parsing(self):
        """Vertical mud should be added to v_mud."""
        mock_game = Mock()
        mock_game._game = Mock()
        mock_game._game.width = 5
        mock_game._game.height = 5
        mock_game._game.wall_entries = Mock(return_value=[])
        # Mud between (1, 3) and (2, 3) - different x, same y
        mock_game.mud_positions = {
            (Mock(__getitem__=lambda s, i: [1, 3][i]),
             Mock(__getitem__=lambda s, i: [2, 3][i])): 2
        }

        display = Display(mock_game, delay=0)
        assert (1, 3) in display.v_mud
