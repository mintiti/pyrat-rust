"""Core game state and management classes.

This module contains the main game state management:
- PyRat: The core game engine
- MoveUndo: Undo information for game tree search
"""

from pyrat_engine.core.observation import GameObservation
from pyrat_engine.core.types import Coordinates, Mud, Wall

class MoveUndo:
    """Information needed to undo a move in the game.

    This class contains all state information required to reverse a move,
    including player positions, scores, and collected cheese.
    """
    @property
    def p1_pos(self) -> Coordinates:
        """Player 1's position before the move."""
        ...

    @property
    def p2_pos(self) -> Coordinates:
        """Player 2's position before the move."""
        ...

    @property
    def p1_target(self) -> Coordinates:
        """Position player 1 was attempting to move to."""
        ...

    @property
    def p2_target(self) -> Coordinates:
        """Position player 2 was attempting to move to."""
        ...

    @property
    def p1_mud(self) -> int:
        """Number of mud turns remaining for player 1."""
        ...

    @property
    def p2_mud(self) -> int:
        """Number of mud turns remaining for player 2."""
        ...

    @property
    def p1_score(self) -> float:
        """Player 1's score before the move."""
        ...

    @property
    def p2_score(self) -> float:
        """Player 2's score before the move."""
        ...

    @property
    def p1_misses(self) -> int:
        """Number of failed moves for player 1."""
        ...

    @property
    def p2_misses(self) -> int:
        """Number of failed moves for player 2."""
        ...

    @property
    def collected_cheese(self) -> list[Coordinates]:
        """List of positions where cheese was collected during this move."""
        ...

    @property
    def turn(self) -> int:
        """Turn number before the move was made."""
        ...

class PyRat:
    """Core PyRat game engine implementation in Rust.

    This class manages all game state including player positions, scores,
    cheese placement, mud effects, and turn counting.

    PyRat instances are created via GameConfig.create() or GameBuilder.
    """
    @property
    def width(self) -> int:
        """Width of the game board."""
        ...

    @property
    def height(self) -> int:
        """Height of the game board."""
        ...

    @property
    def turn(self) -> int:
        """Current turn number (starts at 0)."""
        ...

    @property
    def max_turns(self) -> int:
        """Maximum number of turns before the game ends."""
        ...

    @property
    def player1_position(self) -> Coordinates:
        """Current position of player 1."""
        ...

    @property
    def player2_position(self) -> Coordinates:
        """Current position of player 2."""
        ...

    @property
    def player1_score(self) -> float:
        """Current score of player 1."""
        ...

    @property
    def player2_score(self) -> float:
        """Current score of player 2."""
        ...

    @property
    def player1_mud_turns(self) -> int:
        """Number of turns player 1 remains stuck in mud (0 if not in mud)."""
        ...

    @property
    def player2_mud_turns(self) -> int:
        """Number of turns player 2 remains stuck in mud (0 if not in mud)."""
        ...

    def cheese_positions(self) -> list[Coordinates]:
        """Get positions of all remaining cheese pieces.

        Returns:
            List of Coordinates where cheese pieces are located
        """
        ...

    def mud_entries(self) -> list[Mud]:
        """Get all mud patches in the maze.

        Returns:
            List of Mud objects, each with pos1, pos2, and value attributes
        """
        ...

    def wall_entries(self) -> list[Wall]:
        """Get all walls in the maze.

        Returns:
            List of Wall objects, each with pos1 and pos2 attributes
        """
        ...

    def get_valid_moves(self, pos: Coordinates | tuple[int, int]) -> list[int]:
        """Get valid movement directions from a position.

        Returns direction values where movement is possible (not blocked by
        walls or board boundaries). Does not include STAY. Does not account
        for mud state.

        Direction values: UP=0, RIGHT=1, DOWN=2, LEFT=3.
        Convert to Direction enums with: ``[Direction(v) for v in valid]``

        Args:
            pos: Position to check, as Coordinates or (x, y) tuple

        Returns:
            List of direction integers for valid moves

        Raises:
            ValueError: If position is outside board bounds
        """
        ...

    def effective_actions(self, pos: Coordinates | tuple[int, int]) -> list[int]:
        """Get effective action mapping for a position (ignores mud state).

        Returns a list of 5 integers where ``result[action] = effective_action``.
        Blocked actions (walls, boundaries) map to STAY (4).
        Valid actions map to themselves.

        Direction values: UP=0, RIGHT=1, DOWN=2, LEFT=3, STAY=4.

        Example: at corner (0,0) with no walls::

            [0, 1, 4, 4, 4]  # UP=valid, RIGHT=valid, DOWN->STAY, LEFT->STAY, STAY->STAY

        Args:
            pos: Position to check, as Coordinates or (x, y) tuple

        Returns:
            List of 5 integers mapping each action to its effective action

        Raises:
            ValueError: If position is outside board bounds
        """
        ...

    def effective_actions_p1(self) -> list[int]:
        """Get effective action mapping for player 1, accounting for mud.

        If player 1 is in mud, all actions map to STAY: ``[4, 4, 4, 4, 4]``.
        Otherwise, returns the same as ``effective_actions(player1_position)``.

        Returns:
            List of 5 integers mapping each action to its effective action
        """
        ...

    def effective_actions_p2(self) -> list[int]:
        """Get effective action mapping for player 2, accounting for mud.

        If player 2 is in mud, all actions map to STAY: ``[4, 4, 4, 4, 4]``.
        Otherwise, returns the same as ``effective_actions(player2_position)``.

        Returns:
            List of 5 integers mapping each action to its effective action
        """
        ...

    def step(self, p1_move: int, p2_move: int) -> tuple[bool, list[Coordinates]]:
        """Execute one game step with the given moves.

        Use this for straightforward game execution (playing games, collecting
        data, running simulations). For game tree search where you need to
        backtrack, use ``make_move()`` / ``unmake_move()`` instead.

        Args:
            p1_move: Direction for player 1 (0-4: UP, RIGHT, DOWN, LEFT, STAY)
            p2_move: Direction for player 2 (0-4: UP, RIGHT, DOWN, LEFT, STAY)

        Returns:
            Tuple of (game_over, collected_cheese) where game_over is True
            if the game ended this turn and collected_cheese lists positions
            where cheese was picked up.
        """
        ...

    def make_move(self, p1_move: int, p2_move: int) -> MoveUndo:
        """Execute a move and return undo information for backtracking.

        Use this (with ``unmake_move()``) for game tree search algorithms
        like MCTS or minimax where you need to explore branches and undo them.
        For straightforward game execution, use ``step()`` instead.

        Args:
            p1_move: Direction for player 1 (0-4: UP, RIGHT, DOWN, LEFT, STAY)
            p2_move: Direction for player 2 (0-4: UP, RIGHT, DOWN, LEFT, STAY)

        Returns:
            MoveUndo object that must be passed to ``unmake_move()`` to revert
            this move. Undo objects must be applied in LIFO order (most recent
            move undone first).
        """
        ...

    def unmake_move(self, undo: MoveUndo) -> None:
        """Revert a move using saved undo information.

        Restores all game state (positions, scores, cheese, mud timers, turn
        counter) to what it was before the corresponding ``make_move()`` call.

        Undo objects must be applied in LIFO order — always undo the most
        recent ``make_move()`` first.

        Args:
            undo: MoveUndo object from a previous ``make_move()`` call
        """
        ...

    def reset(self, seed: int | None = None) -> None:
        """Reset the game to initial state.

        Args:
            seed: Optional random seed for reproducible maze generation
        """
        ...

    def get_observation(self, is_player_one: bool) -> GameObservation:
        """Get the current game observation for a player.

        Args:
            is_player_one: True to get player 1's perspective, False for player 2

        Returns:
            GameObservation containing the game state from the player's perspective
        """
        ...

# Alias for the Rust MoveUndo type
PyMoveUndo = MoveUndo
