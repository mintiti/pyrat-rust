"""Tests for utility functions in pyrat_base.utils."""

from pyrat_engine.core.game import GameState as PyGameState
from pyrat_engine.core.types import Coordinates
from pyrat_engine.game import Direction

from pyrat_base import Player, ProtocolState, utils


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
        # Create a 5x5 maze with no walls or mud
        game = PyGameState.create_custom(
            width=5,
            height=5,
            walls=[],
            mud=[],
            cheese=[(4, 4)],
            player1_pos=(0, 0),
            player2_pos=(4, 0),
        )
        state = ProtocolState(game, Player.RAT)

        # Find path from (0,0) to (4,4)
        path = utils.find_fastest_path_dijkstra(state, Coordinates(0, 0), Coordinates(4, 4))
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
        # Create a maze with walls blocking the direct horizontal path
        # We'll create a partial barrier that forces going around
        game = PyGameState.create_custom(
            width=5,
            height=3,
            walls=[
                # Create partial barrier between x=2 and x=3
                # Leave gap at y=2 to allow passage
                ((2, 0), (3, 0)),  # Wall between (2,0) and (3,0)
                ((2, 1), (3, 1)),  # Wall between (2,1) and (3,1)
                # No wall at y=2 - can pass through here
            ],
            mud=[],
            cheese=[(4, 1)],
            player1_pos=(0, 1),
            player2_pos=(4, 1),
        )
        state = ProtocolState(game, Player.RAT)

        # Direct path is blocked, must go around (up or down then across)
        path = utils.find_fastest_path_dijkstra(state, Coordinates(0, 1), Coordinates(4, 1))
        assert path is not None
        # Must go around the wall
        min_path_length = 4  # More than direct distance of 4
        assert len(path) > min_path_length

        # Verify path is valid and reaches destination
        pos = Coordinates(0, 1)
        for move in path:
            old_pos = pos
            dx, dy = utils.direction_to_offset(move)
            pos = Coordinates(pos.x + dx, pos.y + dy)
            # Verify move is valid (not through a wall)
            cost = state.movement_matrix[old_pos.x, old_pos.y, move]
            assert cost >= 0  # Not blocked
        assert pos == Coordinates(4, 1)

    def test_dijkstra_with_mud(self):
        """Test Dijkstra choosing longer path to avoid mud."""
        # Create a maze where direct path has mud
        game = PyGameState.create_custom(
            width=5,
            height=5,
            walls=[],
            mud=[
                ((2, 2), (3, 2), 5),  # 5-turn mud in the middle
            ],
            cheese=[(4, 2)],
            player1_pos=(0, 2),
            player2_pos=(4, 4),
        )
        state = ProtocolState(game, Player.RAT)

        # Two possible strategies:
        # 1. Direct through mud: 2 normal + 5 mud + 1 normal = 8 turns
        # 2. Go around: more moves but no mud
        path = utils.find_fastest_path_dijkstra(state, Coordinates(0, 2), Coordinates(4, 2))
        assert path is not None

        # Calculate actual time cost of the path
        total_time = 0
        pos = Coordinates(0, 2)
        for move in path:
            cost = state.movement_matrix[pos.x, pos.y, move]
            total_time += 1 if cost == 0 else cost
            dx, dy = utils.direction_to_offset(move)
            pos = Coordinates(pos.x + dx, pos.y + dy)
        assert pos == Coordinates(4, 2)

        # The path should avoid the expensive mud
        # Going around should take less than 8 turns
        max_time = 8
        assert total_time < max_time

    def test_dijkstra_no_path(self):
        """Test Dijkstra when no path exists."""
        # Create a maze with complete wall barrier
        # To create a vertical barrier at x=2, we need walls between x=1 and x=2
        game = PyGameState.create_custom(
            width=5,
            height=5,
            walls=[
                ((1, 0), (2, 0)),  # Wall between (1,0) and (2,0)
                ((1, 1), (2, 1)),  # Wall between (1,1) and (2,1)
                ((1, 2), (2, 2)),  # Wall between (1,2) and (2,2)
                ((1, 3), (2, 3)),  # Wall between (1,3) and (2,3)
                ((1, 4), (2, 4)),  # Wall between (1,4) and (2,4)
                # Complete vertical barrier between x=1 and x=2
            ],
            mud=[],
            cheese=[(4, 2)],
            player1_pos=(0, 2),
            player2_pos=(4, 2),
        )
        state = ProtocolState(game, Player.RAT)

        # No path exists
        path = utils.find_fastest_path_dijkstra(state, Coordinates(0, 2), Coordinates(4, 2))
        assert path is None

    def test_dijkstra_same_position(self):
        """Test Dijkstra when start equals goal."""
        game = PyGameState.create_custom(
            width=5, height=5, walls=[], mud=[], cheese=[(2, 2)]
        )
        state = ProtocolState(game, Player.RAT)

        path = utils.find_fastest_path_dijkstra(state, Coordinates(2, 2), Coordinates(2, 2))
        assert path == []  # Empty path

    def test_find_nearest_cheese_by_time_simple(self):
        """Test finding nearest cheese by time in simple maze."""
        # Place multiple cheese at different distances
        game = PyGameState.create_custom(
            width=5,
            height=5,
            walls=[],
            mud=[],
            cheese=[(1, 0), (4, 0), (2, 2)],  # Different distances
            player1_pos=(0, 0),
            player2_pos=(4, 4),
        )
        state = ProtocolState(game, Player.RAT)

        result = utils.find_nearest_cheese_by_time(state)
        assert result is not None
        cheese_pos, path, time_cost = result

        # Should choose (1,0) as it's closest (1 move away)
        assert cheese_pos == Coordinates(1, 0)
        assert len(path) == 1
        assert time_cost == 1

    def test_find_nearest_cheese_by_time_with_mud(self):
        """Test finding nearest cheese considering mud delays."""
        # Create scenario where closer cheese has mud
        game = PyGameState.create_custom(
            width=7,
            height=3,
            walls=[],
            mud=[
                ((0, 0), (1, 0), 5),  # 5-turn mud to nearest cheese
            ],
            cheese=[(1, 0), (4, 0)],  # Two cheese at different distances
            player1_pos=(0, 0),
            player2_pos=(6, 2),
        )
        state = ProtocolState(game, Player.RAT)

        result = utils.find_nearest_cheese_by_time(state)
        assert result is not None
        cheese_pos, path, time_cost = result

        # Cheese at (1,0): 5 turns through mud OR 3 turns around (UP, RIGHT, DOWN)
        # Cheese at (4,0): 4 turns (RIGHT, RIGHT, RIGHT, RIGHT)
        # Should choose (1,0) via the around path as it's faster (3 turns)
        assert cheese_pos == Coordinates(1, 0)
        expected_time_cost = 3
        expected_path_length = 3  # The around path
        assert time_cost == expected_time_cost
        assert len(path) == expected_path_length

    def test_find_nearest_cheese_by_time_complex(self):
        """Test finding nearest cheese in complex maze."""
        # Create a maze where direct paths are blocked
        game = PyGameState.create_custom(
            width=5,
            height=5,
            walls=[
                # Create barriers
                ((1, 1), (2, 1)),
                ((2, 1), (3, 1)),
                ((3, 1), (3, 2)),
                ((3, 2), (3, 3)),
            ],
            mud=[
                ((0, 4), (1, 4), 3),  # Mud near one cheese
            ],
            cheese=[(1, 4), (4, 0), (4, 4)],
            player1_pos=(0, 0),
            player2_pos=(2, 2),
        )
        state = ProtocolState(game, Player.RAT)

        result = utils.find_nearest_cheese_by_time(state)
        assert result is not None
        cheese_pos, path, time_cost = result

        # Should find optimal cheese considering walls and mud
        assert cheese_pos in [Coordinates(1, 4), Coordinates(4, 0), Coordinates(4, 4)]
        assert time_cost > 0

        # Verify the path is valid
        pos = Coordinates(0, 0)
        for move in path:
            old_pos = pos
            dx, dy = utils.direction_to_offset(move)
            pos = Coordinates(pos.x + dx, pos.y + dy)
            cost = state.movement_matrix[old_pos.x, old_pos.y, move]
            assert cost >= 0  # Valid move
        assert pos == cheese_pos

    def test_find_nearest_cheese_no_cheese(self):
        """Test finding cheese when none exist."""
        # PyGameState requires at least one cheese, so we'll place one
        # but then manually clear it to test the no-cheese case
        game = PyGameState.create_custom(
            width=5,
            height=5,
            walls=[],
            mud=[],
            cheese=[(2, 2)],  # Need at least one for creation
            player1_pos=(0, 0),
            player2_pos=(4, 4),
        )

        # Collect the cheese to empty the board
        game.step(Direction.RIGHT, Direction.LEFT)
        game.step(Direction.RIGHT, Direction.LEFT)
        game.step(Direction.UP, Direction.DOWN)
        game.step(Direction.UP, Direction.DOWN)
        # Player 1 should have collected the cheese at (2,2)

        state = ProtocolState(game, Player.RAT)
        assert len(state.cheese) == 0  # Verify no cheese left

        result = utils.find_nearest_cheese_by_time(state)
        assert result is None

    def test_find_nearest_cheese_unreachable(self):
        """Test finding cheese when all are unreachable."""
        # Create maze with cheese behind walls
        game = PyGameState.create_custom(
            width=5,
            height=5,
            walls=[
                # Box in the cheese
                ((3, 3), (4, 3)),
                ((4, 3), (4, 4)),
                ((4, 4), (3, 4)),
                ((3, 4), (3, 3)),
            ],
            mud=[],
            cheese=[(4, 4)],  # Trapped cheese
            player1_pos=(0, 0),
            player2_pos=(2, 2),
        )
        state = ProtocolState(game, Player.RAT)

        result = utils.find_nearest_cheese_by_time(state)
        assert result is None

    def test_get_direction_toward_target(self):
        """Test getting direction toward target using pathfinding."""
        game = PyGameState.create_custom(
            width=5,
            height=5,
            walls=[
                ((2, 2), (3, 2)),  # Horizontal wall
            ],
            mud=[],
            cheese=[(4, 2)],
            player1_pos=(0, 2),
            player2_pos=(4, 4),
        )
        state = ProtocolState(game, Player.RAT)

        # Get direction toward (4, 2) from current position
        direction = utils.get_direction_toward_target(state, Coordinates(4, 2))
        assert direction in [Direction.UP, Direction.DOWN, Direction.RIGHT]
        # Can't be LEFT or STAY as we need to move toward target

    def test_mud_cost_calculation(self):
        """Test that mud costs are calculated correctly."""
        # Create a path that must go through mud
        game = PyGameState.create_custom(
            width=3,
            height=1,
            walls=[],
            mud=[
                ((0, 0), (1, 0), 3),  # 3-turn mud
                ((1, 0), (2, 0), 2),  # 2-turn mud
            ],
            cheese=[(2, 0)],
            player1_pos=(0, 0),
            player2_pos=(2, 0),
        )
        state = ProtocolState(game, Player.RAT)

        path = utils.find_fastest_path_dijkstra(state, Coordinates(0, 0), Coordinates(2, 0))
        assert path is not None
        expected_moves = 2  # Two moves
        assert len(path) == expected_moves

        # Total time should be 3 + 2 = 5 turns
        result = utils.find_nearest_cheese_by_time(state)
        assert result is not None
        _, _, time_cost = result
        expected_total_time = 5
        assert time_cost == expected_total_time
