"""Protocol validation utilities for PyRat tests."""

from typing import List, Optional, Tuple

from pyrat_base import Protocol
from pyrat_base.enums import CommandType, ResponseType


class ProtocolValidator:
    """Validate protocol sequences and state transitions."""

    # Valid state transitions
    VALID_TRANSITIONS = {
        "INITIAL": ["HANDSHAKE"],
        "HANDSHAKE": ["READY"],
        "READY": ["GAME_INIT", "TERMINATED"],
        "GAME_INIT": ["PREPROCESSING", "PLAYING"],
        "PREPROCESSING": ["PLAYING"],
        "PLAYING": ["GAME_OVER"],
        "GAME_OVER": ["READY", "TERMINATED"],
        "TERMINATED": [],
    }

    # Commands allowed in each state
    ALLOWED_COMMANDS = {
        "INITIAL": [CommandType.PYRAT],
        "HANDSHAKE": [CommandType.ISREADY],
        "READY": [CommandType.NEW_GAME, CommandType.QUIT],
        "GAME_INIT": [
            CommandType.MAZE,
            CommandType.WALLS,
            CommandType.MUD,
            CommandType.CHEESE,
            CommandType.PLAYER1,
            CommandType.PLAYER2,
            CommandType.YOU_ARE,
            CommandType.START_PREPROCESSING,
            CommandType.GO,
            CommandType.ISREADY,
        ],
        "PREPROCESSING": [
            CommandType.PREPROCESSING_DONE,
            CommandType.TIMEOUT,
            CommandType.ISREADY,
        ],
        "PLAYING": [
            CommandType.GO,
            CommandType.STOP,
            CommandType.MOVES,
            CommandType.CURRENT_POSITION,
            CommandType.SCORE,
            CommandType.TIMEOUT,
            CommandType.GAME_OVER,
            CommandType.ISREADY,
        ],
        "GAME_OVER": [CommandType.NEW_GAME, CommandType.QUIT, CommandType.ISREADY],
        "TERMINATED": [],
    }

    def __init__(self):
        self.state = "INITIAL"
        self.protocol = Protocol()

    def validate_command_sequence(
        self, commands: List[str]
    ) -> Tuple[bool, Optional[str]]:
        """
        Validate a sequence of protocol commands.

        Returns:
            (is_valid, error_message)
        """
        self.state = "INITIAL"

        for i, command in enumerate(commands):
            cmd = self.protocol.parse_command(command)
            if cmd is None:
                return False, f"Command {i}: Failed to parse '{command}'"

            # Check if command is allowed in current state
            if cmd.type not in self.ALLOWED_COMMANDS.get(self.state, []):
                return (
                    False,
                    f"Command {i}: {cmd.type.name} not allowed in state {self.state}",
                )

            # Update state based on command
            old_state = self.state
            self._update_state(cmd.type)

            # Validate state transition
            if (
                self.state not in self.VALID_TRANSITIONS.get(old_state, [])
                and self.state != old_state
            ):
                return (
                    False,
                    f"Command {i}: Invalid transition from {old_state} to {self.state}",
                )

        return True, None

    def _update_state(self, command_type: CommandType):
        """Update state based on command type."""
        if command_type == CommandType.PYRAT:
            self.state = "HANDSHAKE"
        elif command_type == CommandType.NEW_GAME and self.state in [
            "READY",
            "GAME_OVER",
        ]:
            self.state = "GAME_INIT"
        elif command_type == CommandType.YOU_ARE and self.state == "GAME_INIT":
            self.state = "GAME_INIT"  # Stay in GAME_INIT
        elif (
            command_type == CommandType.START_PREPROCESSING
            and self.state == "GAME_INIT"
        ):
            self.state = "PREPROCESSING"
        elif command_type == CommandType.GO and self.state == "GAME_INIT":
            self.state = "PLAYING"
        elif (
            command_type == CommandType.PREPROCESSING_DONE
            and self.state == "PREPROCESSING"
        ):
            self.state = "PLAYING"
        elif command_type == CommandType.TIMEOUT and self.state == "PREPROCESSING":
            self.state = "PLAYING"
        elif command_type == CommandType.GAME_OVER and self.state == "PLAYING":
            self.state = "GAME_OVER"
        elif command_type == CommandType.QUIT:
            self.state = "TERMINATED"


def validate_move_format(move_str: str) -> Tuple[bool, Optional[str]]:
    """
    Validate move response format.

    Valid formats:
    - "move UP"
    - "move DOWN"
    - "move LEFT"
    - "move RIGHT"
    - "move STAY"
    """
    parts = move_str.strip().split()
    if len(parts) != 2:
        return False, "Move must have exactly 2 parts"

    if parts[0] != "move":
        return False, "Move must start with 'move'"

    valid_directions = ["UP", "DOWN", "LEFT", "RIGHT", "STAY"]
    if parts[1] not in valid_directions:
        return (
            False,
            f"Invalid direction '{parts[1]}', must be one of {valid_directions}",
        )

    return True, None


def validate_handshake_response(responses: List[str]) -> Tuple[bool, Optional[str]]:
    """
    Validate AI handshake response sequence.

    Expected:
    1. "pyratai <name>"
    2. (optional) "id <key> <value>" commands
    3. (optional) "setoption <name> <value>" commands
    4. "pyratready"
    """
    if not responses:
        return False, "No responses provided"

    # First response must be pyratai
    if not responses[0].startswith("pyratai "):
        return False, "First response must be 'pyratai <name>'"

    # Last response must be pyratready
    if responses[-1] != "pyratready":
        return False, "Last response must be 'pyratready'"

    # Check intermediate responses
    for i, response in enumerate(responses[1:-1], 1):
        if not (response.startswith("id ") or response.startswith("setoption ")):
            return False, f"Response {i} must be 'id' or 'setoption' command"

    return True, None


def validate_game_state_consistency(commands: List[str]) -> Tuple[bool, Optional[str]]:
    """
    Validate that game state commands create a consistent state.

    Checks:
    - Maze dimensions are set before other components
    - Wall/mud positions are within maze bounds
    - Cheese positions are within maze bounds
    - Player positions are within maze bounds
    - No duplicate specifications
    """
    protocol = Protocol()

    maze_width = None
    maze_height = None
    walls_seen = set()
    mud_seen = set()
    cheese_seen = set()
    player1_pos = None
    player2_pos = None

    for command in commands:
        cmd = protocol.parse_command(command)
        if not cmd:
            continue

        if cmd.type == CommandType.MAZE:
            maze_width = cmd.data["width"]
            maze_height = cmd.data["height"]

        elif cmd.type == CommandType.WALLS and "walls" in cmd.data:
            if maze_width is None:
                return False, "Walls specified before maze dimensions"
            for wall in cmd.data["walls"]:
                # Check bounds
                for x, y in wall:
                    if not (0 <= x < maze_width and 0 <= y < maze_height):
                        return False, f"Wall position ({x},{y}) out of bounds"
                # Check for duplicates
                wall_tuple = tuple(sorted(wall))
                if wall_tuple in walls_seen:
                    return False, f"Duplicate wall specification: {wall}"
                walls_seen.add(wall_tuple)

        elif cmd.type == CommandType.MUD and "mud" in cmd.data:
            if maze_width is None:
                return False, "Mud specified before maze dimensions"
            for (pos1, pos2), value in cmd.data["mud"]:
                # Check bounds
                for x, y in [pos1, pos2]:
                    if not (0 <= x < maze_width and 0 <= y < maze_height):
                        return False, f"Mud position ({x},{y}) out of bounds"
                # Check for duplicates
                mud_tuple = tuple(sorted([pos1, pos2]))
                if mud_tuple in mud_seen:
                    return False, f"Duplicate mud specification: {pos1}-{pos2}"
                mud_seen.add(mud_tuple)

        elif cmd.type == CommandType.CHEESE and "positions" in cmd.data:
            if maze_width is None:
                return False, "Cheese specified before maze dimensions"
            for x, y in cmd.data["positions"]:
                if not (0 <= x < maze_width and 0 <= y < maze_height):
                    return False, f"Cheese position ({x},{y}) out of bounds"
                if (x, y) in cheese_seen:
                    return False, f"Duplicate cheese at ({x},{y})"
                cheese_seen.add((x, y))

        elif cmd.type == CommandType.PLAYER1:
            if maze_width is None:
                return False, "Player1 specified before maze dimensions"
            x, y = cmd.data["position"]
            if not (0 <= x < maze_width and 0 <= y < maze_height):
                return False, f"Player1 position ({x},{y}) out of bounds"
            player1_pos = (x, y)

        elif cmd.type == CommandType.PLAYER2:
            if maze_width is None:
                return False, "Player2 specified before maze dimensions"
            x, y = cmd.data["position"]
            if not (0 <= x < maze_width and 0 <= y < maze_height):
                return False, f"Player2 position ({x},{y}) out of bounds"
            player2_pos = (x, y)

    # Final consistency checks
    if maze_width is None:
        return False, "No maze dimensions specified"

    if player1_pos == player2_pos and player1_pos is not None:
        return False, "Players cannot start at same position"

    return True, None


class ResponseValidator:
    """Validate AI responses to protocol commands."""

    def __init__(self):
        self.protocol = Protocol()

    def validate_response_format(
        self, response_type: ResponseType, response: str
    ) -> Tuple[bool, Optional[str]]:
        """Validate that a response matches expected format for its type."""
        # Try to parse with protocol
        result = self.protocol.format_response(response_type, {})

        # This is a simple check - in reality we'd need to implement
        # proper validation for each response type
        if response_type == ResponseType.ID:
            if not response.startswith("pyratai "):
                return False, "ID response must start with 'pyratai'"

        elif response_type == ResponseType.READY:
            if response != "pyratready":
                return False, "READY response must be 'pyratready'"

        elif response_type == ResponseType.READYOK:
            if response != "readyok":
                return False, "READYOK response must be 'readyok'"

        elif response_type == ResponseType.MOVE:
            return validate_move_format(response)

        elif response_type == ResponseType.INFO:
            if not response.startswith("info "):
                return False, "INFO response must start with 'info'"

        elif response_type == ResponseType.PREPROCESSING_DONE:
            if response != "preprocessingdone":
                return False, "PREPROCESSING_DONE response must be 'preprocessingdone'"

        return True, None
