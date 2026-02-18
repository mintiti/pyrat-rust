"""Test validation through replay system for cases that the parser supports."""

import pytest
from pyrat_engine.core.types import Coordinates

from pyrat_base.replay import ReplayPlayer, ReplayReader


def test_position_out_of_bounds_in_replay():
    """Test that out-of-bounds positions in replay files give clear error messages."""
    replay_content = """[Event "Test"]
[Rat "Bot1"]
[Python "Bot2"]
[Width "10"]
[Height "10"]
C:(10,10)

1. S/S (0ms/0ms)
"""

    reader = ReplayReader()
    with pytest.raises(ValueError, match="outside board bounds"):
        replay = reader.parse(replay_content)
        ReplayPlayer(replay)


def test_mud_value_too_small_in_replay():
    """Test that mud value < 2 in replay files give clear error messages."""
    replay_content = """[Event "Test"]
[Rat "Bot1"]
[Python "Bot2"]
[Width "10"]
[Height "10"]
C:(5,5)
M:(0,0)-(0,1):1

1. S/S (0ms/0ms)
"""

    reader = ReplayReader()
    with pytest.raises(ValueError, match="Mud value must be at least 2 turns"):
        replay = reader.parse(replay_content)
        ReplayPlayer(replay)


def test_empty_cheese_in_replay():
    """Test that replays with no cheese get a default cheese added."""
    replay_content = """[Event "Test"]
[Rat "Bot1"]
[Python "Bot2"]
[Width "10"]
[Height "10"]

1. S/S (0ms/0ms)
"""

    reader = ReplayReader()
    replay = reader.parse(replay_content)
    player = ReplayPlayer(replay)

    # ReplayPlayer adds a default cheese if none provided
    assert len(player.game.cheese_positions()) == 1
    assert player.game.cheese_positions()[0] == Coordinates(5, 5)  # Center position


def test_valid_replay_still_works():
    """Test that valid replays still work correctly."""
    replay_content = """[Event "Test"]
[Rat "Bot1"]
[Python "Bot2"]
[Width "10"]
[Height "10"]
C:(5,5) (7,7)
M:(0,0)-(0,1):3
W:(1,1)-(1,2)

1. R/L (5ms/10ms)
2. R/U (3ms/8ms)
"""

    reader = ReplayReader()
    replay = reader.parse(replay_content)
    player = ReplayPlayer(replay)

    # Verify game was created successfully
    assert player.game is not None
    expected_width = 10
    expected_height = 10
    assert player.game.width == expected_width
    assert player.game.height == expected_height

    # Step through the replay
    assert player.step_forward() is not None  # Move 1
    assert player.current_turn == 1

    assert player.step_forward() is not None  # Move 2
    expected_turn = 2
    assert player.current_turn == expected_turn

    assert player.step_forward() is None  # No more moves
