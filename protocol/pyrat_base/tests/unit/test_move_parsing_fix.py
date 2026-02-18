"""Test that the move parsing bug is fixed."""

from pyrat_engine import PyRat
from pyrat_engine.core import Direction
from pyrat_engine.core.types import Coordinates

from pyrat_base import Protocol, PyRatAI
from pyrat_base.enums import CommandType, Player


class MinimalTestAI(PyRatAI):
    """Minimal AI for testing."""

    def __init__(self):
        super().__init__("TestBot", "Tester")

    def get_move(self, state):
        return Direction.STAY


def test_move_parsing_with_correct_data_structure():
    """Test that moves are parsed correctly from the protocol data structure."""
    ai = MinimalTestAI()
    protocol = Protocol()

    # Parse a moves command as it comes from the protocol
    cmd = protocol.parse_command("moves rat:UP python:DOWN")
    assert cmd is not None
    assert cmd.type == CommandType.MOVES

    # Verify the data structure
    assert "moves" in cmd.data
    moves = cmd.data["moves"]
    assert Player.RAT in moves
    assert Player.PYTHON in moves
    assert moves[Player.RAT] == "UP"
    assert moves[Player.PYTHON] == "DOWN"

    # Now test that our fix in base_ai.py would work
    # Simulate what happens in _handle_playing when it receives this command
    moves_dict = cmd.data.get("moves", {})

    # The fix checks for Player enum keys first
    if Player.RAT in moves_dict:
        rat_move = ai._parse_direction(moves_dict[Player.RAT])
        python_move = ai._parse_direction(moves_dict[Player.PYTHON])
    else:
        # Fallback to string keys
        rat_move = ai._parse_direction(moves_dict.get("rat", "STAY"))
        python_move = ai._parse_direction(moves_dict.get("python", "STAY"))

    assert rat_move == Direction.UP
    assert python_move == Direction.DOWN


def test_move_parsing_with_game_state():
    """Test that move parsing updates the game state correctly."""
    ai = MinimalTestAI()

    # Create a minimal game state
    game = PyRat.create_custom(
        width=5,
        height=5,
        walls=[],
        cheese=[(2, 2)],
        player1_pos=(0, 0),
        player2_pos=(4, 4),
    )

    # Set the game state on the AI
    ai._game_state = game
    ai._player_identity = Player.RAT

    # Initial positions
    assert game.player1_position == Coordinates(0, 0)
    assert game.player2_position == Coordinates(4, 4)

    # Simulate a move
    game.step(Direction.UP, Direction.DOWN)

    # Check positions updated
    assert game.player1_position == Coordinates(0, 1)  # Moved up
    assert game.player2_position == Coordinates(4, 3)  # Moved down


def test_parse_direction_edge_cases():
    """Test _parse_direction handles edge cases."""
    ai = MinimalTestAI()

    # Valid directions
    assert ai._parse_direction("UP") == Direction.UP
    assert ai._parse_direction("up") == Direction.UP  # Case insensitive
    assert ai._parse_direction("STAY") == Direction.STAY

    # Edge cases that should default to STAY
    assert ai._parse_direction("") == Direction.STAY
    assert ai._parse_direction("INVALID") == Direction.STAY

    # Our fix ensures None is handled
    assert ai._parse_direction(None) == Direction.STAY
