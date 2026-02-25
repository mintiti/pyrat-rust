"""Weighted adjacency graph built from the match config.

Edge weight = number of turns to traverse:
  1 = free passage, N > 1 = mud.
"""

from __future__ import annotations


class Maze:
    """Weighted adjacency graph for a PyRat maze.

    Built once from the MatchConfig's wall and mud lists.  The grid has
    implicit edges between all cardinal neighbours; walls remove them;
    mud sets weight > 1.
    """

    def __init__(
        self,
        width: int,
        height: int,
        walls: list[tuple[tuple[int, int], tuple[int, int]]],
        mud: list[tuple[tuple[int, int], tuple[int, int], int]],
    ) -> None:
        self.width = width
        self.height = height

        # {pos: {neighbor: weight}}
        self._adj: dict[tuple[int, int], dict[tuple[int, int], int]] = {}

        # Start with all cardinal edges, weight 1.
        for x in range(width):
            for y in range(height):
                neighbors: dict[tuple[int, int], int] = {}
                if y + 1 < height:
                    neighbors[(x, y + 1)] = 1
                if y - 1 >= 0:
                    neighbors[(x, y - 1)] = 1
                if x + 1 < width:
                    neighbors[(x + 1, y)] = 1
                if x - 1 >= 0:
                    neighbors[(x - 1, y)] = 1
                self._adj[(x, y)] = neighbors

        # Remove walls (bidirectional).
        for (x1, y1), (x2, y2) in walls:
            self._adj.get((x1, y1), {}).pop((x2, y2), None)
            self._adj.get((x2, y2), {}).pop((x1, y1), None)

        # Set mud weights (bidirectional).
        for (x1, y1), (x2, y2), value in mud:
            if (x2, y2) in self._adj.get((x1, y1), {}):
                self._adj[(x1, y1)][(x2, y2)] = value
            if (x1, y1) in self._adj.get((x2, y2), {}):
                self._adj[(x2, y2)][(x1, y1)] = value

    def get_neighbors(self, pos: tuple[int, int]) -> dict[tuple[int, int], int]:
        """Return ``{neighbor: weight}`` for all reachable neighbors of *pos*."""
        return self._adj.get(pos, {})

    def get_weight(self, a: tuple[int, int], b: tuple[int, int]) -> int:
        """Edge weight from *a* to *b*, or -1 if there's a wall."""
        return self._adj.get(a, {}).get(b, -1)

    def has_edge(self, a: tuple[int, int], b: tuple[int, int]) -> bool:
        return b in self._adj.get(a, {})
