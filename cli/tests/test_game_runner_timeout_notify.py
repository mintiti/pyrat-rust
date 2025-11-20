"""Tests for timeout notification and isready probe in GameRunner.

Ensures that when an AI times out (move is None, process alive),
the runner notifies the AI with `timeout move:STAY` and probes liveness via isready.
"""

from __future__ import annotations


from pyrat_engine.core import Direction
from pyrat_runner.game_runner import GameRunner


class _FakeAI:
    def __init__(self):
        self._alive = True
        self.notified = False
        self.probed = False

    def is_alive(self) -> bool:
        return self._alive

    def notify_timeout(self, default_move: Direction) -> None:
        assert default_move == Direction.STAY
        self.notified = True

    def ready_probe(self, timeout: float = 0.5) -> bool:
        self.probed = True
        return True


def test_runner_timeout_paths_calls_notify_and_probe() -> None:
    runner = GameRunner(
        rat_script="/dev/null",
        python_script="/dev/null",
        width=3,
        height=3,
        cheese_count=1,
        seed=1,
        turn_timeout=0.01,
        display_delay=0.0,
        log_dir=None,
    )

    fake_ai = _FakeAI()
    # Exercise only the error handler with a timed-out move
    should_continue, move_to_use = runner._handle_ai_move_error("rat", fake_ai, None)  # type: ignore[arg-type]

    assert should_continue is True
    assert move_to_use == Direction.STAY
    assert fake_ai.notified is True
    assert fake_ai.probed is True
