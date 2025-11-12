#!/usr/bin/env python3
"""Random AI - Makes random valid moves.

This AI demonstrates:
- Using the game state to check effective moves (moves that result in position change)
- Making random decisions
- Debug logging
"""

import random

from pyrat_engine.game import Direction

from pyrat_base import ProtocolState, PyRatAI
from pyrat_base.protocol import DIRECTION_INT_TO_NAME


class RandomAI(PyRatAI):
    """AI that makes random valid moves."""

    def __init__(self):
        super().__init__("RandomBot v1.0", "PyRat Team")

    def get_move(self, state: ProtocolState) -> Direction:
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
            f"Position: {state.my_position}, Choosing: {DIRECTION_INT_TO_NAME[move]}"
        )

        return move


if __name__ == "__main__":
    # Create and run the AI
    ai = RandomAI()
    ai.run()
