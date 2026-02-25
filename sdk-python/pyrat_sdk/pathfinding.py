"""Dijkstra pathfinding on the Maze graph.

Layer 3 — provides shortest_path, nearest_cheese, distances_from.
Direction offsets: UP=(0,1), RIGHT=(1,0), DOWN=(0,-1), LEFT=(-1,0).
"""

from __future__ import annotations

import heapq
from typing import TYPE_CHECKING

if TYPE_CHECKING:
    from pyrat_sdk.maze import Maze

# Direction enum values and their (dx, dy) offsets.
_UP, _RIGHT, _DOWN, _LEFT = 0, 1, 2, 3
_OFFSETS = {
    (0, 1): _UP,
    (1, 0): _RIGHT,
    (0, -1): _DOWN,
    (-1, 0): _LEFT,
}


def _direction_from_to(a: tuple[int, int], b: tuple[int, int]) -> int:
    """Return the Direction int for moving from *a* to *b* (must be adjacent)."""
    dx, dy = b[0] - a[0], b[1] - a[1]
    return _OFFSETS[(dx, dy)]


def shortest_path(
    maze: Maze,
    start: tuple[int, int],
    goal: tuple[int, int],
) -> tuple[list[int], int] | None:
    """Dijkstra with early exit. Returns ``(directions, cost)`` or None."""
    if start == goal:
        return ([], 0)

    counter = 0
    # (cost, counter, position, prev_positions_list)
    pq: list[tuple[int, int, tuple[int, int]]] = [(0, counter, start)]
    best: dict[tuple[int, int], int] = {start: 0}
    prev: dict[tuple[int, int], tuple[int, int] | None] = {start: None}

    while pq:
        cost, _, pos = heapq.heappop(pq)
        if cost > best.get(pos, float("inf")):  # type: ignore[arg-type]
            continue
        if pos == goal:
            # Reconstruct path as directions.
            path_dirs: list[int] = []
            cur = goal
            while prev[cur] is not None:
                p = prev[cur]
                assert p is not None
                path_dirs.append(_direction_from_to(p, cur))
                cur = p
            path_dirs.reverse()
            return (path_dirs, cost)

        for nb, weight in maze.get_neighbors(pos).items():
            new_cost = cost + weight
            if new_cost < best.get(nb, float("inf")):  # type: ignore[arg-type]
                best[nb] = new_cost
                prev[nb] = pos
                counter += 1
                heapq.heappush(pq, (new_cost, counter, nb))

    return None


def nearest_cheese(
    maze: Maze,
    position: tuple[int, int],
    cheese_set: set[tuple[int, int]],
) -> tuple[tuple[int, int], list[int], int] | None:
    """Single-source Dijkstra, stops at first cheese hit.

    Returns ``(cheese_pos, directions, cost)`` or None.
    """
    if not cheese_set:
        return None
    if position in cheese_set:
        return (position, [], 0)

    counter = 0
    pq: list[tuple[int, int, tuple[int, int]]] = [(0, counter, position)]
    best: dict[tuple[int, int], int] = {position: 0}
    prev: dict[tuple[int, int], tuple[int, int] | None] = {position: None}

    while pq:
        cost, _, pos = heapq.heappop(pq)
        if cost > best.get(pos, float("inf")):  # type: ignore[arg-type]
            continue
        if pos in cheese_set:
            # Reconstruct.
            path_dirs: list[int] = []
            cur = pos
            while prev[cur] is not None:
                p = prev[cur]
                assert p is not None
                path_dirs.append(_direction_from_to(p, cur))
                cur = p
            path_dirs.reverse()
            return (pos, path_dirs, cost)

        for nb, weight in maze.get_neighbors(pos).items():
            new_cost = cost + weight
            if new_cost < best.get(nb, float("inf")):  # type: ignore[arg-type]
                best[nb] = new_cost
                prev[nb] = pos
                counter += 1
                heapq.heappush(pq, (new_cost, counter, nb))

    return None


def distances_from(
    maze: Maze, position: tuple[int, int]
) -> dict[tuple[int, int], int]:
    """Full single-source Dijkstra. Returns ``{pos: cost}`` for all reachable cells."""
    counter = 0
    pq: list[tuple[int, int, tuple[int, int]]] = [(0, counter, position)]
    best: dict[tuple[int, int], int] = {position: 0}

    while pq:
        cost, _, pos = heapq.heappop(pq)
        if cost > best.get(pos, float("inf")):  # type: ignore[arg-type]
            continue
        for nb, weight in maze.get_neighbors(pos).items():
            new_cost = cost + weight
            if new_cost < best.get(nb, float("inf")):  # type: ignore[arg-type]
                best[nb] = new_cost
                counter += 1
                heapq.heappush(pq, (new_cost, counter, nb))

    return best
