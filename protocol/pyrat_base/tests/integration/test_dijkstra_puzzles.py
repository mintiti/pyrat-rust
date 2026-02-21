"""Test puzzles for Dijkstra pathfinding algorithm using the actual engine.

Each test represents a specific scenario designed to verify the algorithm
makes optimal decisions considering walls, mud, and actual travel time.
"""
# ruff: noqa: PLR2004

import pytest
from pyrat_engine import GameBuilder, GameConfig
from pyrat_engine.core import Direction
from pyrat_engine.core.types import Coordinates

from pyrat_base.enums import Player
from pyrat_base.protocol_state import ProtocolState
from pyrat_base.utils import find_nearest_cheese_by_time


@pytest.fixture
def create_game_state():
    """Factory fixture for creating custom game states."""

    def _create_game(
        width=5,
        height=5,
        walls=None,
        mud=None,
        cheese=None,
        player1_pos=(0, 0),
        player2_pos=(4, 4),
    ):
        config = (
            GameBuilder(width, height)
            .with_custom_maze(walls=walls or [], mud=mud or [])
            .with_custom_positions(player1_pos, player2_pos)
            .with_custom_cheese(cheese or [])
            .build()
        )
        game = config.create()
        return ProtocolState(game, Player.RAT)

    return _create_game


class TestBasicPathfinding:
    """Test basic pathfinding scenarios without obstacles."""

    def test_obvious_choice_simple_case(self, create_game_state):
        """Simple case with no obstacles - should choose closest cheese."""
        state = create_game_state(cheese=[(1, 0), (3, 0), (4, 0)])

        result = find_nearest_cheese_by_time(state)
        assert result is not None

        cheese_pos, path, time = result
        assert cheese_pos == Coordinates(1, 0)
        assert time == 1
        assert path == [Direction.RIGHT]


class TestWallNavigation:
    """Test pathfinding around walls."""

    @pytest.fixture
    def vertical_wall(self):
        """Create a vertical wall blocking passage from column 1 to column 2."""
        return [((1, y), (2, y)) for y in range(5)]

    def test_wall_illusion_blocked_path(self, create_game_state, vertical_wall):
        """Geometrically close cheese blocked by wall - should choose accessible one."""
        state = create_game_state(
            width=6,
            height=6,
            walls=vertical_wall,
            cheese=[(2, 0), (3, 3)],
        )

        result = find_nearest_cheese_by_time(state)
        assert result is not None

        cheese_pos, _, time = result
        assert cheese_pos == Coordinates(3, 3)
        assert time == 10


class TestMudNavigation:
    """Test pathfinding with mud obstacles."""

    @pytest.mark.parametrize(
        "mud_cost,cheese_positions,expected_cheese,expected_time",
        [
            # Heavy mud - avoid it
            (5, [(1, 0), (0, 3)], (0, 3), 3),
            # Light mud - worth going through
            (2, [(1, 0), (9, 9)], (1, 0), 2),
        ],
    )
    def test_mud_cost_decisions(
        self,
        create_game_state,
        mud_cost,
        cheese_positions,
        expected_cheese,
        expected_time,
    ):
        """Test choosing optimal path based on mud cost."""
        max_x = max(pos[0] for pos in cheese_positions) + 1
        max_y = max(pos[1] for pos in cheese_positions) + 1

        state = create_game_state(
            width=max(max_x, 5),
            height=max(max_y, 5),
            mud=[((0, 0), (1, 0), mud_cost)],
            cheese=cheese_positions,
        )

        result = find_nearest_cheese_by_time(state)
        assert result is not None

        cheese_pos, _, time = result
        assert cheese_pos == Coordinates(*expected_cheese)
        assert time == expected_time


class TestComplexMazes:
    """Test pathfinding in complex maze scenarios."""

    @pytest.fixture
    def horizontal_barrier_with_gap(self):
        """Create a horizontal barrier at y=3 with gap at x=5."""
        return [
            ((x, 3), (x, 4))
            for x in range(1, 5)
            # Gap at x=5
        ]

    def test_maze_with_barrier_and_mud(
        self, create_game_state, horizontal_barrier_with_gap
    ):
        """Complex maze requiring navigation around barriers and mud."""
        state = create_game_state(
            width=7,
            height=7,
            walls=horizontal_barrier_with_gap,
            mud=[((1, 2), (1, 3), 4)],
            cheese=[(3, 5), (1, 1)],
        )

        result = find_nearest_cheese_by_time(state)
        assert result is not None

        cheese_pos, _, time = result
        assert cheese_pos == Coordinates(1, 1)
        assert time == 2

    def test_multiple_mud_paths_choose_optimal(self, create_game_state):
        """When all paths have mud, choose the least costly route."""
        walls = [
            ((2, 2), (2, 3)),
            ((2, 3), (3, 3)),
            ((3, 3), (4, 3)),
            ((4, 3), (4, 2)),
        ]

        mud = [
            ((2, 1), (3, 1), 3),
            ((3, 1), (4, 1), 5),
            ((1, 2), (2, 2), 2),
        ]

        state = create_game_state(
            width=6,
            height=6,
            walls=walls,
            mud=mud,
            cheese=[(3, 2)],
        )

        result = find_nearest_cheese_by_time(state)
        assert result is not None

        cheese_pos, path, _ = result
        assert cheese_pos == Coordinates(3, 2)
        assert len(path) <= 6


class TestAlgorithmComparison:
    """Test cases demonstrating Dijkstra's superiority over greedy approaches."""

    def test_dijkstra_vs_greedy_manhattan(self, create_game_state):
        """Case where greedy (Manhattan distance) fails due to heavy mud."""
        mud = [
            ((0, 0), (1, 0), 10),
            ((1, 0), (2, 0), 10),
        ]

        state = create_game_state(
            mud=mud,
            cheese=[(2, 0), (0, 4)],
        )

        result = find_nearest_cheese_by_time(state)
        assert result is not None

        cheese_pos, _, time = result
        assert cheese_pos == Coordinates(0, 4)
        assert time == 4


class TestRandomGames:
    """Test pathfinding on procedurally generated games."""

    @pytest.mark.parametrize(
        "seed,width,height,cheese_count",
        [
            (12345, 15, 11, 21),
            (54321, 10, 10, 20),
            (99999, 20, 15, 30),
        ],
    )
    def test_pathfinding_on_random_game(self, seed, width, height, cheese_count):
        """Test pathfinding works correctly on random games."""
        game = GameConfig.classic(width, height, cheese_count).create(seed=seed)
        state = ProtocolState(game, Player.RAT)

        result = find_nearest_cheese_by_time(state)

        if result:
            cheese_pos, path, time = result

            assert cheese_pos in state.cheese
            assert len(path) > 0
            assert time >= len(path)

            if path:
                first_move = path[0]
                assert first_move in [
                    Direction.UP,
                    Direction.DOWN,
                    Direction.LEFT,
                    Direction.RIGHT,
                ]


@pytest.mark.integration
class TestPathfindingIntegration:
    """Integration tests combining multiple pathfinding challenges."""

    def test_complex_scenario_walls_and_mud(self, create_game_state):
        """Test scenario with both walls and mud obstacles."""
        walls = [
            ((2, 0), (2, 1)),
            ((2, 1), (2, 2)),
            ((2, 2), (2, 3)),
        ]
        mud = [
            ((0, 3), (1, 3), 3),
            ((3, 0), (4, 0), 2),
        ]

        state = create_game_state(
            width=6, height=5, walls=walls, mud=mud, cheese=[(4, 0), (1, 3), (5, 4)]
        )

        result = find_nearest_cheese_by_time(state)
        assert result is not None

        cheese_pos, path, time = result
        assert cheese_pos in [Coordinates(4, 0), Coordinates(1, 3), Coordinates(5, 4)]
        assert time > 0
        assert len(path) > 0
