"""GameState — the single rich object passed to ``think()``.

Holds an engine-backed ``GameSim`` mirror that tracks the canonical Zobrist
hash. Each ``apply_advance`` invokes the engine's ``process_turn`` so the
state hash stays in sync turn-by-turn. ``load_turn_state`` /
``load_full_state`` rebuild from a snapshot (received via GoState or
FullState) and recompute the hash from scratch.
"""

from __future__ import annotations

import copy
from enum import IntEnum
from typing import Any, NamedTuple

import numpy as np

from pyrat_sdk._engine import GameSim, PyMaze


class Direction(IntEnum):
    UP = 0
    RIGHT = 1
    DOWN = 2
    LEFT = 3
    STAY = 4

    def apply_to(self, pos: tuple[int, int]) -> tuple[int, int]:
        """Return the neighbouring cell after moving in this direction."""
        dx, dy = _DIR_DELTAS[self.value]
        return (pos[0] + dx, pos[1] + dy)


_DIR_DELTAS = {0: (0, 1), 1: (1, 0), 2: (0, -1), 3: (-1, 0), 4: (0, 0)}


class Player(IntEnum):
    PLAYER1 = 0
    PLAYER2 = 1


class PathResult(NamedTuple):
    """Result of a pathfinding query.

    Matches Rust SDK's ``FullPathResult`` — same four fields.
    """

    target: tuple[int, int]
    path: list[Direction]
    first_moves: list[Direction]
    cost: int


class GameState:
    """SDK-facing game state.

    Built once from MatchConfig + the slot assigned by ``Welcome``. The
    canonical state advances via ``apply_advance`` (one engine step,
    incremental Zobrist) or rebuilds via ``load_turn_state`` /
    ``load_full_state`` (full snapshot, recompute Zobrist).
    """

    # Static config
    width: int
    height: int
    max_turns: int
    move_timeout_ms: int
    preprocessing_timeout_ms: int

    # Maze topology and the engine mirror
    _maze: PyMaze
    _sim: GameSim
    movement_matrix: np.ndarray

    # Cached derived data
    cheese_matrix: np.ndarray

    # Slot assigned by Welcome
    _is_player1: bool

    # Last moves (engine doesn't track them)
    _player1_last_move: int
    _player2_last_move: int

    def __init__(self, slot: int, config: dict[str, Any]) -> None:
        self._is_player1 = slot == 0
        self._load_config(config)

        # Initial sim from config: starts, zero scores, no mud, turn 0.
        self._sim = self._maze.to_sim(
            config["player1_start"],
            config["player2_start"],
            0.0,
            0.0,
            0,
            0,
            config["cheese"],
            0,
        )
        self._player1_last_move = int(Direction.STAY)
        self._player2_last_move = int(Direction.STAY)
        self.cheese_matrix = _build_cheese_matrix(
            config["cheese"], self.width, self.height
        )

    # ── Mutation paths ────────────────────────────────

    def apply_advance(self, p1: Direction | int, p2: Direction | int) -> int:
        """Apply both players' moves and return the new state hash.

        Drives the engine via ``make_move`` so Zobrist stays consistent
        without a full recompute.
        """
        p1_int, p2_int = int(p1), int(p2)
        self._sim.make_move(p1_int, p2_int)
        self._player1_last_move = p1_int
        self._player2_last_move = p2_int
        self.cheese_matrix = _build_cheese_matrix(
            self._sim.cheese_positions, self.width, self.height
        )
        return self._sim.state_hash

    def load_turn_state(self, ts: dict[str, Any]) -> int:
        """Replace the sim with a snapshot from the given TurnState dict."""
        self._sim = self._maze.to_sim(
            ts["player1_position"],
            ts["player2_position"],
            ts["player1_score"],
            ts["player2_score"],
            ts["player1_mud_turns"],
            ts["player2_mud_turns"],
            ts["cheese"],
            ts["turn"],
        )
        self._player1_last_move = ts["player1_last_move"]
        self._player2_last_move = ts["player2_last_move"]
        self.cheese_matrix = _build_cheese_matrix(
            ts["cheese"], self.width, self.height
        )
        return self._sim.state_hash

    def load_full_state(
        self, config: dict[str, Any], ts: dict[str, Any]
    ) -> int:
        """Rebuild from a fresh MatchConfig + TurnState (FullState recovery)."""
        self._load_config(config)
        return self.load_turn_state(ts)

    def _load_config(self, config: dict[str, Any]) -> None:
        self.width = config["width"]
        self.height = config["height"]
        self.max_turns = config["max_turns"]
        self.move_timeout_ms = config["move_timeout_ms"]
        self.preprocessing_timeout_ms = config["preprocessing_timeout_ms"]
        self._maze = PyMaze(
            self.width,
            self.height,
            config["walls"],
            config["mud"],
        )
        self.movement_matrix = self._maze.build_movement_matrix()

    # ── Sim-backed accessors ──────────────────────────

    @property
    def turn(self) -> int:
        """Current turn number."""
        return self._sim.turn

    @property
    def state_hash(self) -> int:
        """Canonical engine Zobrist hash for the current position."""
        return self._sim.state_hash

    @property
    def cheese(self) -> list[tuple[int, int]]:
        """Cheese positions remaining on the board."""
        return self._sim.cheese_positions

    # ── My / opponent perspective ──────────────────────

    @property
    def my_position(self) -> tuple[int, int]:
        return self._sim.player1_position if self._is_player1 else self._sim.player2_position

    @property
    def opponent_position(self) -> tuple[int, int]:
        return self._sim.player2_position if self._is_player1 else self._sim.player1_position

    @property
    def my_score(self) -> float:
        return self._sim.player1_score if self._is_player1 else self._sim.player2_score

    @property
    def opponent_score(self) -> float:
        return self._sim.player2_score if self._is_player1 else self._sim.player1_score

    @property
    def my_mud_turns(self) -> int:
        return (
            self._sim.player1_mud_turns
            if self._is_player1
            else self._sim.player2_mud_turns
        )

    @property
    def opponent_mud_turns(self) -> int:
        return (
            self._sim.player2_mud_turns
            if self._is_player1
            else self._sim.player1_mud_turns
        )

    @property
    def my_last_move(self) -> Direction:
        raw = self._player1_last_move if self._is_player1 else self._player2_last_move
        return Direction(raw)

    @property
    def opponent_last_move(self) -> Direction:
        raw = self._player2_last_move if self._is_player1 else self._player1_last_move
        return Direction(raw)

    @property
    def my_player(self) -> Player:
        return Player.PLAYER1 if self._is_player1 else Player.PLAYER2

    # ── Raw player data (for HivemindBot) ─────────────

    @property
    def player1_position(self) -> tuple[int, int]:
        return self._sim.player1_position

    @property
    def player2_position(self) -> tuple[int, int]:
        return self._sim.player2_position

    @property
    def player1_score(self) -> float:
        return self._sim.player1_score

    @property
    def player2_score(self) -> float:
        return self._sim.player2_score

    @property
    def player1_mud_turns(self) -> int:
        return self._sim.player1_mud_turns

    @property
    def player2_mud_turns(self) -> int:
        return self._sim.player2_mud_turns

    @property
    def player1_last_move(self) -> Direction:
        return Direction(self._player1_last_move)

    @property
    def player2_last_move(self) -> Direction:
        return Direction(self._player2_last_move)

    # ── Layer 4: simulation ───────────────────────────

    def to_sim(self) -> GameSim:
        """Independent mutable copy of the engine state.

        Returns a clone for tree-search use (``make_move`` / ``unmake_move``)
        that doesn't disturb the SDK's internal mirror.
        """
        return copy.copy(self._sim)

    # ── Layer 2 convenience ────────────────────────────

    def effective_moves(self, pos: tuple[int, int] | None = None) -> list[Direction]:
        """Directions that don't hit a wall from *pos* (default: my position).

        Returns a list of Direction values (UP, RIGHT, DOWN, LEFT).
        Does not include STAY.
        """
        if pos is None:
            pos = self.my_position
        x, y = pos
        return [Direction(d) for d in self._maze.effective_moves(x, y)]

    def move_cost(
        self, direction: Direction | int, pos: tuple[int, int] | None = None
    ) -> int | None:
        """Cost of moving in *direction* from *pos*.

        Returns None if there's a wall, 1 for a free passage (1 turn),
        or N for a mud passage (N turns to traverse).
        """
        if pos is None:
            pos = self.my_position
        x, y = pos
        result: int | None = self._maze.move_cost(x, y, direction)
        return result

    # ── Layer 3 convenience ────────────────────────────

    def shortest_path(
        self, start: tuple[int, int], goal: tuple[int, int]
    ) -> PathResult | None:
        """Shortest path between two cells.

        Returns a ``PathResult(target, path, first_moves, cost)`` where
        *path* is the full Direction sequence and *first_moves* lists
        every direction that starts an optimal route (there may be ties).
        Returns None if unreachable.
        """
        result = self._maze.shortest_path(start, goal)
        if result is None:
            return None
        target, path, first_moves, cost = result
        return PathResult(
            target,
            [Direction(d) for d in path],
            [Direction(d) for d in first_moves],
            cost,
        )

    def nearest_cheese(self, pos: tuple[int, int] | None = None) -> PathResult | None:
        """Nearest cheese from *pos* (default: my position).

        Returns a ``PathResult(target, path, first_moves, cost)``.
        Returns None if no cheese remains.

        When multiple cheeses tie at the minimum distance, returns the first
        one in the cheese list. Use ``nearest_cheeses`` to get all tied results.
        """
        if pos is None:
            pos = self.my_position
        result = self._maze.nearest_cheese(pos, self.cheese)
        if result is None:
            return None
        target, path, first_moves, cost = result
        return PathResult(
            target,
            [Direction(d) for d in path],
            [Direction(d) for d in first_moves],
            cost,
        )

    def nearest_cheeses(self, pos: tuple[int, int] | None = None) -> list[PathResult]:
        """All cheeses tied at the minimum distance from *pos* (default: my position).

        Each result has the full path reconstructed. Returns an empty list
        if no cheese remains.
        """
        if pos is None:
            pos = self.my_position
        results = self._maze.nearest_cheeses(pos, self.cheese)
        return [
            PathResult(
                target,
                [Direction(d) for d in path],
                [Direction(d) for d in first_moves],
                cost,
            )
            for target, path, first_moves, cost in results
        ]

    def distances_from(
        self, pos: tuple[int, int] | None = None
    ) -> dict[tuple[int, int], int]:
        """Weighted distances from *pos* (default: my position) to all reachable cells.

        Returns a dict of {(x, y): cost} where cost matches shortest_path costs
        (i.e. turns, accounting for mud weights).
        """
        if pos is None:
            pos = self.my_position
        result: dict[tuple[int, int], int] = self._maze.distances_from(pos)
        return result


def _build_cheese_matrix(
    cheese: list[tuple[int, int]], width: int, height: int
) -> np.ndarray:
    """Shape ``(width, height)``, dtype uint8.  1 where cheese exists."""
    mat = np.zeros((width, height), dtype=np.uint8)
    for x, y in cheese:
        mat[x, y] = 1
    return mat
