"""Protocol parsing and formatting for PyRat communication.

This module handles converting between text-based protocol messages and
structured Python objects for the PyRat AI protocol.

Note on linting exceptions:
- PLR2004 (magic numbers): The numbers used throughout this module (2, 3, 5, etc.)
  are not "magic" but are part of the protocol specification. They represent the
  exact number of tokens expected for each command format.
- ERA001 (commented code): Comments showing command syntax are documentation,
  not commented-out code.
"""
# ruff: noqa: PLR2004, ERA001

from dataclasses import dataclass
from typing import Any, Dict, Optional, Tuple

from pyrat_engine.game import Direction

from pyrat_base.enums import (
    CommandType,
    Player,
    ResponseType,
    command_from_string,
    game_result_from_string,
    player_from_string,
)


@dataclass
class Command:
    """Structured representation of a protocol command.

    Attributes:
        type: The command type enum
        data: Command-specific data as a dictionary
    """

    type: CommandType
    data: Dict[str, Any]


class Protocol:
    """Protocol message parser and formatter."""

    @staticmethod
    def parse_command(line: str) -> Optional[Command]:  # noqa: C901, PLR0911, PLR0912, PLR0915
        """Parse a protocol command into structured data.

        Args:
            line: Raw command string from the engine

        Returns:
            Command object with parsed data, or None if parsing fails

        Note:
            This function has high complexity metrics due to handling 25+ different
            command types from the protocol specification. The complexity is inherent
            to the protocol design and cannot be reasonably reduced without making
            the code harder to understand or maintain.
        """
        line = line.strip()
        if not line:
            return None

        parts = line.split()
        if not parts:
            return None

        # Get command type
        cmd_str = parts[0].lower()
        cmd_type = command_from_string(cmd_str)
        if cmd_type is None:
            return None

        # Parse based on command type
        try:
            if cmd_type == CommandType.PYRAT:
                return Command(cmd_type, {})

            elif cmd_type == CommandType.ISREADY:
                return Command(cmd_type, {})

            elif cmd_type == CommandType.SETOPTION:
                # setoption name [name] value [value]
                if len(parts) < 5 or parts[1] != "name" or "value" not in parts:
                    return None
                value_idx = parts.index("value")
                name = " ".join(parts[2:value_idx])
                value = " ".join(parts[value_idx + 1 :])
                return Command(cmd_type, {"name": name, "value": value})

            elif cmd_type == CommandType.DEBUG:
                # debug [on|off]
                if len(parts) != 2 or parts[1] not in ["on", "off"]:
                    return None
                return Command(cmd_type, {"enabled": parts[1] == "on"})

            elif cmd_type == CommandType.NEWGAME:
                return Command(cmd_type, {})

            elif cmd_type == CommandType.MAZE:
                # maze height:[H] width:[W]
                if len(parts) != 3:
                    return None
                maze_data = {}
                for part in parts[1:]:
                    if ":" not in part:
                        return None
                    key, value = part.split(":", 1)
                    if key not in ["height", "width"]:
                        return None
                    maze_data[key] = int(value)
                if "height" not in maze_data or "width" not in maze_data:
                    return None
                return Command(cmd_type, maze_data)

            elif cmd_type == CommandType.WALLS:
                # walls [list of wall positions as (x1,y1)-(x2,y2)]
                walls = []
                for part in parts[1:]:
                    wall = _parse_wall(part)
                    if wall is None:
                        return None
                    walls.append(wall)
                return Command(cmd_type, {"walls": walls})

            elif cmd_type == CommandType.MUD:
                # mud [list of mud positions as (x1,y1)-(x2,y2):N]
                mud_list = []
                for part in parts[1:]:
                    mud = _parse_mud(part)
                    if mud is None:
                        return None
                    mud_list.append(mud)
                return Command(cmd_type, {"mud": mud_list})

            elif cmd_type == CommandType.CHEESE:
                # cheese [list of cheese positions as (x,y)]
                cheese = []
                for part in parts[1:]:
                    pos = _parse_position(part)
                    if pos is None:
                        return None
                    cheese.append(pos)
                return Command(cmd_type, {"cheese": cheese})

            elif cmd_type == CommandType.PLAYER1:
                # player1 rat (x,y)
                if len(parts) != 3 or parts[1] != "rat":
                    return None
                pos = _parse_position(parts[2])
                if pos is None:
                    return None
                return Command(cmd_type, {"position": pos})

            elif cmd_type == CommandType.PLAYER2:
                # player2 python (x,y)
                if len(parts) != 3 or parts[1] != "python":
                    return None
                pos = _parse_position(parts[2])
                if pos is None:
                    return None
                return Command(cmd_type, {"position": pos})

            elif cmd_type == CommandType.YOUARE:
                # youare [rat|python]
                if len(parts) != 2:
                    return None
                player = player_from_string(parts[1])
                if player is None:
                    return None
                return Command(cmd_type, {"player": player})

            elif cmd_type == CommandType.TIMECONTROL:
                # timecontrol move:[ms] preprocessing:[ms] postprocessing:[ms]
                if len(parts) < 2:  # Require at least one time parameter
                    return None
                time_data = {}
                for part in parts[1:]:
                    if ":" not in part:
                        return None
                    key, value = part.split(":", 1)
                    if key not in ["move", "preprocessing", "postprocessing"]:
                        return None
                    time_data[key] = int(value)
                return Command(cmd_type, time_data)

            elif cmd_type == CommandType.STARTPREPROCESSING:
                return Command(cmd_type, {})

            elif cmd_type == CommandType.MOVES:
                # moves rat:[MOVE] python:[MOVE]
                if len(parts) != 3:
                    return None
                moves: Dict[Player, str] = {}
                for part in parts[1:]:
                    if ":" not in part:
                        return None
                    player_str, move_str = part.split(":", 1)
                    player = player_from_string(player_str)
                    if player is None:
                        return None
                    move = _parse_move(move_str)
                    if move is None:
                        return None
                    moves[player] = move
                if Player.RAT not in moves or Player.PYTHON not in moves:
                    return None
                return Command(cmd_type, {"moves": moves})

            elif cmd_type == CommandType.GO:
                return Command(cmd_type, {})

            elif cmd_type == CommandType.STOP:
                return Command(cmd_type, {})

            elif cmd_type == CommandType.TIMEOUT:
                # timeout move:STAY or timeout preprocessing or timeout postprocessing
                if len(parts) < 2:
                    return None
                if len(parts) == 2 and parts[1] in ["preprocessing", "postprocessing"]:
                    return Command(cmd_type, {"phase": parts[1]})
                elif len(parts) == 2 and ":" in parts[1]:
                    key, value = parts[1].split(":", 1)
                    if key == "move":
                        move = _parse_move(value)
                        if move is None:
                            return None
                        return Command(cmd_type, {"move": move})
                return None

            elif cmd_type == CommandType.READY:
                # This is "ready?" from engine
                return Command(cmd_type, {})

            elif cmd_type == CommandType.GAMEOVER:
                # gameover winner:[rat|python|draw] score:[X]-[Y]
                if len(parts) != 3:
                    return None
                gameover_data: Dict[str, Any] = {}
                for part in parts[1:]:
                    if ":" not in part:
                        return None
                    key, value = part.split(":", 1)
                    if key == "winner":
                        winner = game_result_from_string(value)
                        if winner is None:
                            return None
                        gameover_data["winner"] = winner
                    elif key == "score":
                        if "-" not in value:
                            return None
                        score_parts = value.split("-")
                        if len(score_parts) != 2:
                            return None
                        gameover_data["score"] = (
                            float(score_parts[0]),
                            float(score_parts[1]),
                        )
                if "winner" not in gameover_data or "score" not in gameover_data:
                    return None
                return Command(cmd_type, gameover_data)

            elif cmd_type == CommandType.STARTPOSTPROCESSING:
                return Command(cmd_type, {})

            elif cmd_type == CommandType.RECOVER:
                return Command(cmd_type, {})

            elif cmd_type == CommandType.MOVES_HISTORY:
                # moves_history [list of all moves]
                history = []
                for part in parts[1:]:
                    move = _parse_move(part)
                    if move is None:
                        return None
                    history.append(move)
                return Command(cmd_type, {"history": history})

            elif cmd_type == CommandType.CURRENT_POSITION:
                # current_position rat:(x,y) python:(x,y)
                if len(parts) != 3:
                    return None
                positions: Dict[Player, Tuple[int, int]] = {}
                for part in parts[1:]:
                    if ":" not in part:
                        return None
                    player_str, pos_str = part.split(":", 1)
                    player = player_from_string(player_str)
                    if player is None:
                        return None
                    pos = _parse_position(pos_str)
                    if pos is None:
                        return None
                    positions[player] = pos
                if Player.RAT not in positions or Player.PYTHON not in positions:
                    return None
                return Command(cmd_type, {"positions": positions})

            elif cmd_type == CommandType.SCORE:
                # score rat:[X] python:[Y]
                if len(parts) != 3:
                    return None
                scores: Dict[Player, float] = {}
                for part in parts[1:]:
                    if ":" not in part:
                        return None
                    player_str, score_str = part.split(":", 1)
                    player = player_from_string(player_str)
                    if player is None:
                        return None
                    scores[player] = float(score_str)
                if Player.RAT not in scores or Player.PYTHON not in scores:
                    return None
                return Command(cmd_type, {"scores": scores})

        except (ValueError, IndexError):
            return None

    @staticmethod
    def format_response(  # noqa: C901, PLR0911, PLR0912
        response_type: ResponseType, data: Optional[Dict[str, Any]] = None
    ) -> str:
        """Format a response for the protocol.

        Args:
            response_type: The type of response to format
            data: Optional data for the response

        Returns:
            Formatted response string

        Note:
            This function handles all 9 response types defined in the protocol.
            The complexity is necessary to provide complete protocol compliance.
        """
        if data is None:
            data = {}

        if response_type == ResponseType.ID:
            # id name [name] or id author [author]
            if "name" in data:
                return f"id name {data['name']}"
            elif "author" in data:
                return f"id author {data['author']}"
            else:
                raise ValueError("ID response requires 'name' or 'author' in data")

        elif response_type == ResponseType.OPTION:
            # option name [name] type [type] default [value] [additional parameters]
            return _format_option(data)

        elif response_type == ResponseType.PYRATREADY:
            return "pyratready"

        elif response_type == ResponseType.READYOK:
            return "readyok"

        elif response_type == ResponseType.PREPROCESSINGDONE:
            return "preprocessingdone"

        elif response_type == ResponseType.MOVE:
            # move [UP|DOWN|LEFT|RIGHT|STAY]
            if "move" not in data:
                raise ValueError("MOVE response requires 'move' in data")
            move = data["move"]
            if isinstance(move, Direction):
                move = move.name
            return f"move {move}"

        elif response_type == ResponseType.POSTPROCESSINGDONE:
            return "postprocessingdone"

        elif response_type == ResponseType.READY:
            return "ready"

        elif response_type == ResponseType.INFO:
            # info [key value pairs]
            parts = ["info"]
            for key, value in data.items():
                if key == "string":
                    # Special case: string values go at the end
                    continue
                elif key in ["currline", "pv"] and isinstance(value, list):
                    # List of moves
                    parts.append(f"{key}")
                    parts.extend(value)
                elif key == "target" and isinstance(value, tuple) and len(value) == 2:
                    # Target position
                    parts.append(f"{key} ({value[0]},{value[1]})")
                else:
                    # Simple key-value
                    parts.append(f"{key} {value}")

            # Add string message at the end if present
            if "string" in data:
                parts.append(f"string {data['string']}")

            return " ".join(parts)

        else:
            raise ValueError(f"Unknown response type: {response_type}")


# Helper functions


def _parse_position(s: str) -> Optional[Tuple[int, int]]:
    """Parse a position string like (x,y) into a tuple."""
    s = s.strip()
    if not s.startswith("(") or not s.endswith(")"):
        return None
    s = s[1:-1]  # Remove parentheses
    parts = s.split(",")
    if len(parts) != 2:
        return None
    try:
        x = int(parts[0].strip())
        y = int(parts[1].strip())
        return (x, y)
    except ValueError:
        return None


def _parse_wall(s: str) -> Optional[Tuple[Tuple[int, int], Tuple[int, int]]]:
    """Parse a wall string like (x1,y1)-(x2,y2) into a tuple of positions."""
    parts = s.split("-")
    if len(parts) != 2:
        return None
    pos1 = _parse_position(parts[0])
    pos2 = _parse_position(parts[1])
    if pos1 is None or pos2 is None:
        return None
    return (pos1, pos2)


def _parse_mud(s: str) -> Optional[Tuple[Tuple[int, int], Tuple[int, int], int]]:
    """Parse a mud string like (x1,y1)-(x2,y2):N into positions and cost."""
    if ":" not in s:
        return None
    wall_part, cost_part = s.rsplit(":", 1)
    wall = _parse_wall(wall_part)
    if wall is None:
        return None
    try:
        cost = int(cost_part)
        return (wall[0], wall[1], cost)
    except ValueError:
        return None


def _parse_move(s: str) -> Optional[str]:
    """Parse and validate a move string."""
    s = s.upper()
    valid_moves = [d.name for d in Direction]
    if s in valid_moves:
        return s
    return None


def _format_option(data: Dict[str, Any]) -> str:
    """Format an option declaration."""
    if "name" not in data or "type" not in data:
        raise ValueError("Option requires 'name' and 'type' in data")

    parts = ["option", f"name {data['name']}", f"type {data['type']}"]

    if "default" in data:
        parts.append(f"default {data['default']}")

    # Add type-specific parameters
    option_type = data.get("type")
    if option_type == "spin":
        if "min" in data:
            parts.append(f"min {data['min']}")
        if "max" in data:
            parts.append(f"max {data['max']}")
    elif option_type == "combo":
        if "values" in data:
            for value in data["values"]:
                parts.append(f"var {value}")

    return " ".join(parts)
