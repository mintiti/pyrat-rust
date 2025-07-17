"""PyRat Base Library for AI Development.

This package provides the base classes and utilities for developing PyRat AIs
that communicate via the PyRat protocol.
"""

from pyrat_base import utils
from pyrat_base.base_ai import PyRatAI
from pyrat_base.enums import (
    PROTOCOL_VERSION,
    CommandType,
    GameResult,
    InfoType,
    OptionType,
    Player,
    ResponseType,
    command_from_string,
    game_result_from_string,
    info_type_from_string,
    option_type_from_string,
    player_from_string,
    response_to_string,
)
from pyrat_base.io_handler import IOHandler
from pyrat_base.protocol import Command, Protocol
from pyrat_base.protocol_state import ProtocolState
from pyrat_base.replay import (
    InitialState,
    Move,
    Replay,
    ReplayMetadata,
    ReplayPlayer,
    ReplayReader,
    ReplayWriter,
    StreamingReplayWriter,
)

__version__ = "0.1.0"

__all__ = [
    "PROTOCOL_VERSION",
    "Command",
    "CommandType",
    "GameResult",
    "IOHandler",
    "InfoType",
    "InitialState",
    "Move",
    "OptionType",
    "Player",
    "Protocol",
    "ProtocolState",
    "PyRatAI",
    "Replay",
    "ReplayMetadata",
    "ReplayPlayer",
    "ReplayReader",
    "ReplayWriter",
    "ResponseType",
    "StreamingReplayWriter",
    "command_from_string",
    "game_result_from_string",
    "info_type_from_string",
    "option_type_from_string",
    "player_from_string",
    "response_to_string",
    "utils",
]
