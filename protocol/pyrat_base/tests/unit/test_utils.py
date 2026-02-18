"""Tests for utility functions in pyrat_base.utils."""

from pyrat_engine import GameBuilder
from pyrat_engine.core import Direction
from pyrat_engine.core.types import Coordinates

from pyrat_base import Player, ProtocolState, utils


def _create_game(
    width=5,
    height=5,
    walls=None,
    mud=None,
    cheese=None,
    player1_pos=(0, 0),
    player2_pos=None,
):
    """Helper to create a game via GameBuilder."""
    if player2_pos is None:
        player2_pos = (width - 1, height - 1)
    config = (
        GameBuilder(width, height)
        .with_custom_maze(walls=walls or [], mud=mud or [])
        .with_custom_positions(player1_pos, player2_pos)
        .with_custom_cheese(cheese or [(width // 2, height // 2)])
        .build()
    )
    return config.create()


class TestBasicUtilities:
    """Test basic utility functions."""

    def test_direction_to_offset(self):
        """Test converting directions to position offsets."""
        # In PyRat coordinates: UP is +y, DOWN is -y
        assert utils.direction_to_offset(Direction.UP) == (0, 1)  # UP increases y
        assert utils.direction_to_offset(Direction.RIGHT) == (1, 0)
        assert utils.direction_to_offset(Direction.DOWN) == (0, -1)  # DOWN decreases y
        assert utils.direction_to_offset(Direction.LEFT) == (-1, 0)
        assert utils.direction_to_offset(Direction.STAY) == (0, 0)

    def test_offset_to_direction(self):
        """Test converting position offsets to directions."""
        # In PyRat coordinates: UP is +y, DOWN is -y
        assert utils.offset_to_direction(0, 1) == Direction.UP  # UP increases y
        assert utils.offset_to_direction(1, 0) == Direction.RIGHT
        assert utils.offset_to_direction(0, -1) == Direction.DOWN  # DOWN decreases y
        assert utils.offset_to_direction(-1, 0) == Direction.LEFT
        assert utils.offset_to_direction(0, 0) == Direction.STAY

        # Invalid offsets
        assert utils.offset_to_direction(2, 0) is None
        assert utils.offset_to_direction(1, 1) is None
        assert utils.offset_to_direction(-2, 3) is None


class TestPathfinding:
    """Test pathfinding algorithms."""

    def test_dijkstra_simple_path(self):
        """Test Dijkstra on a simple maze without mud."""
        game = _create_game(
            width=5,
            height=5,
            cheese=[(4, 4)],
            player1_pos=(0, 0),
            player2_pos=(4, 0),
        )
        state = ProtocolState(game, Player.RAT)

        # Find path from (0,0) to (4,4)
        path = utils.find_fastest_path_dijkstra(
            state, Coordinates(0, 0), Coordinates(4, 4)
        )
        assert path is not None
        expected_path_length = 8  # 4 moves right + 4 moves up
        assert len(path) == expected_path_length

        # Verify it's a valid path (many valid paths exist)
        pos = Coordinates(0, 0)
        for move in path:
            dx, dy = utils.direction_to_offset(move)
            pos = Coordinates(pos.x + dx, pos.y + dy)
        assert pos == Coordinates(4, 4)

    def test_dijkstra_with_walls(self):
        """Test Dijkstra finding path around walls."""
        game = _create_game(
            width=5,
            height=3,
            walls=[
                ((2, 0), (3, 0)),
                ((2, 1), (3, 1)),
            ],
            cheese=[(4, 1)],
            player1_pos=(0, 1),
            player2_pos=(4, 1),
        )
        state = ProtocolState(game, Player.RAT)

        path = utils.find_fastest_path_dijkstra(
            state, Coordinates(0, 1), Coordinates(4, 1)
        )
        assert path is not None
        min_path_length = 4
        assert len(path) > min_path_length

        # Verify path is valid and reaches destination
        pos = Coordinates(0, 1)
        for move in path:
            old_pos = pos
            dx, dy = utils.direction_to_offset(move)
            pos = Coordinates(pos.x + dx, pos.y + dy)
            cost = state.movement_matrix[old_pos.x, old_pos.y, move]
            assert cost >= 0
        assert pos == Coordinates(4, 1)

    def test_dijkstra_with_mud(self):
        """Test Dijkstra choosing longer path to avoid mud."""
        game = _create_game(
            width=5,
            height=5,
            mud=[((2, 2), (3, 2), 5)],
            cheese=[(4, 2)],
            player1_pos=(0, 2),
            player2_pos=(4, 4),
        )
        state = ProtocolState(game, Player.RAT)

        path = utils.find_fastest_path_dijkstra(
            state, Coordinates(0, 2), Coordinates(4, 2)
        )
        assert path is not None

        total_time = 0
        pos = Coordinates(0, 2)
        for move in path:
            cost = state.movement_matrix[pos.x, pos.y, move]
            total_time += 1 if cost == 0 else cost
            dx, dy = utils.direction_to_offset(move)
            pos = Coordinates(pos.x + dx, pos.y + dy)
        assert pos == Coordinates(4, 2)

        max_time = 8
        assert total_time < max_time

    def test_dijkstra_no_path(self):
        """Test Dijkstra when no path exists."""
        game = _create_game(
            width=5,
            height=5,
            walls=[
                ((1, 0), (2, 0)),
                ((1, 1), (2, 1)),
                ((1, 2), (2, 2)),
                ((1, 3), (2, 3)),
                ((1, 4), (2, 4)),
            ],
            cheese=[(4, 2)],
            player1_pos=(0, 2),
            player2_pos=(4, 2),
        )
        state = ProtocolState(game, Player.RAT)

        path = utils.find_fastest_path_dijkstra(
            state, Coordinates(0, 2), Coordinates(4, 2)
        )
        assert path is None

    def test_dijkstra_same_position(self):
        """Test Dijkstra when start equals goal."""
        game = _create_game(cheese=[(2, 2)])
        state = ProtocolState(game, Player.RAT)

        path = utils.find_fastest_path_dijkstra(
            state, Coordinates(2, 2), Coordinates(2, 2)
        )
        assert path == []

    def test_find_nearest_cheese_by_time_simple(self):
        """Test finding nearest cheese by time in simple maze."""
        game = _create_game(
            cheese=[(1, 0), (4, 0), (2, 2)],
            player1_pos=(0, 0),
            player2_pos=(4, 4),
        )
        state = ProtocolState(game, Player.RAT)

        result = utils.find_nearest_cheese_by_time(state)
        assert result is not None
        cheese_pos, path, time_cost = result

        assert cheese_pos == Coordinates(1, 0)
        assert len(path) == 1
        assert time_cost == 1

    def test_find_nearest_cheese_by_time_with_mud(self):
        """Test finding nearest cheese considering mud delays."""
        game = _create_game(
            width=7,
            height=3,
            mud=[((0, 0), (1, 0), 5)],
            cheese=[(1, 0), (4, 0)],
            player1_pos=(0, 0),
            player2_pos=(6, 2),
        )
        state = ProtocolState(game, Player.RAT)

        result = utils.find_nearest_cheese_by_time(state)
        assert result is not None
        cheese_pos, path, time_cost = result

        assert cheese_pos == Coordinates(1, 0)
        expected_time_cost = 3
        expected_path_length = 3
        assert time_cost == expected_time_cost
        assert len(path) == expected_path_length

    def test_find_nearest_cheese_by_time_complex(self):
        """Test finding nearest cheese in complex maze."""
        game = _create_game(
            width=5,
            height=5,
            walls=[
                ((1, 1), (2, 1)),
                ((2, 1), (3, 1)),
                ((3, 1), (3, 2)),
                ((3, 2), (3, 3)),
            ],
            mud=[((0, 4), (1, 4), 3)],
            cheese=[(1, 4), (4, 0), (4, 4)],
            player1_pos=(0, 0),
            player2_pos=(2, 2),
        )
        state = ProtocolState(game, Player.RAT)

        result = utils.find_nearest_cheese_by_time(state)
        assert result is not None
        cheese_pos, path, time_cost = result

        assert cheese_pos in [Coordinates(1, 4), Coordinates(4, 0), Coordinates(4, 4)]
        assert time_cost > 0

        pos = Coordinates(0, 0)
        for move in path:
            old_pos = pos
            dx, dy = utils.direction_to_offset(move)
            pos = Coordinates(pos.x + dx, pos.y + dy)
            cost = state.movement_matrix[old_pos.x, old_pos.y, move]
            assert cost >= 0
        assert pos == cheese_pos

    def test_find_nearest_cheese_no_cheese(self):
        """Test finding cheese when none exist."""
        game = _create_game(
            cheese=[(2, 2)],
            player1_pos=(0, 0),
            player2_pos=(4, 4),
        )

        # Collect the cheese to empty the board
        game.step(Direction.RIGHT, Direction.LEFT)
        game.step(Direction.RIGHT, Direction.LEFT)
        game.step(Direction.UP, Direction.DOWN)
        game.step(Direction.UP, Direction.DOWN)

        state = ProtocolState(game, Player.RAT)
        assert len(state.cheese) == 0

        result = utils.find_nearest_cheese_by_time(state)
        assert result is None

    def test_find_nearest_cheese_unreachable(self):
        """Test finding cheese when all are unreachable."""
        game = _create_game(
            walls=[
                ((3, 3), (4, 3)),
                ((4, 3), (4, 4)),
                ((4, 4), (3, 4)),
                ((3, 4), (3, 3)),
            ],
            cheese=[(4, 4)],
            player1_pos=(0, 0),
            player2_pos=(2, 2),
        )
        state = ProtocolState(game, Player.RAT)

        result = utils.find_nearest_cheese_by_time(state)
        assert result is None

    def test_get_direction_toward_target(self):
        """Test getting direction toward target using pathfinding."""
        game = _create_game(
            walls=[((2, 2), (3, 2))],
            cheese=[(4, 2)],
            player1_pos=(0, 2),
            player2_pos=(4, 4),
        )
        state = ProtocolState(game, Player.RAT)

        direction = utils.get_direction_toward_target(state, Coordinates(4, 2))
        assert direction in [Direction.UP, Direction.DOWN, Direction.RIGHT]

    def test_mud_cost_calculation(self):
        """Test that mud costs are calculated correctly."""
        game = _create_game(
            width=3,
            height=1,
            mud=[
                ((0, 0), (1, 0), 3),
                ((1, 0), (2, 0), 2),
            ],
            cheese=[(2, 0)],
            player1_pos=(0, 0),
            player2_pos=(2, 0),
        )
        state = ProtocolState(game, Player.RAT)

        path = utils.find_fastest_path_dijkstra(
            state, Coordinates(0, 0), Coordinates(2, 0)
        )
        assert path is not None
        expected_moves = 2
        assert len(path) == expected_moves

        result = utils.find_nearest_cheese_by_time(state)
        assert result is not None
        _, _, time_cost = result
        expected_total_time = 5
        assert time_cost == expected_total_time
