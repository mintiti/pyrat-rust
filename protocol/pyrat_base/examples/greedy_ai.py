#!/usr/bin/env python3
"""Greedy AI - Moves toward the cheese that can be reached fastest.

This AI uses Dijkstra's algorithm to find the cheese that takes the minimum
number of turns to reach, properly accounting for both walls and mud delays.

For example:
- A cheese 3 moves away through normal passages takes 3 turns
- A cheese 2 moves away but through 3-turn mud takes 4 turns total
- The AI will choose the first option as it's faster

This AI demonstrates:
- Proper pathfinding that accounts for walls AND mud
- Using utility functions from pyrat_base.utils
- Optimal decision making based on actual time cost
"""

from typing import Optional

from pyrat_engine.game import Direction

from pyrat_base import ProtocolState, PyRatAI
from pyrat_base.utils import find_nearest_cheese_by_time, get_direction_toward_target


class GreedyAI(PyRatAI):
    """AI that uses Dijkstra pathfinding to reach cheese in minimum time."""

    def __init__(self):
        super().__init__("GreedyBot v2.0", "PyRat Team")
        self._current_target: Optional[tuple] = None
        self._path_to_target: Optional[list] = None

    def preprocess(self, state: ProtocolState, time_limit_ms: int) -> None:
        """Analyze the maze during preprocessing."""
        self.log(f"Preprocessing with {time_limit_ms}ms time limit")
        self.log(f"Maze size: {state.width}x{state.height}")
        self.log(f"Total cheese: {len(state.cheese)}")

        # Count walls by checking movement matrix
        wall_count = 0
        for x in range(state.width):
            for y in range(state.height):
                for direction in range(4):
                    if state.movement_matrix[x, y, direction] < 0:
                        wall_count += 1

        # Each wall is counted twice (once from each side)
        self.log(f"Total walls: {wall_count // 2}")

    def get_move(self, state: ProtocolState) -> Direction:
        """Move toward the cheese that can be reached in minimum turns."""
        # If no cheese left, stay
        if not state.cheese:
            self.log("No cheese remaining")
            return Direction.STAY

        # Find cheese that takes minimum time to reach (accounting for mud)
        result = find_nearest_cheese_by_time(state)

        if result is None:
            self.log("No reachable cheese found!")
            return Direction.STAY

        cheese_pos, path, total_time = result

        # Log if we found a new target
        if cheese_pos != self._current_target:
            self._current_target = cheese_pos
            self._path_to_target = path
            self.log(
                f"New target: {cheese_pos} (time: {total_time} turns, path: {len(path)} moves)"
            )
            self.send_info(
                target=cheese_pos,
                string=f"Time to reach: {total_time} turns ({len(path)} moves)",
            )

        # Get the first move in the path
        if path and len(path) > 0:
            move = path[0]
            self.log(f"Following path: {move.name}")
            return move

        # Fallback: use simple direction toward target
        self.log("Using fallback movement")
        return get_direction_toward_target(state, cheese_pos)


if __name__ == "__main__":
    # Create and run the AI
    ai = GreedyAI()
    ai.run()
