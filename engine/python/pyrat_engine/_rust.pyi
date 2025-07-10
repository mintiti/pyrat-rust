import numpy as np

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
    @staticmethod
    def create_custom(
        width: int,
        height: int,
        walls: list[tuple[tuple[int, int], tuple[int, int]]] = [],
        mud: list[tuple[tuple[int, int], tuple[int, int], int]] = [],
        cheese: list[tuple[int, int]] = [],
        player1_pos: tuple[int, int] | None = None,
        player2_pos: tuple[int, int] | None = None,
        max_turns: int = 300,
    ) -> PyGameState:
        """Create a game with a fully specified maze configuration.

        Args:
            width: Width of the game board
            height: Height of the game board
            walls: List of wall pairs, each defined by two (x,y) positions
            mud: List of mud patches, each defined by two positions and mud value
            cheese: List of cheese positions as (x,y) coordinates
            player1_pos: Starting position for player 1 (default: (0,0))
            player2_pos: Starting position for player 2 (default: (width-1, height-1))
            max_turns: Maximum number of turns before game ends

        Returns:
            A new PyGameState instance with the specified configuration
        """
        ...

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
    def player1_position(self) -> tuple[int, int]:
        """Current (x,y) position of player 1."""
        ...

    @property
    def player2_position(self) -> tuple[int, int]:
        """Current (x,y) position of player 2."""
        ...

    @property
    def player1_score(self) -> float:
        """Current score of player 1."""
        ...

    @property
    def player2_score(self) -> float:
        """Current score of player 2."""
        ...

    def cheese_positions(self) -> list[tuple[int, int]]:
        """Get positions of all remaining cheese pieces.

        Returns:
            List of (x,y) coordinates where cheese pieces are located
        """
        ...

    def mud_entries(self) -> list[tuple[tuple[int, int], tuple[int, int], int]]:
        """Get all mud patches in the maze.

        Returns:
            List of mud entries, each containing:
            - Starting position (x1,y1)
            - Ending position (x2,y2)
            - Number of turns required to cross the mud
        """
        ...

    def step(self, p1_move: int, p2_move: int) -> tuple[bool, list[tuple[int, int]]]:
        """Execute one game step with the given moves.

        Args:
            p1_move: Direction for player 1 (0-4: UP, RIGHT, DOWN, LEFT, STAY)
            p2_move: Direction for player 2 (0-4: UP, RIGHT, DOWN, LEFT, STAY)

        Returns:
            Tuple containing:
            - Whether the game is over (bool)
            - List of positions where cheese was collected this turn
        """
        ...

    def make_move(self, p1_move: int, p2_move: int) -> PyMoveUndo:
        """Make a move and return undo information.

        Similar to step(), but returns information needed to undo the move.
        Useful for game tree search algorithms.

        Args:
            p1_move: Direction for player 1 (0-4: UP, RIGHT, DOWN, LEFT, STAY)
            p2_move: Direction for player 2 (0-4: UP, RIGHT, DOWN, LEFT, STAY)

        Returns:
            PyMoveUndo object containing state information for undoing the move
        """
        ...

    def unmake_move(self, undo: PyMoveUndo) -> None:
        """Undo a move using saved undo information.

        Args:
            undo: PyMoveUndo object from a previous make_move() call
        """
        ...

    def reset(self, seed: int | None = None) -> None:
        """Reset the game to initial state.

        Args:
            seed: Optional random seed for reproducible maze generation
        """
        ...

    def get_observation(self, is_player_one: bool) -> PyGameObservation:
        """Get the current game observation for a player.

        Args:
            is_player_one: True to get player 1's perspective, False for player 2

        Returns:
            PyGameObservation containing the game state from the player's perspective
        """
        ...

class PyObservationHandler:
    """Handles efficient updates and access to game observations.

    This class manages the observation state for the game, including cheese positions
    and movement constraints, providing efficient updates during gameplay.
    """

    def __init__(self, game: PyGameState) -> None:
        """Creates a new observation handler for tracking game state.

        Args:
            game: The game state to create observations for
        """
        ...

    def update_collected_cheese(self, collected: list[tuple[int, int]]) -> None:
        """Updates the observation state after cheese collection.

        Efficiently updates internal state when cheese is collected during gameplay,
        avoiding full state recalculation.

        Args:
            collected: List of (x,y) coordinates where cheese was collected
        """
        ...

    def update_restored_cheese(self, restored: list[tuple[int, int]]) -> None:
        """Updates the observation state when cheese is restored during move undo.

        Restores cheese positions when moves are undone, maintaining consistency
        with the game state.

        Args:
            restored: List of (x,y) coordinates where cheese should be restored
        """
        ...

    def get_observation(
        self, game: PyGameState, is_player_one: bool
    ) -> PyGameObservation:
        """Gets the current game observation from a player's perspective.

        Returns a complete observation of the game state, including player positions,
        scores, cheese locations, and movement constraints.

        Args:
            game: Current game state
            is_player_one: True to get player 1's perspective, False for player 2

        Returns:
            Complete game state observation from the specified player's perspective
        """
        ...

class PyGameObservation:
    """Game state observation from a player's perspective.

    This class provides a complete view of the game state from either player's perspective,
    including positions, scores, mud status, and the current game progression.
    All coordinates are provided as (x,y) tuples where (0,0) is the bottom-left corner.

    Note:
        When is_player_one=True in get_observation(), this represents player 1's view.
        When False, player/opponent properties are swapped to represent player 2's view.
    """

    @property
    def player_position(self) -> tuple[int, int]:
        """Current position of the observing player.

        Returns:
            (x,y) coordinates of the player's position
        """
        ...

    @property
    def player_mud_turns(self) -> int:
        """Remaining turns the observing player is stuck in mud.

        Returns:
            Number of turns remaining in mud (0 if not in mud)
        """
        ...

    @property
    def player_score(self) -> float:
        """Current score of the observing player.

        Returns:
            Player's score (number of cheese pieces collected)
        """
        ...

    @property
    def opponent_position(self) -> tuple[int, int]:
        """Current position of the opponent player.

        Returns:
            (x,y) coordinates of the opponent's position
        """
        ...

    @property
    def opponent_mud_turns(self) -> int:
        """Remaining turns the opponent is stuck in mud.

        Returns:
            Number of turns remaining in mud (0 if not in mud)
        """
        ...

    @property
    def opponent_score(self) -> float:
        """Current score of the opponent player.

        Returns:
            Opponent's score (number of cheese pieces collected)
        """
        ...

    @property
    def current_turn(self) -> int:
        """Current game turn number.

        Returns:
            Current turn (starts at 0)
        """
        ...

    @property
    def max_turns(self) -> int:
        """Maximum number of turns before game truncation.

        Returns:
            Maximum allowed turns for this game
        """
        ...

    @property
    def cheese_matrix(self) -> np.ndarray[tuple[int, ...], np.dtype[np.uint8]]:
        """Binary matrix indicating cheese positions.

        Returns:
            2D numpy array of shape (width, height) where 1 indicates
            cheese presence and 0 indicates no cheese
        """
        ...

    @property
    def movement_matrix(self) -> np.ndarray[tuple[int, ...], np.dtype[np.int8]]:
        """Matrix encoding valid moves and their costs.

        Returns:
            3D numpy array of shape (width, height, 4) where:
            - The first two dimensions correspond to board positions
            - The third dimension corresponds to moves [UP, RIGHT, DOWN, LEFT]
            - Values:
                -1: Invalid move (wall or out of bounds)
                0: Valid immediate move
                N>0: Valid move with N turns of mud delay
        """
        ...

class PyGameConfigBuilder:
    """Builder for creating custom PyRat game configurations.

    This class provides a fluent interface for constructing custom game states
    with specific maze layouts, including walls, mud patches, cheese positions,
    and player starting positions.

    Example:
        >>> game = (PyGameConfigBuilder(width=4, height=4)
        ...         .with_walls([((1, 1), (1, 2))])  # Vertical wall
        ...         .with_mud([((1, 1), (2, 1), 3)])  # 3-turn mud
        ...         .with_cheese([(1, 2), (3, 1)])
        ...         .with_player1_pos((0, 0))
        ...         .with_player2_pos((3, 3))
        ...         .with_max_turns(100)
        ...         .build())

    Args:
        width: Width of the game board
        height: Height of the game board
    """

    def __init__(self, width: int, height: int) -> None: ...
    def with_walls(
        self, walls: list[tuple[tuple[int, int], tuple[int, int]]]
    ) -> PyGameConfigBuilder:
        """Add walls to the maze configuration.

        Args:
            walls: List of wall definitions, where each wall is defined by two adjacent
                  cell positions it blocks movement between.
                  Format: [((x1,y1), (x2,y2)), ...]

        Returns:
            Self for method chaining

        Example:
            >>> builder.with_walls([
            ...     ((1, 1), (1, 2)),  # Vertical wall between (1,1) and (1,2)
            ...     ((0, 0), (1, 0)),  # Horizontal wall between (0,0) and (1,0)
            ... ])
        """
        ...

    def with_mud(
        self, mud: list[tuple[tuple[int, int], tuple[int, int], int]]
    ) -> PyGameConfigBuilder:
        """Add mud patches to the maze configuration.

        Args:
            mud: List of mud definitions, where each mud is defined by two adjacent
                 cell positions and the number of turns it takes to cross.
                 Format: [((x1,y1), (x2,y2), turns), ...]

        Returns:
            Self for method chaining

        Example:
            >>> builder.with_mud([
            ...     ((1, 1), (2, 1), 3),  # 3-turn mud between (1,1) and (2,1)
            ... ])
        """
        ...

    def with_cheese(self, cheese: list[tuple[int, int]]) -> PyGameConfigBuilder:
        """Set cheese positions in the maze.

        Args:
            cheese: List of coordinates where cheese should be placed.
                   Format: [(x1,y1), (x2,y2), ...]

        Returns:
            Self for method chaining

        Example:
            >>> builder.with_cheese([
            ...     (1, 2),  # Cheese at (1,2)
            ...     (3, 1),  # Cheese at (3,1)
            ... ])
        """
        ...

    def with_player1_pos(self, pos: tuple[int, int]) -> PyGameConfigBuilder:
        """Set the starting position for player 1.

        Args:
            pos: Tuple of (x,y) coordinates for player 1's starting position

        Returns:
            Self for method chaining

        Example:
            >>> builder.with_player1_pos((0, 0))  # Start at bottom left
        """
        ...

    def with_player2_pos(self, pos: tuple[int, int]) -> PyGameConfigBuilder:
        """Set the starting position for player 2.

        Args:
            pos: Tuple of (x,y) coordinates for player 2's starting position

        Returns:
            Self for method chaining

        Example:
            >>> builder.with_player2_pos((3, 3))  # Start at top right
        """
        ...

    def with_max_turns(self, max_turns: int) -> PyGameConfigBuilder:
        """Set the maximum number of turns for the game.

        Args:
            max_turns: Maximum number of turns before the game is truncated.
                      Must be greater than 0.

        Returns:
            Self for method chaining

        Example:
            >>> builder.with_max_turns(100)
        """
        ...

    def build(self) -> PyGameState:
        """Construct and return the final game state.

        Returns:
            A new PyGameState instance with the configured parameters

        Raises:
            ValueError: If the configuration is invalid (e.g., invalid positions,
                       overlapping walls/mud, no cheese placed)

        Example:
            >>> game = builder.build()
        """
        ...
