"""Unit tests for display logic."""

import pytest

from pyrat_engine import GameBuilder

from pyrat_runner.display import (
    Display,
    RAT,
    PYTHON,
    RAT_AND_PYTHON,
    CHEESE,
    RAT_AND_CHEESE,
    PYTHON_AND_CHEESE,
    RAT_AND_PYTHON_AND_CHEESE,
    EMPTY,
    VERTICAL_WALL,
    VERTICAL_MUD,
    VERTICAL_NOTHING,
    HORIZONTAL_WALL,
    HORIZONTAL_MUD,
    HORIZONTAL_NOTHING,
)


@pytest.fixture
def empty_game():
    """Game with no walls or mud.

    Creates a 5x5 game with players at opposite corners.
    Has one cheese at an out-of-the-way position (engine requires at least one).
    """
    return (
        GameBuilder(5, 5)
        .with_custom_maze(walls=[], mud=[])
        .with_custom_positions((0, 0), (4, 4))
        .with_custom_cheese([(0, 4)])
        .build()
        .create()
    )


@pytest.fixture
def game_with_walls():
    """Game with specific walls configured.

    Walls:
    - Vertical wall between (1,1) and (2,1)
    - Horizontal wall between (3,2) and (3,3)
    """
    return (
        GameBuilder(5, 5)
        .with_custom_maze(
            walls=[((1, 1), (2, 1)), ((3, 2), (3, 3))],
            mud=[],
        )
        .with_custom_positions((0, 0), (4, 4))
        .with_custom_cheese([(0, 4)])
        .build()
        .create()
    )


@pytest.fixture
def game_with_mud():
    """Game with specific mud configured.

    Mud patches:
    - Vertical mud (3 turns) between (1,2) and (2,2)
    - Horizontal mud (2 turns) between (3,1) and (3,2)
    """
    return (
        GameBuilder(5, 5)
        .with_custom_maze(
            walls=[],
            mud=[((1, 2), (2, 2), 3), ((3, 1), (3, 2), 2)],
        )
        .with_custom_positions((0, 0), (4, 4))
        .with_custom_cheese([(0, 4)])
        .build()
        .create()
    )


class TestCellContent:
    """Test cell content determination logic."""

    @pytest.mark.parametrize(
        "x,y,rat_pos,python_pos,cheese_set,expected",
        [
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
        ],
    )
    def test_cell_content_combinations(
        self, x, y, rat_pos, python_pos, cheese_set, expected
    ):
        """Test all combinations of cell occupancy."""
        # Create a game with specific player and cheese positions
        # Add a dummy cheese at (0,4) if cheese_set is empty (engine requires at least one)
        cheese_list = list(cheese_set) if cheese_set else [(0, 4)]

        game = (
            GameBuilder(5, 5)
            .with_custom_maze(walls=[], mud=[])
            .with_custom_positions(rat_pos, python_pos)
            .with_custom_cheese(cheese_list)
            .build()
            .create()
        )

        display = Display(game, delay=0)
        content = display._get_cell_content(x, y, cheese_set)

        assert content == expected


class TestSeparators:
    """Test wall and mud separator determination logic."""

    @pytest.mark.parametrize(
        "x,y,expected",
        [
            # Wall position from fixture: vertical wall between (1,1) and (2,1)
            # Stored at min_x = 1
            (1, 1, VERTICAL_WALL),
            # Position with no wall or mud
            (0, 0, VERTICAL_NOTHING),
            (3, 3, VERTICAL_NOTHING),
        ],
    )
    def test_vertical_separator_with_walls(self, game_with_walls, x, y, expected):
        """Test vertical separators with known wall positions."""
        display = Display(game_with_walls, delay=0)
        assert display._get_vertical_separator(x, y) == expected

    @pytest.mark.parametrize(
        "x,y,expected",
        [
            # Mud position from fixture: vertical mud between (1,2) and (2,2)
            # Stored at min_x = 1
            (1, 2, VERTICAL_MUD),
            # Position with no wall or mud
            (0, 0, VERTICAL_NOTHING),
            (4, 4, VERTICAL_NOTHING),
        ],
    )
    def test_vertical_separator_with_mud(self, game_with_mud, x, y, expected):
        """Test vertical separators with known mud positions."""
        display = Display(game_with_mud, delay=0)
        assert display._get_vertical_separator(x, y) == expected

    @pytest.mark.parametrize(
        "x,y,expected",
        [
            # Wall position from fixture: horizontal wall between (3,2) and (3,3)
            (3, 2, HORIZONTAL_WALL),
            # Position with no wall or mud
            (0, 0, HORIZONTAL_NOTHING),
            (1, 1, HORIZONTAL_NOTHING),
        ],
    )
    def test_horizontal_separator_with_walls(self, game_with_walls, x, y, expected):
        """Test horizontal separators with known wall positions."""
        display = Display(game_with_walls, delay=0)
        assert display._get_horizontal_separator(x, y) == expected

    @pytest.mark.parametrize(
        "x,y,expected",
        [
            # Mud position from fixture: horizontal mud between (3,1) and (3,2)
            (3, 1, HORIZONTAL_MUD),
            # Position with no wall or mud
            (0, 0, HORIZONTAL_NOTHING),
            (2, 2, HORIZONTAL_NOTHING),
        ],
    )
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

    def test_wall_order_independence(self):
        """Wall parsing should work regardless of coordinate order."""
        # Test that ((x1, y1), (x2, y2)) and ((x2, y2), (x1, y1)) produce same result
        game = (
            GameBuilder(5, 5)
            .with_custom_maze(walls=[((2, 1), (1, 1))], mud=[])
            .with_custom_positions((0, 0), (4, 4))
            .with_custom_cheese([(0, 4)])
            .build()
            .create()
        )

        display = Display(game, delay=0)
        # Should still be stored with min x
        assert (1, 1) in display.v_walls, "Wall should be normalized to (1, 1)"

    def test_empty_maze_structures(self, empty_game):
        """Display with no walls or mud should have empty structure sets."""
        display = Display(empty_game, delay=0)

        assert len(display.h_walls) == 0, "Should have no horizontal walls"
        assert len(display.v_walls) == 0, "Should have no vertical walls"
        assert len(display.h_mud) == 0, "Should have no horizontal mud"
        assert len(display.v_mud) == 0, "Should have no vertical mud"
