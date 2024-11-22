from pyrat._rust import PyGameConfigBuilder
from pyrat.game import Direction
import pytest


def test_custom_maze() -> None:
    """Test environment with custom maze configuration.

    Creates a 4x4 maze with the following layout (coordinate system starts from bottom left):

    3  -  -  -  P2
    2  -  C  -  -
    1  -  -  -  C     | = vertical wall
    0  P1 -  -  -     = = horizontal wall
       0  1  2  3     ~ = mud (3 turns)

    Walls block movement between:
    - (1,1)-(1,2)  # Vertical wall
    - (0,0)-(1,0)  # Horizontal wall

    Mud affects movement between:
    - (1,1)-(2,1)  # 3-turn mud
    """
    width, height = 4, 4

    # Create a test maze
    game = (PyGameConfigBuilder(width=width, height=height)
            .with_walls([
                # Walls are defined as pairs of adjacent cells they block movement between
                ((1, 1), (1, 2)),  # Vertical wall
                ((0, 0), (1, 0)),  # Horizontal wall
            ])
            .with_mud([
                # Mud is defined as (cell1, cell2, mud_value)
                ((1, 1), (2, 1), 3),  # 3-turn mud between cells
            ])
            .with_cheese([
                (1, 2),  # Cheese at (1,2)
                (3, 1),  # Cheese at (3,1)
            ])
            .with_player1_pos((0, 0))  # Bottom left
            .with_player2_pos((3, 3))  # Top right
            .with_max_turns(100)
            .build())

    # Basic dimension checks
    assert game.width == width
    assert game.height == height
    assert game.max_turns == 100

    # Player position checks
    assert game.player1_position == (0, 0), "Player 1 should start at bottom left"
    assert game.player2_position == (3, 3), "Player 2 should start at top right"

    # Cheese position checks
    cheese_positions = game.cheese_positions()
    assert len(cheese_positions) == 2, "Should have exactly 2 cheese pieces"
    assert (1, 2) in cheese_positions, "Should have cheese at (1,2)"
    assert (3, 1) in cheese_positions, "Should have cheese at (3,1)"

    # Get observation to check movement constraints
    obs = game.get_observation(is_player_one=True)
    movement_matrix = obs.movement_matrix  # Shape: [width, height, 5]

    # Check vertical wall between (1,1) and (1,2)
    assert movement_matrix[1, 1, Direction.UP.value] == -1, "Should not be able to move up from (1,1)"
    assert movement_matrix[1, 2, Direction.DOWN.value] == -1, "Should not be able to move down from (1,2)"

    # Check horizontal wall between (0,0) and (1,0)
    assert movement_matrix[0, 0, Direction.RIGHT.value] == -1, "Should not be able to move right from (0,0)"
    assert movement_matrix[1, 0, Direction.LEFT.value] == -1, "Should not be able to move left from (1,0)"

    # Check mud between (1,1) and (2,1)
    assert movement_matrix[1, 1, Direction.RIGHT.value] == 3, "Should have 3-turn mud moving right from (1,1)"
    assert movement_matrix[2, 1, Direction.LEFT.value] == 3, "Should have 3-turn mud moving left from (2,1)"

    # Check some valid moves
    assert movement_matrix[0, 0, 0] == 0, "Should be able to move up from (0,0)"
    assert movement_matrix[3, 3, 2] == 0, "Should be able to move down from (3,3)"
    assert movement_matrix[1, 1, 2] == 0, "Should be able to move down from (1,1)"

    # Initial scores should be 0
    assert game.player1_score == 0.0, "Initial P1 score should be 0"
    assert game.player2_score == 0.0, "Initial P2 score should be 0"


def test_invalid_maze_configurations() -> None:
    """Test that invalid maze configurations are properly rejected."""
    width, height = 4, 4

    # Test invalid mud values
    with pytest.raises(ValueError, match=r".*must be at least 2.*"):
        (PyGameConfigBuilder(width=width, height=height)
         .with_cheese([(1, 2)])
         .with_mud([((1, 1), (1, 2), 0)])  # Invalid mud value (0)
         .build())

    with pytest.raises(ValueError, match=r".*must be at least 2.*"):
        (PyGameConfigBuilder(width=width, height=height)
         .with_cheese([(1, 2)])
         .with_mud([((1, 1), (1, 2), 1)])  # Invalid mud value (1 is normal passage)
         .build())

    # Test wall on existing mud
    with pytest.raises(ValueError, match=r".*already mud.*"):
        (PyGameConfigBuilder(width=width, height=height)
        .with_cheese([(1, 2)])
         .with_mud([((1, 1), (1, 2), 3)])
         .with_walls([((1, 1), (1, 2))])  # Wall on existing mud
         .build())

    # Test mud on existing wall
    with pytest.raises(ValueError, match=r".*already a wall.*"):
        (PyGameConfigBuilder(width=width, height=height)
         .with_cheese([(1, 2)])
         .with_walls([((1, 1), (1, 2))])
         .with_mud([((1, 1), (1, 2), 3)])  # Mud on existing wall
         .build())
