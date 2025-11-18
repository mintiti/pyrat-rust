#!/usr/bin/env python3
"""Random AI - Makes random valid moves.

This AI demonstrates:
- Using the game state to check effective moves (moves that result in position change)
- Making random decisions
- Debug logging
"""

import random

from pyrat_engine.core import DirectionType
from pyrat_engine.core.types import Direction, direction_to_name

from pyrat_base import ProtocolState, PyRatAI


class RandomAI(PyRatAI):
    """AI that makes random valid moves."""

    def __init__(self) -> None:
        super().__init__("RandomBot v1.0", "PyRat Team")

    def get_move(self, state: ProtocolState) -> DirectionType:
        """Choose a random effective move."""
        # Get all moves that will actually change our position (or STAY)
        effective_moves = state.get_effective_moves()

        if not effective_moves:
            # No effective moves (shouldn't happen in a well-formed maze)
            self.log("No effective moves available!")
            return Direction.STAY

        # Choose randomly
        move = random.choice(effective_moves)

        # Log our choice if in debug mode
        self.log(
            f"Position: {state.my_position}, Choosing: {direction_to_name(move)}"
        )

        return move


if __name__ == "__main__":
    # Create and run the AI
    ai = RandomAI()
    ai.run()
