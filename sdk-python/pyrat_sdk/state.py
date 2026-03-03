"""GameState — the single rich object passed to ``think()``."""

from __future__ import annotations

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


class Player(IntEnum):
    PLAYER1 = 0
    PLAYER2 = 1


class PathResult(NamedTuple):
    """Result of a shortest-path query."""

    directions: list[Direction]
    cost: int


class NearestCheeseResult(NamedTuple):
    """Result of a nearest-cheese query."""

    position: tuple[int, int]
    directions: list[Direction]
    cost: int


class GameState:
    """Combines static match config, per-turn snapshot, and convenience methods.

    Built once from MatchConfig (maze, movement_matrix are computed once).
    Updated each turn from TurnState (positions, scores, cheese, etc.).
    """

    # ── Built from MatchConfig (once) ──────────────────

    width: int
    height: int
    max_turns: int
    _maze: PyMaze
    movement_matrix: np.ndarray
    move_timeout_ms: int
    preprocessing_timeout_ms: int

    # Which players this bot controls (usually [0] or [1]).
    controlled_players: list[int]
    # True when this bot is Player1, False when Player2.
    _is_player1: bool

    # ── Updated from TurnState (each turn) ─────────────

    turn: int
    cheese: list[tuple[int, int]]
    cheese_matrix: np.ndarray

    _player1_pos: tuple[int, int]
    _player2_pos: tuple[int, int]
    _player1_score: float
    _player2_score: float
    _player1_mud_turns: int
    _player2_mud_turns: int
    _player1_last_move: int
    _player2_last_move: int

    def __init__(self, config: dict[str, Any]) -> None:
        self.width = config["width"]
        self.height = config["height"]
        self.max_turns = config["max_turns"]
        self.move_timeout_ms = config["move_timeout_ms"]
        self.preprocessing_timeout_ms = config["preprocessing_timeout_ms"]
        self.controlled_players = config["controlled_players"]
        self._is_player1 = 0 in self.controlled_players

        self._maze = PyMaze(
            self.width,
            self.height,
            config["walls"],
            config["mud"],
        )
        self.movement_matrix = self._maze.build_movement_matrix()

        # Initial cheese from config.
        self.cheese = config["cheese"]
        self.cheese_matrix = _build_cheese_matrix(self.cheese, self.width, self.height)

        # Initial positions.
        self._player1_pos = config["player1_start"]
        self._player2_pos = config["player2_start"]
        self._player1_score = 0.0
        self._player2_score = 0.0
        self._player1_mud_turns = 0
        self._player2_mud_turns = 0
        self._player1_last_move = 4  # STAY
        self._player2_last_move = 4
        self.turn = 0

    def update(self, ts: dict[str, Any]) -> None:
        """Apply a TurnState dict (from ``codec.extract_turn_state``)."""
        self.turn = ts["turn"]
        self._player1_pos = ts["player1_position"]
        self._player2_pos = ts["player2_position"]
        self._player1_score = ts["player1_score"]
        self._player2_score = ts["player2_score"]
        self._player1_mud_turns = ts["player1_mud_turns"]
        self._player2_mud_turns = ts["player2_mud_turns"]
        self._player1_last_move = ts["player1_last_move"]
        self._player2_last_move = ts["player2_last_move"]
        self.cheese = ts["cheese"]
        self.cheese_matrix = _build_cheese_matrix(self.cheese, self.width, self.height)

    # ── My / opponent perspective ──────────────────────

    @property
    def my_position(self) -> tuple[int, int]:
        """(x, y) position of this bot."""
        return self._player1_pos if self._is_player1 else self._player2_pos

    @property
    def opponent_position(self) -> tuple[int, int]:
        """(x, y) position of the opponent."""
        return self._player2_pos if self._is_player1 else self._player1_pos

    @property
    def my_score(self) -> float:
        """Current score. 1.0 per cheese, 0.5 if both collect simultaneously."""
        return self._player1_score if self._is_player1 else self._player2_score

    @property
    def opponent_score(self) -> float:
        """Opponent's current score."""
        return self._player2_score if self._is_player1 else self._player1_score

    @property
    def my_mud_turns(self) -> int:
        """Turns remaining stuck in mud. 0 means free to move."""
        return self._player1_mud_turns if self._is_player1 else self._player2_mud_turns

    @property
    def opponent_mud_turns(self) -> int:
        """Opponent's remaining mud turns."""
        return self._player2_mud_turns if self._is_player1 else self._player1_mud_turns

    @property
    def my_last_move(self) -> Direction:
        """This bot's last move as a Direction."""
        raw = self._player1_last_move if self._is_player1 else self._player2_last_move
        return Direction(raw)

    @property
    def opponent_last_move(self) -> Direction:
        """Opponent's last move as a Direction."""
        raw = self._player2_last_move if self._is_player1 else self._player1_last_move
        return Direction(raw)

    @property
    def my_player(self) -> Player:
        """The Player enum value (PLAYER1 or PLAYER2) for this bot."""
        return Player.PLAYER1 if self._is_player1 else Player.PLAYER2

    # ── Raw player data (for HivemindBot) ─────────────

    @property
    def player1_position(self) -> tuple[int, int]:
        """Player 1's (x, y) position."""
        return self._player1_pos

    @property
    def player2_position(self) -> tuple[int, int]:
        """Player 2's (x, y) position."""
        return self._player2_pos

    @property
    def player1_score(self) -> float:
        """Player 1's current score."""
        return self._player1_score

    @property
    def player2_score(self) -> float:
        """Player 2's current score."""
        return self._player2_score

    @property
    def player1_mud_turns(self) -> int:
        """Player 1's remaining mud turns."""
        return self._player1_mud_turns

    @property
    def player2_mud_turns(self) -> int:
        """Player 2's remaining mud turns."""
        return self._player2_mud_turns

    @property
    def player1_last_move(self) -> Direction:
        """Player 1's last move as a Direction."""
        return Direction(self._player1_last_move)

    @property
    def player2_last_move(self) -> Direction:
        """Player 2's last move as a Direction."""
        return Direction(self._player2_last_move)

    # ── Layer 4: simulation ───────────────────────────

    def simulate(self) -> GameSim:
        """Mutable game snapshot for make_move / unmake_move tree search.

        Returns a Rust-backed ``GameSim`` with the current maze topology
        and game state. Uses objective player1/player2 naming — no
        my/opponent mapping.
        """
        return self._maze.simulate(
            self._player1_pos,
            self._player2_pos,
            self._player1_score,
            self._player2_score,
            self._player1_mud_turns,
            self._player2_mud_turns,
            self.cheese,
            self.turn,
        )

    # ── Layer 2 convenience ────────────────────────────

    def get_effective_moves(
        self, pos: tuple[int, int] | None = None
    ) -> list[Direction]:
        """Directions that don't hit a wall from *pos* (default: my position).

        Returns a list of Direction values (UP, RIGHT, DOWN, LEFT).
        Does not include STAY.
        """
        if pos is None:
            pos = self.my_position
        x, y = pos
        return [Direction(d) for d in self._maze.valid_moves(x, y)]

    def get_move_cost(
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

        Returns a PathResult(directions, cost) where directions is the
        full Direction sequence and cost is in turns (mud passages cost
        more than 1). Returns None if unreachable.
        """
        result = self._maze.shortest_path(start, goal)
        if result is None:
            return None
        dirs, cost = result
        return PathResult([Direction(d) for d in dirs], cost)

    def nearest_cheese(
        self, pos: tuple[int, int] | None = None
    ) -> NearestCheeseResult | None:
        """Nearest cheese from *pos* (default: my position).

        Returns a NearestCheeseResult(position, directions, cost) where
        position is the cheese (x, y), directions is the full path, and
        cost is in turns. Returns None if no cheese remains.
        """
        if pos is None:
            pos = self.my_position
        result = self._maze.nearest_cheese(pos, self.cheese)
        if result is None:
            return None
        target, dirs, cost = result
        return NearestCheeseResult(target, [Direction(d) for d in dirs], cost)

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
