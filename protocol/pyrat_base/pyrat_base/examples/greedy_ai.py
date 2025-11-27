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

from typing import List, Optional

from pyrat_engine.core.types import Coordinates, Direction

from pyrat_base import ProtocolState, PyRatAI
from pyrat_base.utils import find_nearest_cheese_by_time


def _direction_name(direction: int) -> str:
    """Get the name of a direction (since Direction is now int-based)."""
    if direction == Direction.UP:
        return "UP"
    elif direction == Direction.RIGHT:
        return "RIGHT"
    elif direction == Direction.DOWN:
        return "DOWN"
    elif direction == Direction.LEFT:
        return "LEFT"
    elif direction == Direction.STAY:
        return "STAY"
    else:
        return f"UNKNOWN({direction})"


class GreedyAI(PyRatAI):
    """AI that uses Dijkstra pathfinding to reach cheese in minimum time."""

    def __init__(self) -> None:
        super().__init__("GreedyBot v2.0", "PyRat Team")
        self._current_target: Optional[Coordinates] = None
        self._path_to_target: Optional[List[Direction]] = None
        self._last_position: Optional[Coordinates] = None

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
        # Check if stuck in mud first
        if state.my_mud_turns > 0:
            self.log(f"Stuck in mud for {state.my_mud_turns} more turns")
            return Direction.STAY

        # If no cheese left, stay
        if not state.cheese:
            self.log("No cheese remaining")
            return Direction.STAY

        # Check if we moved (position changed or just got out of mud)
        if self._last_position != state.my_position:
            # Position changed, need to recalculate path
            self._current_target = None
            self._path_to_target = None
            self.log(
                f"Position changed from {self._last_position} to {state.my_position}"
            )

        self._last_position = state.my_position

        # Recalculate path if we don't have one or target changed
        if (
            self._current_target is None
            or self._path_to_target is None
            or len(self._path_to_target) == 0
        ):
            # Find cheese that takes minimum time to reach (accounting for mud)
            result = find_nearest_cheese_by_time(state)

            if result is None:
                self.log("No reachable cheese found!")
                return Direction.STAY

            cheese_pos, path, total_time = result

            # Set new target
            self._current_target = cheese_pos
            self._path_to_target = path
            self.log(
                f"New target: {cheese_pos} (time: {total_time} turns, path: {len(path)} moves)"
            )
            self.send_info(
                target=cheese_pos,
                string=f"Time to reach: {total_time} turns ({len(path)} moves)",
            )

        # Get the first move in the path and consume it
        if self._path_to_target and len(self._path_to_target) > 0:
            move = self._path_to_target[0]
            # Remove this move from the path for next turn
            self._path_to_target = self._path_to_target[1:]

            # Check if this move would take us into mud
            move_cost = state.get_move_cost(move)
            if move_cost and move_cost > 1:
                self.log(
                    f"Following path: {_direction_name(move)} (entering {move_cost}-turn mud)"
                )
            else:
                self.log(f"Following path: {_direction_name(move)}")
            return move

        # Fallback - recalculate path
        self.log("Path exhausted, recalculating...")
        self._current_target = None
        return self.get_move(state)  # Recursive call to recalculate


if __name__ == "__main__":
    # Create and run the AI
    ai = GreedyAI()
    ai.run()
