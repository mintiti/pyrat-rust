"""Tests for BaseAI protocol initialization mapping and player identity.

Validates:
- YOUARE sets Player enum identity (RAT/PYTHON)
- WALLS and MUD are mapped from correct parser keys
- Game state is created once init is complete, with expected dimensions and walls/mud
"""

from __future__ import annotations

import pytest

from pyrat_base.base_ai import PyRatAI
from pyrat_base.enums import Player
from pyrat_base.protocol import Protocol


class _TestAI(PyRatAI):
    def get_move(self, state):  # type: ignore[override]
        raise NotImplementedError


@pytest.fixture()
def ai() -> _TestAI:
    return _TestAI("TestAI")


def _dispatch(ai: _TestAI, cmd_line: str) -> None:
    cmd = Protocol().parse_command(cmd_line)
    assert cmd is not None, f"Failed to parse command: {cmd_line}"
    ai._state = "GAME_INIT"
    ai._handle_game_init(cmd)  # type: ignore[attr-defined]


def test_youare_sets_player_enum(ai: _TestAI) -> None:
    _dispatch(ai, "youare rat")
    assert ai._player == Player.RAT
    _dispatch(ai, "youare python")
    assert ai._player == Player.PYTHON


def test_init_builds_game_state_with_correct_keys(ai: _TestAI) -> None:
    _dispatch(ai, "maze height:5 width:7")
    _dispatch(ai, "walls (0,0)-(0,1) (1,1)-(2,1)")
    _dispatch(ai, "mud (0,1)-(0,2):3")
    _dispatch(ai, "cheese (2,2)")
    _dispatch(ai, "player1 rat (6,4)")
    _dispatch(ai, "player2 python (0,0)")
    _dispatch(ai, "youare rat")
    _dispatch(ai, "timecontrol move:100 preprocessing:3000")

    # Game state should be created
    assert ai._game_state is not None, "Game state was not created after init"
    # Dimensions
    expected_w, expected_h = 7, 5
    assert ai._game_state.width == expected_w  # type: ignore[attr-defined]
    assert ai._game_state.height == expected_h  # type: ignore[attr-defined]
    assert ai._player == Player.RAT

    # Walls and mud should be present and match inputs (order-insensitive)
    walls = ai._game_state.wall_entries()  # type: ignore[attr-defined]
    mud = ai._game_state.mud_entries()  # type: ignore[attr-defined]
    # Convert Wall objects to tuple pairs for comparison
    wall_tuples = {
        (
            min((w.pos1.x, w.pos1.y), (w.pos2.x, w.pos2.y)),
            max((w.pos1.x, w.pos1.y), (w.pos2.x, w.pos2.y)),
        )
        for w in walls
    }
    assert ((0, 0), (0, 1)) in wall_tuples or ((0, 1), (0, 0)) in wall_tuples
    assert ((1, 1), (2, 1)) in wall_tuples or ((2, 1), (1, 1)) in wall_tuples
    # Convert Mud objects to tuple format for comparison
    mud_tuples = {
        (
            min((m.pos1.x, m.pos1.y), (m.pos2.x, m.pos2.y)),
            max((m.pos1.x, m.pos1.y), (m.pos2.x, m.pos2.y)),
            m.value,
        )
        for m in mud
    }
    assert ((0, 1), (0, 2), 3) in mud_tuples or ((0, 2), (0, 1), 3) in mud_tuples
