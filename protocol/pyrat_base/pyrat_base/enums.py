"""Protocol enums for PyRat communication.

This module defines all enum types used in the PyRat protocol communication
between the engine and AI processes.

Example usage:
    >>> from pyrat_base import CommandType, command_from_string
    >>> cmd = command_from_string("go")
    >>> if cmd == CommandType.GO:
    ...     # Handle go command
    ...     pass

    >>> from pyrat_base import ResponseType, response_to_string
    >>> response = response_to_string(ResponseType.MOVE)
    >>> print(response)  # Output: "move"
"""

from enum import Enum, auto, unique
from typing import Optional

# Protocol version this implementation supports
PROTOCOL_VERSION = "1.0"


@unique
class CommandType(Enum):
    """Commands sent from engine to AI.

    See protocol specification section 3 'Protocol Messages' for detailed command syntax.
    Each command corresponds to a specific phase of the game protocol.
    """

    # Handshake
    PYRAT = auto()

    # Synchronization
    ISREADY = auto()

    # Configuration
    SETOPTION = auto()
    DEBUG = auto()

    # Game initialization
    NEWGAME = auto()
    MAZE = auto()
    WALLS = auto()
    MUD = auto()
    CHEESE = auto()
    PLAYER1 = auto()
    PLAYER2 = auto()
    YOUARE = auto()
    TIMECONTROL = auto()

    # Preprocessing
    STARTPREPROCESSING = auto()

    # Turn communication
    MOVES = auto()
    GO = auto()
    STOP = auto()
    TIMEOUT = auto()
    READY = auto()  # Ready check after timeout

    # Game end
    GAMEOVER = auto()

    # Postprocessing
    STARTPOSTPROCESSING = auto()

    # Recovery
    RECOVER = auto()
    MOVES_HISTORY = auto()
    CURRENT_POSITION = auto()
    SCORE = auto()


@unique
class ResponseType(Enum):
    """Responses sent from AI to engine.

    See protocol specification section 3 'Protocol Messages' for detailed response syntax.
    AIs must respond with appropriate response types based on received commands.
    """

    # Handshake
    ID = auto()
    OPTION = auto()
    PYRATREADY = auto()

    # Synchronization
    READYOK = auto()

    # Preprocessing
    PREPROCESSINGDONE = auto()

    # Turn communication
    MOVE = auto()

    # Postprocessing
    POSTPROCESSINGDONE = auto()

    # Ready check
    READY = auto()

    # Information
    INFO = auto()


@unique
class Player(Enum):
    """Player types in the game.

    PyRat is a two-player game with a Rat and a Python competing for cheese.
    See protocol specification section 'Game Initialization' for player setup.
    """

    RAT = "rat"
    PYTHON = "python"


@unique
class GameResult(Enum):
    """Possible game outcomes.

    Games end when a player collects more than half the cheese, all cheese is collected,
    or the maximum turn limit is reached. See protocol specification section 'Game End'.
    """

    RAT = "rat"
    PYTHON = "python"
    DRAW = "draw"


@unique
class OptionType(Enum):
    """Types of configurable options.

    Options allow AIs to expose configurable parameters to the engine.
    See protocol specification section 'Connection Handshake' for option declaration syntax.
    """

    CHECK = "check"  # Boolean option
    SPIN = "spin"  # Integer with min/max range
    COMBO = "combo"  # Choice from predefined values
    STRING = "string"  # Text value
    BUTTON = "button"  # Action trigger


@unique
class InfoType(Enum):
    """Types of info messages that can be sent by AI.

    Info messages provide optional progress information during AI calculation.
    See protocol specification section 'Turn Communication' for info message format.
    """

    NODES = "nodes"  # Number of nodes/states evaluated
    DEPTH = "depth"  # Current search depth
    TIME = "time"  # Time spent in milliseconds
    CURRMOVE = "currmove"  # Move currently being evaluated
    CURRLINE = "currline"  # Current line being analyzed
    SCORE = "score"  # Position evaluation
    PV = "pv"  # Principal variation (best line found)
    TARGET = "target"  # Current target cheese being considered
    STRING = "string"  # Any debug/status message


# Utility functions for string conversion


def command_from_string(s: str) -> Optional[CommandType]:
    """Convert a string to CommandType enum.

    Special cases:
        - "ready?" maps to CommandType.READY (note the question mark)
        - All commands are case-insensitive

    Args:
        s: Command string from protocol

    Returns:
        CommandType enum value or None if not found

    Example:
        >>> command_from_string("ready?")
        <CommandType.READY: 19>
        >>> command_from_string("ISREADY")  # Case insensitive
        <CommandType.ISREADY: 2>
    """
    command_map = {
        "pyrat": CommandType.PYRAT,
        "isready": CommandType.ISREADY,
        "setoption": CommandType.SETOPTION,
        "debug": CommandType.DEBUG,
        "newgame": CommandType.NEWGAME,
        "maze": CommandType.MAZE,
        "walls": CommandType.WALLS,
        "mud": CommandType.MUD,
        "cheese": CommandType.CHEESE,
        "player1": CommandType.PLAYER1,
        "player2": CommandType.PLAYER2,
        "youare": CommandType.YOUARE,
        "timecontrol": CommandType.TIMECONTROL,
        "startpreprocessing": CommandType.STARTPREPROCESSING,
        "moves": CommandType.MOVES,
        "go": CommandType.GO,
        "stop": CommandType.STOP,
        "timeout": CommandType.TIMEOUT,
        "ready?": CommandType.READY,  # Note: protocol uses "ready?" for ready check
        "gameover": CommandType.GAMEOVER,
        "startpostprocessing": CommandType.STARTPOSTPROCESSING,
        "recover": CommandType.RECOVER,
        "moves_history": CommandType.MOVES_HISTORY,
        "current_position": CommandType.CURRENT_POSITION,
        "score": CommandType.SCORE,
    }
    return command_map.get(s.lower())


def response_to_string(r: ResponseType) -> str:
    """Convert ResponseType enum to protocol string.

    Args:
        r: ResponseType enum value

    Returns:
        String representation for protocol
    """
    response_map = {
        ResponseType.ID: "id",
        ResponseType.OPTION: "option",
        ResponseType.PYRATREADY: "pyratready",
        ResponseType.READYOK: "readyok",
        ResponseType.PREPROCESSINGDONE: "preprocessingdone",
        ResponseType.MOVE: "move",
        ResponseType.POSTPROCESSINGDONE: "postprocessingdone",
        ResponseType.READY: "ready",
        ResponseType.INFO: "info",
    }
    return response_map[r]


def player_from_string(s: str) -> Optional[Player]:
    """Convert a string to Player enum.

    Args:
        s: Player string from protocol

    Returns:
        Player enum value or None if not found
    """
    try:
        return Player(s.lower())
    except ValueError:
        return None


def game_result_from_string(s: str) -> Optional[GameResult]:
    """Convert a string to GameResult enum.

    Args:
        s: Game result string from protocol

    Returns:
        GameResult enum value or None if not found
    """
    try:
        return GameResult(s.lower())
    except ValueError:
        return None


def option_type_from_string(s: str) -> Optional[OptionType]:
    """Convert a string to OptionType enum.

    Args:
        s: Option type string from protocol

    Returns:
        OptionType enum value or None if not found
    """
    try:
        return OptionType(s.lower())
    except ValueError:
        return None


def info_type_from_string(s: str) -> Optional[InfoType]:
    """Convert a string to InfoType enum.

    Args:
        s: Info type string from protocol

    Returns:
        InfoType enum value or None if not found
    """
    try:
        return InfoType(s.lower())
    except ValueError:
        return None
