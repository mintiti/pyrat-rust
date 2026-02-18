"""Shared pytest fixtures for PyRat protocol tests."""

import io
import sys
from contextlib import contextmanager
from typing import List, Optional, Tuple

import pytest
from pyrat_engine import GameBuilder

from pyrat_base import Protocol, ProtocolState
from pyrat_base.enums import Player


@pytest.fixture
def basic_game_config():
    """Standard game configurations for testing."""
    return {
        "small": {"width": 5, "height": 5},
        "medium": {"width": 10, "height": 10},
        "standard": {"width": 21, "height": 15},
    }


@pytest.fixture
def protocol_commands():  # noqa: C901
    """Factory for creating protocol command strings."""

    class CommandFactory:
        @staticmethod
        def handshake() -> str:
            return "pyrat"

        @staticmethod
        def newgame() -> str:
            return "newgame"

        @staticmethod
        def maze(width: int, height: int) -> str:
            return f"maze width:{width} height:{height}"

        @staticmethod
        def walls(*walls: Tuple[Tuple[int, int], Tuple[int, int]]) -> str:
            if not walls:
                return "walls"
            wall_strs = []
            for (x1, y1), (x2, y2) in walls:
                wall_strs.append(f"({x1},{y1})-({x2},{y2})")
            return f"walls {' '.join(wall_strs)}"

        @staticmethod
        def mud(*mud_specs: Tuple[Tuple[Tuple[int, int], Tuple[int, int]], int]) -> str:
            if not mud_specs:
                return "mud"
            mud_strs = []
            for (pos1, pos2), value in mud_specs:
                mud_strs.append(f"({pos1[0]},{pos1[1]})-({pos2[0]},{pos2[1]}):{value}")
            return f"mud {' '.join(mud_strs)}"

        @staticmethod
        def cheese(*positions: Tuple[int, int]) -> str:
            if not positions:
                return "cheese"
            pos_strs = [f"({x},{y})" for x, y in positions]
            return f"cheese {' '.join(pos_strs)}"

        @staticmethod
        def player(player_num: int, name: str, x: int, y: int) -> str:
            return f"player{player_num} {name} ({x},{y})"

        @staticmethod
        def youare(player: str) -> str:
            return f"youare {player}"

        @staticmethod
        def go(turn_time_ms: Optional[int] = None) -> str:
            if turn_time_ms is None:
                return "go"
            return f"go {turn_time_ms}"

        @staticmethod
        def stop() -> str:
            return "stop"

        @staticmethod
        def moves(rat_move: str, python_move: str) -> str:
            return f"moves rat:{rat_move} python:{python_move}"

        @staticmethod
        def gameover(
            rat_score: float, python_score: float, reason: str = "all_cheese_collected"
        ) -> str:
            return f"gameover rat:{rat_score} python:{python_score} reason:{reason}"

    return CommandFactory()


@pytest.fixture
def mock_io():
    """Mock stdin/stdout for testing AI communication."""

    @contextmanager
    def _mock(input_lines: List[str]):
        old_stdin = sys.stdin
        old_stdout = sys.stdout

        # Create mock stdin with provided input
        mock_stdin = io.StringIO("\n".join(input_lines) + "\n")
        mock_stdout = io.StringIO()

        sys.stdin = mock_stdin
        sys.stdout = mock_stdout

        try:
            yield {
                "stdin": mock_stdin,
                "stdout": mock_stdout,
                "get_output": lambda: mock_stdout.getvalue(),
                "get_output_lines": lambda: mock_stdout.getvalue().strip().split("\n")
                if mock_stdout.getvalue().strip()
                else [],
            }
        finally:
            sys.stdin = old_stdin
            sys.stdout = old_stdout

    return _mock


@pytest.fixture
def game_state_builder():
    """Factory for creating game states with ProtocolState wrapper."""

    def create_game(
        width: int = 5, height: int = 5, player: Player = Player.RAT
    ) -> Tuple[GameBuilder, Player]:
        """
        Returns a tuple of (builder, player) to use with ProtocolState.

        Example:
            builder, player = game_state_builder()
            config = (builder.with_open_maze()
                      .with_corner_positions()
                      .with_custom_cheese([(2, 2)])
                      .build())
            game = config.create()
            state = ProtocolState(game, player)
        """
        return GameBuilder(width, height), player

    # Also provide a direct method to build ProtocolState
    def build_protocol_state(
        width: int = 5,
        height: int = 5,
        walls: Optional[List[Tuple[Tuple[int, int], Tuple[int, int]]]] = None,
        mud: Optional[List[Tuple[Tuple[int, int], Tuple[int, int], int]]] = None,
        cheese: Optional[List[Tuple[int, int]]] = None,
        player1_pos: Tuple[int, int] = (0, 0),
        player2_pos: Optional[Tuple[int, int]] = None,
        player: Player = Player.RAT,
    ) -> ProtocolState:
        """Build a ProtocolState directly using GameBuilder."""
        if player2_pos is None:
            player2_pos = (width - 1, height - 1)

        config = (
            GameBuilder(width, height)
            .with_custom_maze(walls=walls or [], mud=mud or [])
            .with_custom_positions(player1_pos, player2_pos)
            .with_custom_cheese(cheese or [(width // 2, height // 2)])
            .build()
        )
        game = config.create()
        return ProtocolState(game, player)

    # Return both methods
    create_game.build = build_protocol_state
    return create_game


@pytest.fixture
def sample_mazes():
    """Collection of predefined maze configurations for testing."""
    return {
        "empty_5x5": {
            "width": 5,
            "height": 5,
            "walls": [],
            "mud": {},
            "cheese": [(2, 2)],
        },
        "simple_maze": {
            "width": 5,
            "height": 5,
            "walls": [
                ((0, 0), (1, 0)),  # Wall between (0,0) and (1,0)
                ((2, 1), (2, 2)),  # Wall between (2,1) and (2,2)
            ],
            "mud": {
                ((1, 1), (1, 2)): 2,  # Mud between (1,1) and (1,2) with cost 2
            },
            "cheese": [(0, 4), (4, 0), (2, 2)],
        },
        "complex_maze": {
            "width": 10,
            "height": 10,
            "walls": [
                # Create a maze with multiple paths
                ((0, 5), (1, 5)),
                ((1, 5), (2, 5)),
                ((3, 5), (4, 5)),
                ((4, 5), (5, 5)),
                ((5, 4), (5, 5)),
                ((5, 5), (5, 6)),
            ],
            "mud": {
                ((2, 5), (3, 5)): 3,  # Mud bridge with cost 3
                ((7, 7), (8, 7)): 2,  # Another mud section
            },
            "cheese": [(1, 1), (8, 8), (5, 2), (3, 7)],
        },
    }


@pytest.fixture
def protocol_parser():
    """Convenience wrapper for protocol parsing."""
    return Protocol()


@pytest.fixture
def valid_protocol_sequences():
    """Valid protocol command sequences for different game phases."""
    return {
        "handshake": [
            "pyrat",
            # AI responds with pyratai, possibly setoption, then pyratready
        ],
        "game_init": [
            "newgame",
            "maze width:5 height:5",
            "walls",
            "mud",
            "cheese (2,2)",
            "player1 rat (0,0)",
            "player2 python (4,4)",
            "youare rat",
        ],
        "preprocessing": [
            "startpreprocessing 3000",
            # AI does preprocessing
            "preprocessingdone",  # Or timeout
        ],
        "turn": [
            "go",
            # AI responds with move
            "moves rat:UP python:LEFT",
            "current_position rat (0,1)",
            "current_position python (3,4)",
        ],
        "game_end": [
            "gameover rat:1.0 python:0.0 reason:all_cheese_collected",
        ],
    }
