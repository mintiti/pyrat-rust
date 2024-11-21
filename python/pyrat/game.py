"""High-level PyRat game interface.

This module provides the main game interface for PyRat, wrapping the Rust engine
with a Pythonic API. It includes core game types and the main PyRat class.

The game follows these basic rules:
- Two players move simultaneously on a maze-like grid
- Players collect cheese pieces to score points
- Mud spaces can delay player movement
- Game ends when a player collects majority of cheese or max turns reached
"""

from dataclasses import dataclass
from enum import IntEnum
from typing import Dict, List, NamedTuple, Optional, Tuple

from pyrat._rust import PyGameState as _RustGameState
from pyrat._rust import PyMoveUndo as _RustMoveUndo


@dataclass(frozen=True)
class Position:
    """2D position on the game board.

    Represents a position on the game board with integer coordinates.
    The origin (0,0) is at the bottom-left corner of the board.

    Attributes:
        x: Horizontal position (0 to width-1)
        y: Vertical position (0 to height-1)
    """

    x: int
    y: int

    def __hash__(self) -> int:
        return hash((self.x, self.y))

    def __repr__(self) -> str:
        return f"Pos({self.x}, {self.y})"


class Direction(IntEnum):
    """Available movement directions.

    Defines the possible moves a player can make each turn.
    STAY allows a player to remain in their current position.

    Values:
        UP (0): Move up one cell
        RIGHT (1): Move right one cell
        DOWN (2): Move down one cell
        LEFT (3): Move left one cell
        STAY (4): Stay in current position
    """

    UP = 0
    RIGHT = 1
    DOWN = 2
    LEFT = 3
    STAY = 4


@dataclass(frozen=True)
class MoveUndo:
    """Information needed to undo a move in the game.

    This class stores all state information required to reverse a move,
    enabling the game engine to support move undo/redo functionality.
    This is particularly useful for implementing game tree search algorithms
    and analyzing different game strategies.

    Example:
        >>> game = PyRat(width=15, height=15)
        >>> # Make a move and store undo information
        >>> undo_info = game.make_move(Direction.RIGHT, Direction.LEFT)
        >>> # Make another move
        >>> game.make_move(Direction.UP, Direction.DOWN)
        >>> # Undo the last move
        >>> game.unmake_move(undo_info)
    """

    _undo: _RustMoveUndo  # Internal rust undo data

    @property
    def p1_position(self) -> Position:
        """Player 1's position before the move."""
        x, y = self._undo.p1_pos
        return Position(x, y)

    @property
    def p2_position(self) -> Position:
        """Player 2's position before the move."""
        x, y = self._undo.p2_pos
        return Position(x, y)

    @property
    def scores(self) -> Tuple[float, float]:
        """Scores before the move."""
        return (self._undo.p1_score, self._undo.p2_score)

    @property
    def collected_cheese(self) -> List[Position]:
        """Cheese collected during this move."""
        return [Position(x, y) for x, y in self._undo.collected_cheese]

    @property
    def turn(self) -> int:
        """Turn number before the move."""
        return self._undo.turn

    def __repr__(self) -> str:
        return (
            f"MoveUndo(turn={self.turn}, "
            f"p1_pos={self.p1_position}, "
            f"p2_pos={self.p2_position}, "
            f"scores={self.scores})"
        )


class GameResult(NamedTuple):
    """Result of a game step.

    Contains information about the outcome of a single game step.

    Attributes:
        game_over: True if the game has ended
        collected_cheese: List of positions where cheese was collected this turn
        p1_score: Player 1's current score
        p2_score: Player 2's current score
    """

    game_over: bool
    collected_cheese: List[Position]
    p1_score: float
    p2_score: float


class PyRat:
    """High-performance PyRat game implementation.

    This class provides the main interface to the PyRat game engine. It wraps
    the Rust implementation with a Pythonic API while maintaining high performance.

    Args:
        width: Width of the game board (default: 21)
        height: Height of the game board (default: 15)
        cheese_count: Number of cheese pieces to place (default: 41)
        symmetric: If True, generate symmetric mazes (default: True)
        seed: Random seed for reproducible games (default: None)

    Example:
        >>> game = PyRat(width=15, height=15)
        >>> # Make a move
        >>> result = game.step(Direction.RIGHT, Direction.LEFT)
        >>> print(f"Cheese collected: {result.collected_cheese}")
        >>> # Use undo/redo functionality
        >>> undo_info = game.make_move(Direction.UP, Direction.DOWN)
        >>> game.unmake_move(undo_info)  # Restore previous state
    """

    def __init__(
        self,
        width: Optional[int] = None,
        height: Optional[int] = None,
        cheese_count: Optional[int] = None,
        symmetric: bool = True,
        seed: Optional[int] = None,
    ):
        """Initialize a new PyRat game."""
        self._game = _RustGameState(width, height, cheese_count, symmetric, seed)

    @property
    def dimensions(self) -> Tuple[int, int]:
        """Get board dimensions."""
        return self._game.width, self._game.height

    @property
    def turn(self) -> int:
        """Current turn number."""
        return self._game.turn

    @property
    def max_turns(self) -> int:
        """Maximum number of turns."""
        return self._game.max_turns

    @property
    def player1_pos(self) -> Position:
        """Get player 1's position."""
        x, y = self._game.player1_position
        return Position(x, y)

    @property
    def player2_pos(self) -> Position:
        """Get player 2's position."""
        x, y = self._game.player2_position
        return Position(x, y)

    @property
    def scores(self) -> Tuple[float, float]:
        """Get current scores."""
        return self._game.player1_score, self._game.player2_score

    @property
    def cheese_positions(self) -> List[Position]:
        """Get all cheese positions."""
        return [Position(x, y) for x, y in self._game.cheese_positions()]

    @property
    def mud_positions(self) -> Dict[Tuple[Position, Position], int]:
        """Get mud positions and their values."""
        return {
            (Position(x1, y1), Position(x2, y2)): value
            for ((x1, y1), (x2, y2), value) in self._game.mud_entries()
        }

    def step(self, p1_move: Direction, p2_move: Direction) -> GameResult:
        """Execute one game step."""
        game_over, collected = self._game.step(p1_move.value, p2_move.value)
        return GameResult(
            game_over=game_over,
            collected_cheese=[Position(x, y) for x, y in collected],
            p1_score=self._game.player1_score,
            p2_score=self._game.player2_score,
        )

    def reset(self, seed: Optional[int] = None) -> None:
        """Reset the game."""
        self._game.reset(seed)

    def make_move(self, p1_move: Direction, p2_move: Direction) -> MoveUndo:
        """Make a move and return undo information."""
        undo = self._game.make_move(p1_move.value, p2_move.value)
        return MoveUndo(_undo=undo)

    def unmake_move(self, undo: MoveUndo) -> None:
        """Unmake a move using saved undo information."""
        self._game.unmake_move(undo._undo)

    def __repr__(self) -> str:
        return str(self._game)
