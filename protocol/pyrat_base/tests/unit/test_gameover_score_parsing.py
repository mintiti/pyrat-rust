"""Tests for robust GAMEOVER score parsing in BaseAI.

Accepts tuple form from parser and string form for backward compatibility.
"""

from __future__ import annotations

from typing import Any

import pytest

from pyrat_base.base_ai import PyRatAI
from pyrat_base.enums import CommandType
from pyrat_base.protocol import Command


class _ScoreAI(PyRatAI):
    def get_move(self, state):  # type: ignore[override]
        raise NotImplementedError


@pytest.fixture()
def ai() -> _ScoreAI:
    return _ScoreAI("ScoreAI")


def _handle_playing(ai: _ScoreAI, cmd: Command) -> None:
    ai._state = "PLAYING"
    ai._handle_playing(cmd)  # type: ignore[attr-defined]


@pytest.mark.parametrize(
    "score_val,expected",
    [
        ((1.0, 2.5), (1.0, 2.5)),
        ("3-4", (3.0, 4.0)),
    ],
)
def test_gameover_score_parsing(
    ai: _ScoreAI, score_val: Any, expected: tuple[float, float]
) -> None:
    cmd = Command(CommandType.GAMEOVER, {"winner": "draw", "score": score_val})
    _handle_playing(ai, cmd)
    assert ai._final_score == expected
