"""Utility functions for PyRat AI development.

This module provides essential helper functions for developing PyRat AIs,
focusing on pathfinding that accounts for both walls and mud.
"""

import heapq
from typing import Dict, List, Optional, Tuple

from pyrat_engine.core.types import Coordinates, Direction

from .protocol_state import ProtocolState


def direction_to_offset(direction: Direction) -> Tuple[int, int]:
    """Convert a Direction to position offset.

    In PyRat's coordinate system:
    - (0,0) is bottom-left
    - UP increases y (toward top)
    - DOWN decreases y (toward bottom)

    Args:
        direction: Direction enum

    Returns:
        Position offset as (dx, dy)
    """
    if direction == Direction.UP:
        return (0, 1)  # UP increases y
    elif direction == Direction.RIGHT:
        return (1, 0)
    elif direction == Direction.DOWN:
        return (0, -1)  # DOWN decreases y
    elif direction == Direction.LEFT:
        return (-1, 0)
    else:  # STAY
        return (0, 0)


def offset_to_direction(dx: int, dy: int) -> Optional[Direction]:
    """Convert position offset to Direction.

    Args:
        dx: X offset
        dy: Y offset

    Returns:
        Direction enum or None if not a unit move
    """
    if dx == 0 and dy == 1:
        return Direction.UP  # UP is +y
    elif dx == 1 and dy == 0:
        return Direction.RIGHT
    elif dx == 0 and dy == -1:
        return Direction.DOWN  # DOWN is -y
    elif dx == -1 and dy == 0:
        return Direction.LEFT
    elif dx == 0 and dy == 0:
        return Direction.STAY
    else:
        return None


def position_after_move(pos: Coordinates, direction: Direction) -> Coordinates:
    """Calculate position after moving in a direction.

    Args:
        pos: Current position
        direction: Direction to move

    Returns:
        New position after move
    """
    dx, dy = direction_to_offset(direction)
    return Coordinates(pos.x + dx, pos.y + dy)


def _position_after_move(pos: Coordinates, direction: Direction) -> Coordinates:
    """Calculate position after moving in a direction (internal helper).

    Args:
        pos: Current position
        direction: Direction to move

    Returns:
        New position after move
    """
    dx, dy = direction_to_offset(direction)
    return Coordinates(pos.x + dx, pos.y + dy)


def find_fastest_path_dijkstra(
    state: ProtocolState, start: Coordinates, goal: Coordinates
) -> Optional[List[Direction]]:
    """Find the fastest path using Dijkstra's algorithm, accounting for mud.

    This finds the path that takes the minimum number of turns to traverse,
    where mud passages cost more turns than normal passages.

    Args:
        state: Current game state
        start: Starting position
        goal: Goal position

    Returns:
        List of directions for the fastest path, or None if no path exists
    """
    if start == goal:
        return []

    # Priority queue: (total_cost, counter, position, path)
    # counter is used as tie-breaker to avoid comparing Coordinates
    counter = 0
    pq: List[Tuple[int, int, Coordinates, List[Direction]]] = [(0, counter, start, [])]
    # Best known cost to reach each position
    best_cost: Dict[Coordinates, int] = {start: 0}

    while pq:
        current_cost, _, current_pos, path = heapq.heappop(pq)

        # Skip if we've found a better path to this position
        if current_cost > best_cost.get(current_pos, float("inf")):
            continue

        # Try each direction
        directions = [Direction.UP, Direction.RIGHT, Direction.DOWN, Direction.LEFT]
        for direction in directions:
            # Calculate next position with bounds checking BEFORE creating Coordinates
            dx, dy = direction_to_offset(direction)
            next_x = current_pos.x + dx
            next_y = current_pos.y + dy

            # Check bounds before creating Coordinates (which rejects negatives)
            if not (0 <= next_x < state.width and 0 <= next_y < state.height):
                continue

            next_pos = Coordinates(next_x, next_y)

            # Get movement cost from movement matrix
            x, y = current_pos.x, current_pos.y
            movement_cost = state.movement_matrix[x, y, direction]
            if movement_cost < 0:  # Wall or boundary
                continue

            # Calculate total cost to reach next position
            # Movement cost is 1 for normal move, or mud value for mud passages
            edge_cost = 1 if movement_cost == 0 else movement_cost
            new_cost = current_cost + edge_cost

            # If this is a better path to next_pos, update it
            if new_cost < best_cost.get(next_pos, float("inf")):
                best_cost[next_pos] = new_cost
                new_path = [*path, direction]

                # Check if we reached the goal
                if next_pos == goal:
                    return new_path

                # Add to priority queue with counter as tie-breaker
                counter += 1
                heapq.heappush(pq, (new_cost, counter, next_pos, new_path))

    return None  # No path found


def find_nearest_cheese_by_time(
    state: ProtocolState,
) -> Optional[Tuple[Coordinates, List[Direction], int]]:
    """Find the cheese that can be reached in the minimum number of turns.

    This uses Dijkstra's algorithm to find the cheese that takes the
    least time to reach, properly accounting for mud delays.

    Args:
        state: Current game state

    Returns:
        Tuple of (cheese_position, path_to_cheese, total_turns) or None
    """
    if not state.cheese:
        return None

    my_pos = state.my_position
    best_cheese: Optional[Coordinates] = None
    best_path: Optional[List[Direction]] = None
    best_time: float = float("inf")

    # Run Dijkstra from my position to all positions
    # Priority queue: (total_cost, counter, position, path)
    counter = 0
    pq: List[Tuple[int, int, Coordinates, List[Direction]]] = [(0, counter, my_pos, [])]
    best_cost: Dict[Coordinates, int] = {my_pos: 0}
    paths_to_positions: Dict[Coordinates, List[Direction]] = {my_pos: []}

    while pq:
        current_cost, _, current_pos, path = heapq.heappop(pq)

        # Skip if we've found a better path to this position
        if current_cost > best_cost.get(current_pos, float("inf")):
            continue

        # Check if this position has cheese
        if current_pos in state.cheese and current_cost < best_time:
            best_cheese = current_pos
            best_path = path
            best_time = current_cost

        # Try each direction
        directions = [Direction.UP, Direction.RIGHT, Direction.DOWN, Direction.LEFT]
        for direction in directions:
            # Calculate next position with bounds checking BEFORE creating Coordinates
            dx, dy = direction_to_offset(direction)
            next_x = current_pos.x + dx
            next_y = current_pos.y + dy

            # Check bounds before creating Coordinates (which rejects negatives)
            if not (0 <= next_x < state.width and 0 <= next_y < state.height):
                continue

            next_pos = Coordinates(next_x, next_y)

            # Get movement cost
            x, y = current_pos.x, current_pos.y
            movement_cost = state.movement_matrix[x, y, direction]
            if movement_cost < 0:  # Wall or boundary
                continue

            # Calculate total cost
            edge_cost = 1 if movement_cost == 0 else movement_cost
            new_cost = current_cost + edge_cost

            # If this is a better path to next_pos, update it
            if new_cost < best_cost.get(next_pos, float("inf")):
                best_cost[next_pos] = new_cost
                new_path = [*path, direction]
                paths_to_positions[next_pos] = new_path

                # Add to priority queue with counter as tie-breaker
                counter += 1
                heapq.heappush(pq, (new_cost, counter, next_pos, new_path))

    if best_cheese is not None and best_path is not None:
        return (best_cheese, best_path, int(best_time))

    return None


def get_direction_toward_target(state: ProtocolState, target: Coordinates) -> Direction:
    """Get the best direction to move toward a target using Dijkstra pathfinding.

    This finds the fastest path (accounting for mud) to the target and
    returns the first move in that path. Falls back to STAY if no path exists.

    Args:
        state: Current game state
        target: Target position

    Returns:
        Best direction to move toward target
    """
    path = find_fastest_path_dijkstra(state, state.my_position, target)
    if path and len(path) > 0:
        return path[0]
    return Direction.STAY
