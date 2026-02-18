"""Test helper utilities for PyRat protocol tests."""

import io
import sys
from contextlib import contextmanager
from typing import Any, Dict, List, Optional

from pyrat_engine import GameBuilder
from pyrat_engine.core.types import Direction

from pyrat_base import Protocol, ProtocolState, PyRatAI
from pyrat_base.enums import CommandType, Player


class MockAI:
    """Mock AI for testing protocol interactions without spawning subprocesses."""

    def __init__(self, ai_instance: PyRatAI):
        self.ai = ai_instance
        self.input_buffer: List[str] = []
        self.output_buffer: List[str] = []
        self.protocol = Protocol()

    def send_command(self, command: str):
        """Send a command to the AI."""
        self.input_buffer.append(command)

    def process_commands(self) -> List[str]:
        """Process all buffered commands and return responses."""
        responses = []

        for command in self.input_buffer:
            # Parse command
            cmd = self.protocol.parse_command(command)
            if cmd:
                # Simulate AI processing based on command type
                if cmd.type == CommandType.PYRAT:
                    responses.append(f"pyratai {self.ai.name}")
                    responses.append(f"id author {self.ai.author}")
                    responses.append("pyratready")
                elif cmd.type == CommandType.ISREADY:
                    responses.append("readyok")
                elif cmd.type == CommandType.GO:
                    # Get move from AI
                    if hasattr(self.ai, "_game_state") and self.ai._game_state:
                        state = self.ai._protocol_state
                        move = self.ai.get_move(state)
                        responses.append(f"move {Direction(move).name}")
                    else:
                        responses.append("move STAY")
                elif cmd.type == CommandType.PREPROCESSING_DONE:
                    responses.append("preprocessingdone")

        self.input_buffer.clear()
        self.output_buffer.extend(responses)
        return responses

    def get_all_output(self) -> List[str]:
        """Get all output produced by the AI."""
        return self.output_buffer.copy()


def run_protocol_sequence(ai_class: type, commands: List[str]) -> Dict[str, Any]:
    """
    Run a sequence of protocol commands against an AI class.

    Returns a dict with:
    - responses: List of AI responses
    - errors: Any errors that occurred
    - final_state: The AI's final game state (if available)
    """
    # Capture stdout/stderr
    old_stdout = sys.stdout
    old_stderr = sys.stderr
    stdout_capture = io.StringIO()
    stderr_capture = io.StringIO()

    try:
        sys.stdout = stdout_capture
        sys.stderr = stderr_capture

        # Create AI instance
        ai = ai_class()
        mock = MockAI(ai)

        # Process each command
        all_responses = []
        for command in commands:
            # Simulate AI receiving command
            parsed = Protocol().parse_command(command)
            if parsed:
                # Update AI state based on command
                if hasattr(ai, "_handle_command"):
                    ai._handle_command(parsed)

            mock.send_command(command)
            responses = mock.process_commands()
            all_responses.extend(responses)

        return {
            "responses": all_responses,
            "errors": stderr_capture.getvalue(),
            "stdout": stdout_capture.getvalue(),
            "final_state": getattr(ai, "_game_state", None),
        }

    finally:
        sys.stdout = old_stdout
        sys.stderr = old_stderr


def create_minimal_game_sequence() -> List[str]:
    """Create a minimal valid game initialization sequence."""
    return [
        "newgame",
        "maze width:5 height:5",
        "walls",
        "mud",
        "cheese (2,2)",
        "player1 rat (0,0)",
        "player2 python (4,4)",
        "youare rat",
    ]


def create_game_with_obstacles() -> List[str]:
    """Create a game with walls and mud for testing pathfinding."""
    return [
        "newgame",
        "maze width:5 height:5",
        "walls (0,0)-(1,0) (2,1)-(2,2)",
        "mud (1,1)-(1,2):2 (3,3)-(4,3):3",
        "cheese (0,4) (4,0) (2,2)",
        "player1 rat (0,0)",
        "player2 python (4,4)",
        "youare rat",
    ]


def assert_valid_move_response(response: str) -> str:
    """Assert that a response is a valid move and return the direction."""
    parts = response.strip().split()
    expected_parts = 2
    assert len(parts) == expected_parts, f"Invalid move response format: {response}"
    assert parts[0] == "move", f"Response must start with 'move': {response}"

    direction = parts[1]
    valid_directions = ["UP", "DOWN", "LEFT", "RIGHT", "STAY"]
    assert direction in valid_directions, f"Invalid direction {direction}"

    return direction


def assert_protocol_compliant(commands: List[str], responses: List[str]) -> bool:
    """
    Assert that a command/response sequence follows protocol rules.

    This is a simplified check - see validators.py for more comprehensive validation.
    """
    # Check handshake if present
    if commands and commands[0] == "pyrat":
        min_handshake_responses = 3
        assert (
            len(responses) >= min_handshake_responses
        ), "Handshake requires at least 3 responses"
        assert responses[0].startswith("pyratai "), "First response must be pyratai"
        assert responses[-1] == "pyratready", "Handshake must end with pyratready"

    # Check isready/readyok pairs
    for _i, cmd in enumerate(commands):
        if cmd == "isready":
            # Find corresponding readyok
            found_readyok = False
            for resp in responses:
                if resp == "readyok":
                    found_readyok = True
                    break
            assert found_readyok, "isready must be answered with readyok"

    return True


def capture_ai_execution(
    ai_script: str, commands: List[str], timeout: float = 2.0
) -> Dict[str, Any]:
    """
    Capture execution of an AI script with given commands.

    This is a simplified version for testing without subprocess complexity.
    """
    # This would normally spawn a subprocess, but for testing we can mock it
    return {
        "success": True,
        "responses": ["pyratai TestAI", "id author Test", "pyratready"],
        "errors": "",
        "timeout": False,
    }


@contextmanager
def mock_game_state(width: int = 5, height: int = 5):
    """Context manager that provides a mock game state for testing."""
    config = (
        GameBuilder(width, height)
        .with_open_maze()
        .with_custom_positions((0, 0), (width - 1, height - 1))
        .with_custom_cheese([(2, 2)])
        .build()
    )
    game = config.create()

    protocol_state = ProtocolState(game, Player.RAT)

    yield protocol_state


def compare_game_states(
    state1: Any, state2: Any, ignore_fields: Optional[List[str]] = None
) -> bool:
    """
    Compare two game states for equality.

    Args:
        state1: First game state
        state2: Second game state
        ignore_fields: List of field names to ignore in comparison

    Returns:
        True if states are equal (ignoring specified fields)
    """
    if ignore_fields is None:
        ignore_fields = []

    # Get all attributes
    attrs1 = {
        k: v
        for k, v in vars(state1).items()
        if not k.startswith("_") and k not in ignore_fields
    }
    attrs2 = {
        k: v
        for k, v in vars(state2).items()
        if not k.startswith("_") and k not in ignore_fields
    }

    return attrs1 == attrs2


def format_protocol_exchange(commands: List[str], responses: List[str]) -> str:
    """Format a protocol exchange for debugging/logging."""
    lines = []

    cmd_idx = 0
    resp_idx = 0

    # Simple interleaving - in reality this would need to be smarter
    while cmd_idx < len(commands) or resp_idx < len(responses):
        if cmd_idx < len(commands):
            lines.append(f">>> {commands[cmd_idx]}")
            cmd_idx += 1

        # Add responses that would come after this command
        if resp_idx < len(responses) and responses[resp_idx].startswith(
            (
                "pyratai",
                "id",
                "pyratready",
                "readyok",
                "move",
                "info",
                "preprocessingdone",
            )
        ):
            lines.append(f"<<< {responses[resp_idx]}")
            resp_idx += 1

    return "\n".join(lines)
