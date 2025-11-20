"""Tests for BaseAI protocol initialization mapping and player identity.

Covers bugs revealed by logging:
- YOUARE should set Player enum correctly (RAT/PYTHON)
- WALLS and MUD mapping should use correct keys from the parser
- Creating the game state once all required init messages are processed
"""

import pytest

from pyrat_base.base_ai import PyRatAI
from pyrat_base.enums import Player
from pyrat_base.protocol import Protocol


class _TestAI(PyRatAI):
    """Minimal AI subclass for driving internal handlers directly in tests."""

    def get_move(self, state):  # type: ignore[override]
        raise NotImplementedError


@pytest.fixture()
def ai() -> _TestAI:
    return _TestAI("TestAI")


def _dispatch(ai: _TestAI, cmd_line: str) -> None:
    cmd = Protocol().parse_command(cmd_line)
    assert cmd is not None, f"Failed to parse command: {cmd_line}"
    # Drive internal init handler for deterministic unit testing
    if cmd.type.name in {
        "MAZE",
        "WALLS",
        "MUD",
        "CHEESE",
        "PLAYER1",
        "PLAYER2",
        "YOUARE",
        "TIMECONTROL",
        "STARTPREPROCESSING",
        "GO",
        "MOVES_HISTORY",
        "CURRENT_POSITION",
        "SCORE",
    }:
        # Ensure we're in GAME_INIT to accept init messages
        ai._state = "GAME_INIT"
        ai._handle_game_init(cmd)  # type: ignore[attr-defined]
    else:
        pytest.fail(f"Unexpected command in init test: {cmd_line}")


def test_youare_sets_player_enum(ai: _TestAI) -> None:
    # YOUARE should carry Player enum and set identity without string compare
    _dispatch(ai, "youare rat")
    assert ai._player == Player.RAT
    _dispatch(ai, "youare python")
    assert ai._player == Player.PYTHON


def test_init_builds_game_state_with_correct_keys(ai: _TestAI) -> None:
    # Send a minimal but complete game init sequence
    _dispatch(ai, "maze height:5 width:7")
    _dispatch(ai, "walls (0,0)-(0,1) (1,1)-(2,1)")
    _dispatch(ai, "mud (0,1)-(0,2):3")
    _dispatch(ai, "cheese (2,2)")
    _dispatch(ai, "player1 rat (6,4)")
    _dispatch(ai, "player2 python (0,0)")
    _dispatch(ai, "youare rat")
    _dispatch(ai, "timecontrol move:100 preprocessing:3000")

    # Game state should be created once required keys are present
    assert ai._game_state is not None, "Game state was not created after init"
    # Sanity check dimensions and player identity
    # Sanity-check dimensions match the maze declaration
    expected_w, expected_h = 7, 5
    assert ai._game_state.width == expected_w  # type: ignore[attr-defined]
    assert ai._game_state.height == expected_h  # type: ignore[attr-defined]
    assert ai._player == Player.RAT
