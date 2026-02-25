"""Precomputed numpy arrays for fast per-turn lookups.

Layer 2 — sits on top of the Maze graph.
"""

from __future__ import annotations

from typing import TYPE_CHECKING

import numpy as np

if TYPE_CHECKING:
    from pyrat_sdk.maze import Maze

# Direction enum values — must match the FlatBuffers schema.
_UP = 0
_RIGHT = 1
_DOWN = 2
_LEFT = 3

# (dx, dy) for each direction index.
_OFFSETS = [(0, 1), (1, 0), (0, -1), (-1, 0)]


def build_movement_matrix(maze: Maze) -> np.ndarray:
    """Shape ``(width, height, 4)``, dtype int8.

    Index 0-3 = UP / RIGHT / DOWN / LEFT.
    Values: -1 = wall, 0 = free passage, N > 0 = mud cost.
    """
    w, h = maze.width, maze.height
    mat = np.full((w, h, 4), -1, dtype=np.int8)
    for x in range(w):
        for y in range(h):
            for d, (dx, dy) in enumerate(_OFFSETS):
                nx, ny = x + dx, y + dy
                weight = maze.get_weight((x, y), (nx, ny))
                if weight < 0:
                    continue  # wall — stays -1
                # weight 1 = free (encode as 0), weight > 1 = mud cost
                mat[x, y, d] = 0 if weight == 1 else weight
    return mat


def build_cheese_matrix(
    cheese: list[tuple[int, int]], width: int, height: int
) -> np.ndarray:
    """Shape ``(width, height)``, dtype uint8.  1 where cheese exists."""
    mat = np.zeros((width, height), dtype=np.uint8)
    for x, y in cheese:
        mat[x, y] = 1
    return mat


def get_effective_moves(movement_matrix: np.ndarray, pos: tuple[int, int]) -> list[int]:
    """Directions (0-3) that don't hit a wall from *pos*."""
    x, y = pos
    return [d for d in range(4) if movement_matrix[x, y, d] >= 0]


def get_move_cost(movement_matrix: np.ndarray, pos: tuple[int, int], direction: int) -> int:
    """Return -1 (wall), 0 (free), or N (mud cost) for *direction* from *pos*."""
    x, y = pos
    return int(movement_matrix[x, y, direction])
