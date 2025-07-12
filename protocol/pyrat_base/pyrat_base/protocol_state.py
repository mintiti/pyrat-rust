"""Protocol-oriented wrapper for PyGameState.

This module provides a thin wrapper around PyGameState that adds player identity
awareness, allowing AI developers to access game state from their perspective
(my_position vs opponent_position) while delegating all game logic to the
underlying Rust implementation.
"""
# ruff: noqa: F821

from typing import List, Optional, Tuple

from pyrat_engine._rust import PyGameState
from pyrat_engine.game import Direction

from pyrat_base.enums import Player


class ProtocolState:
    """Ultra-thin wrapper providing protocol-oriented view of game state.

    This class adds player identity awareness to PyGameState, providing
    convenient my/opponent properties while delegating everything else
    to the underlying Rust implementation.

    The wrapper uses observation caching to minimize calls to the Rust layer
    when accessing multiple player-perspective properties.

    Args:
        game_state: The underlying PyGameState from the Rust engine
        i_am: Which player this AI is (RAT or PYTHON)

    Example:
        >>> from pyrat_engine import PyGameState
        >>> from pyrat_base.enums import Player
        >>> game = PyGameState(width=15, height=15)
        >>> state = ProtocolState(game, Player.RAT)
        >>> print(f"My position: {state.my_position}")
        >>> print(f"Opponent position: {state.opponent_position}")
    """

    def __init__(self, game_state: PyGameState, i_am: Player):
        """Initialize the protocol state wrapper.

        Args:
            game_state: The underlying PyGameState instance
            i_am: The player identity (RAT or PYTHON)
        """
        self._game = game_state
        self.i_am = i_am
        self._observation = None  # Cache for current observation

    # Direct passthrough properties (zero overhead)
    @property
    def width(self) -> int:
        """Width of the game board."""
        return self._game.width

    @property
    def height(self) -> int:
        """Height of the game board."""
        return self._game.height

    @property
    def turn(self) -> int:
        """Current turn number (starts at 0)."""
        return self._game.turn

    @property
    def max_turns(self) -> int:
        """Maximum number of turns before game truncation."""
        return self._game.max_turns

    @property
    def cheese(self) -> List[Tuple[int, int]]:
        """List of remaining cheese positions as (x, y) tuples."""
        return self._game.cheese_positions()

    @property
    def mud(self) -> List[Tuple[Tuple[int, int], Tuple[int, int], int]]:
        """List of mud entries as ((x1, y1), (x2, y2), mud_value) tuples."""
        return self._game.mud_entries()

    # Protocol-oriented properties using cached observation
    def _get_observation(self) -> "PyGameObservation":  # type: ignore[name-defined]
        """Get observation from my perspective (with caching).

        This method caches the observation to avoid repeated calls to the
        Rust layer when accessing multiple observation-based properties.
        """
        if self._observation is None:
            is_player_one = self.i_am == Player.RAT
            self._observation = self._game.get_observation(is_player_one)
        return self._observation

    @property
    def my_position(self) -> Tuple[int, int]:
        """My current position as (x, y) tuple."""
        return self._get_observation().player_position

    @property
    def opponent_position(self) -> Tuple[int, int]:
        """Opponent's current position as (x, y) tuple."""
        return self._get_observation().opponent_position

    @property
    def my_score(self) -> float:
        """My current score (cheese collected)."""
        return self._get_observation().player_score

    @property
    def opponent_score(self) -> float:
        """Opponent's current score (cheese collected)."""
        return self._get_observation().opponent_score

    @property
    def my_mud_turns(self) -> int:
        """Number of turns I'm still stuck in mud (0 if not in mud)."""
        return self._get_observation().player_mud_turns

    @property
    def opponent_mud_turns(self) -> int:
        """Number of turns opponent is still stuck in mud (0 if not in mud)."""
        return self._get_observation().opponent_mud_turns

    # Additional helpful properties
    @property
    def cheese_matrix(self) -> "np.ndarray":  # type: ignore[name-defined]
        """2D numpy array where 1 indicates cheese presence, 0 indicates absence.

        Shape: (width, height)
        """
        return self._get_observation().cheese_matrix

    @property
    def movement_matrix(self) -> "np.ndarray":  # type: ignore[name-defined]
        """3D numpy array encoding valid moves and mud costs.

        Shape: (width, height, 4) where the last dimension corresponds to
        [UP, RIGHT, DOWN, LEFT] with values:
        - -1: Invalid move (wall or out of bounds)
        - 0: Valid immediate move
        - N>0: Valid move with N turns of mud delay
        """
        return self._get_observation().movement_matrix

    def invalidate_cache(self) -> None:
        """Invalidate cached observation after state change.

        This should be called after any operation that modifies the game state
        to ensure subsequent property accesses get fresh data.
        """
        self._observation = None

    # Convenience methods
    def get_valid_moves(self) -> List[Direction]:
        """Get list of valid moves from current position.

        Returns:
            List of Direction enums representing valid moves from current position.
            Always includes Direction.STAY as it's always valid.
        """
        valid_moves = [Direction.STAY]  # STAY is always valid

        x, y = self.my_position
        movement_costs = self.movement_matrix[x, y]

        # Check each direction (UP, RIGHT, DOWN, LEFT)
        directions = [Direction.UP, Direction.RIGHT, Direction.DOWN, Direction.LEFT]
        for i, direction in enumerate(directions):
            if movement_costs[i] >= 0:  # -1 means invalid (wall/boundary)
                valid_moves.append(direction)

        return valid_moves

    def get_move_cost(self, direction: Direction) -> Optional[int]:
        """Get the mud cost for moving in a given direction.

        Args:
            direction: The direction to check

        Returns:
            The mud cost (0 for immediate move, >0 for mud delay),
            or None if the move is invalid.
        """
        if direction == Direction.STAY:
            return 0

        x, y = self.my_position
        movement_costs = self.movement_matrix[x, y]

        # Map direction to index in movement matrix
        direction_indices = {
            Direction.UP: 0,
            Direction.RIGHT: 1,
            Direction.DOWN: 2,
            Direction.LEFT: 3,
        }

        if direction in direction_indices:
            cost = movement_costs[direction_indices[direction]]
            return cost if cost >= 0 else None

        return None

    def __repr__(self) -> str:
        """String representation of the protocol state."""
        return (
            f"ProtocolState(turn={self.turn}/{self.max_turns}, "
            f"i_am={self.i_am.value}, "
            f"my_pos={self.my_position}, "
            f"my_score={self.my_score}, "
            f"opponent_pos={self.opponent_position}, "
            f"opponent_score={self.opponent_score})"
        )
