class PyMoveUndo:
    """Information needed to undo a move in the game.

    This class contains all state information required to reverse a move,
    including player positions, scores, and collected cheese.
    """
    @property
    def p1_pos(self) -> tuple[int, int]:
        """Player 1's position before the move."""
        ...

    @property
    def p2_pos(self) -> tuple[int, int]:
        """Player 2's position before the move."""
        ...

    @property
    def p1_target(self) -> tuple[int, int]:
        """Position player 1 was attempting to move to."""
        ...

    @property
    def p2_target(self) -> tuple[int, int]:
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
    def collected_cheese(self) -> list[tuple[int, int]]:
        """List of positions where cheese was collected during this move."""
        ...

    @property
    def turn(self) -> int:
        """Turn number before the move was made."""
        ...

class PyGameState:
    """Core game state implementation in Rust.

    This class provides the low-level interface to the Rust game engine.
    It manages all game state including:
    - Player positions and scores
    - Cheese placement and collection
    - Mud effects and movement delays
    - Turn counting and game termination

    Note:
        This is an internal class. Users should typically use the PyRat class
        instead, which provides a more Pythonic interface.

    Args:
        width: Board width (default: 21)
        height: Board height (default: 15)
        cheese_count: Number of cheese pieces (default: 41)
        symmetric: Whether to generate symmetric mazes (default: True)
        seed: Random seed for reproducible games (default: None)
    """
    def __init__(
        self,
        width: int | None = None,
        height: int | None = None,
        cheese_count: int | None = None,
        symmetric: bool = True,
        seed: int | None = None,
    ) -> None: ...
    @property
    def width(self) -> int: ...
    @property
    def height(self) -> int: ...
    @property
    def turn(self) -> int: ...
    @property
    def max_turns(self) -> int: ...
    @property
    def player1_position(self) -> tuple[int, int]: ...
    @property
    def player2_position(self) -> tuple[int, int]: ...
    @property
    def player1_score(self) -> float: ...
    @property
    def player2_score(self) -> float: ...
    def cheese_positions(self) -> list[tuple[int, int]]: ...
    def mud_entries(self) -> list[tuple[tuple[int, int], tuple[int, int], int]]: ...
    def step(
        self, p1_move: int, p2_move: int
    ) -> tuple[bool, list[tuple[int, int]]]: ...
    def make_move(self, p1_move: int, p2_move: int) -> PyMoveUndo: ...
    def unmake_move(self, undo: PyMoveUndo) -> None: ...
    def reset(self, seed: int | None = None) -> None: ...

class PyObservationHandler:
    def __init__(self, game: PyGameState) -> None: ...
    def update_collected_cheese(self, collected: list[tuple[int, int]]) -> None: ...
    def update_restored_cheese(self, restored: list[tuple[int, int]]) -> None: ...
    def get_observation(
        self, game: PyGameState, is_player_one: bool
    ) -> PyGameObservation: ...

class PyGameObservation:
    @property
    def player_position(self) -> tuple[int, int]: ...
    @property
    def player_mud_turns(self) -> int: ...
    @property
    def player_score(self) -> float: ...
    @property
    def opponent_position(self) -> tuple[int, int]: ...
    @property
    def opponent_mud_turns(self) -> int: ...
    @property
    def opponent_score(self) -> float: ...
    @property
    def current_turn(self) -> int: ...
    @property
    def max_turns(self) -> int: ...
