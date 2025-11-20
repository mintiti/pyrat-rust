"""Tests for AIProcess init messages alignment with protocol spec.

Verifies send_game_start emits:
- player1 rat (x,y)
- player2 python (x,y)
- timecontrol move:[ms] preprocessing:[ms]
"""

from __future__ import annotations

from typing import List


from pyrat_engine import PyRat
from pyrat_runner.ai_process import AIProcess


class _CapturingAI(AIProcess):
    def __init__(self, *args, **kwargs):  # type: ignore[no-untyped-def]
        super().__init__(*args, **kwargs)
        self.sent: List[str] = []

    def _write_line(self, line: str):  # type: ignore[override]
        # Capture instead of writing to a process
        self.sent.append(line)


def test_send_game_start_emits_protocol_compliant_messages(tmp_path) -> None:
    game = PyRat(width=7, height=5, cheese_count=1, seed=123)
    ai = _CapturingAI(script_path="/dev/null", player_name="rat", timeout=0.5)

    ai.send_game_start(game_state=game, preprocessing_time=0.1)

    # Expect player1/player2 and timecontrol move/preprocessing
    combined = "\n".join(ai.sent)
    assert "player1 rat (" in combined
    assert "player2 python (" in combined
    assert "timecontrol move:" in combined and " preprocessing:" in combined
