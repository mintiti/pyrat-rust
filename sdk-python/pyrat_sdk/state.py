"""GameState — the single rich object passed to ``think()``."""

from __future__ import annotations

import numpy as np

from pyrat_sdk._engine import PyMaze


class GameState:
    """Combines static match config, per-turn snapshot, and convenience methods.

    Built once from MatchConfig (maze, movement_matrix are computed once).
    Updated each turn from TurnState (positions, scores, cheese, etc.).
    """

    # ── Built from MatchConfig (once) ──────────────────

    width: int
    height: int
    max_turns: int
    maze: PyMaze
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

    def __init__(self, config: dict) -> None:
        self.width = config["width"]
        self.height = config["height"]
        self.max_turns = config["max_turns"]
        self.move_timeout_ms = config["move_timeout_ms"]
        self.preprocessing_timeout_ms = config["preprocessing_timeout_ms"]
        self.controlled_players = config["controlled_players"]
        self._is_player1 = 0 in self.controlled_players

        self.maze = PyMaze(
            self.width,
            self.height,
            config["walls"],
            config["mud"],
        )
        self.movement_matrix = self.maze.build_movement_matrix()

        # Initial cheese from config.
        self.cheese = config["cheese"]
        self.cheese_matrix = _build_cheese_matrix(
            self.cheese, self.width, self.height
        )

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

    def update(self, ts: dict) -> None:
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
        self.cheese_matrix = _build_cheese_matrix(
            self.cheese, self.width, self.height
        )

    # ── My / opponent perspective ──────────────────────

    @property
    def my_position(self) -> tuple[int, int]:
        return self._player1_pos if self._is_player1 else self._player2_pos

    @property
    def opponent_position(self) -> tuple[int, int]:
        return self._player2_pos if self._is_player1 else self._player1_pos

    @property
    def my_score(self) -> float:
        return self._player1_score if self._is_player1 else self._player2_score

    @property
    def opponent_score(self) -> float:
        return self._player2_score if self._is_player1 else self._player1_score

    @property
    def my_mud_turns(self) -> int:
        return self._player1_mud_turns if self._is_player1 else self._player2_mud_turns

    @property
    def opponent_mud_turns(self) -> int:
        return self._player2_mud_turns if self._is_player1 else self._player1_mud_turns

    @property
    def my_last_move(self) -> int:
        return self._player1_last_move if self._is_player1 else self._player2_last_move

    @property
    def opponent_last_move(self) -> int:
        return self._player2_last_move if self._is_player1 else self._player1_last_move

    @property
    def my_player(self) -> int:
        """The Player enum int (0 or 1) for this bot."""
        return 0 if self._is_player1 else 1

    # ── Layer 2 convenience ────────────────────────────

    def get_effective_moves(self, pos: tuple[int, int] | None = None) -> list[int]:
        """Directions (0-3) that don't hit a wall from *pos* (default: my position)."""
        x, y = pos or self.my_position
        return self.maze.valid_moves(x, y)

    def get_move_cost(self, direction: int, pos: tuple[int, int] | None = None) -> int:
        """Return -1 (wall), 0 (free), or N (mud) for *direction* from *pos*."""
        x, y = pos or self.my_position
        cost = self.maze.move_cost(x, y, direction)
        if cost is None:
            return -1
        return 0 if cost == 1 else cost

    # ── Layer 3 convenience ────────────────────────────

    def shortest_path(
        self, start: tuple[int, int], goal: tuple[int, int]
    ) -> tuple[list[int], int] | None:
        return self.maze.shortest_path(start, goal)

    def nearest_cheese(
        self, pos: tuple[int, int] | None = None
    ) -> tuple[tuple[int, int], list[int], int] | None:
        return self.maze.nearest_cheese(
            pos or self.my_position, self.cheese
        )

    def distances_from(
        self, pos: tuple[int, int] | None = None
    ) -> dict[tuple[int, int], int]:
        return self.maze.distances_from(pos or self.my_position)


def _build_cheese_matrix(
    cheese: list[tuple[int, int]], width: int, height: int
) -> np.ndarray:
    """Shape ``(width, height)``, dtype uint8.  1 where cheese exists."""
    mat = np.zeros((width, height), dtype=np.uint8)
    for x, y in cheese:
        mat[x, y] = 1
    return mat
