#!/usr/bin/env python3
"""Dummy AI - Always stays in place.

This is the simplest possible PyRat AI that demonstrates the basic structure.
It always returns STAY regardless of the game state.
"""

from pyrat_engine.core.types import Direction

from pyrat_base import ProtocolState, PyRatAI


class DummyAI(PyRatAI):
    """AI that always stays in place."""

    def __init__(self) -> None:
        super().__init__("DummyBot v1.0", "PyRat Team")

    def get_move(self, state: ProtocolState) -> Direction:
        """Always return STAY."""
        return Direction.STAY


if __name__ == "__main__":
    # Create and run the AI
    ai = DummyAI()
    ai.run()
