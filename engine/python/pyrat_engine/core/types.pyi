"""Core types for the PyRat game engine.

This module contains the fundamental types used throughout the engine:
- Coordinates: Position on the game board
- Direction: Movement directions
- Wall: Barriers between cells
- Mud: Passages that slow movement
"""

class Coordinates:
    """A position on the game board with x and y coordinates.

    This is the core position type used throughout the engine.
    Coordinates are 0-indexed, with (0,0) at the bottom-left corner.

    Args:
        x: X coordinate (0-255)
        y: Y coordinate (0-255)

    Attributes:
        x: The x coordinate (read-only)
        y: The y coordinate (read-only)
    """

    x: int
    y: int

    def __init__(self, x: int, y: int) -> None: ...
    def get_neighbor(self, direction: int) -> Coordinates:
        """Get the coordinates of an adjacent cell in the given direction.

        Args:
            direction: Direction value (0-4: UP, RIGHT, DOWN, LEFT, STAY)

        Returns:
            New Coordinates object for the neighboring position

        Raises:
            ValueError: If direction is not in range 0-4
        """
        ...

    def is_adjacent_to(self, other: Coordinates) -> bool:
        """Check if this position is adjacent to another (not diagonally).

        Args:
            other: Another Coordinates object

        Returns:
            True if positions are orthogonally adjacent, False otherwise
        """
        ...

    def manhattan_distance(self, other: Coordinates) -> int:
        """Calculate Manhattan distance to another position.

        Args:
            other: Another Coordinates object

        Returns:
            Manhattan distance (sum of absolute differences in x and y)
        """
        ...

    def __repr__(self) -> str: ...
    def __str__(self) -> str: ...
    def __getitem__(self, index: int) -> int: ...
    def __len__(self) -> int: ...
    def __hash__(self) -> int: ...
    def __eq__(self, other: object) -> bool: ...
    def __ne__(self, other: object) -> bool: ...

class Direction:
    """Movement directions in the game.

    This is an enum with the following values:
    - UP = 0: Move up (increase y)
    - RIGHT = 1: Move right (increase x)
    - DOWN = 2: Move down (decrease y)
    - LEFT = 3: Move left (decrease x)
    - STAY = 4: Don't move
    """

    UP: int = 0
    RIGHT: int = 1
    DOWN: int = 2
    LEFT: int = 3
    STAY: int = 4

class Wall:
    """A wall between two adjacent cells that blocks movement.

    Walls are always between orthogonally adjacent cells.
    The order of positions is normalized (smaller position first).

    Args:
        pos1: First position
        pos2: Second position (must be adjacent to pos1)

    Raises:
        ValueError: If positions are not adjacent

    Attributes:
        pos1: First position (read-only)
        pos2: Second position (read-only)
    """

    pos1: Coordinates
    pos2: Coordinates

    def __init__(
        self,
        pos1: Coordinates | tuple[int, int],
        pos2: Coordinates | tuple[int, int],
    ) -> None: ...
    def blocks_movement(self, from_pos: Coordinates, to_pos: Coordinates) -> bool:
        """Check if this wall blocks movement between two positions.

        Args:
            from_pos: Starting position
            to_pos: Target position

        Returns:
            True if the wall blocks this movement, False otherwise
        """
        ...

    def __repr__(self) -> str: ...
    def __eq__(self, other: object) -> bool: ...
    def __ne__(self, other: object) -> bool: ...

class Mud:
    """A muddy passage between two cells that slows movement.

    Mud makes movement take multiple turns. A mud value of N means
    it takes N turns total to traverse (including the initial turn).

    Args:
        pos1: First position
        pos2: Second position (must be adjacent to pos1)
        value: Number of turns to traverse (must be >= 2)

    Raises:
        ValueError: If positions are not adjacent or value < 2

    Attributes:
        pos1: First position (read-only)
        pos2: Second position (read-only)
        value: Number of turns to traverse (read-only)
    """

    pos1: Coordinates
    pos2: Coordinates
    value: int

    def __init__(
        self,
        pos1: Coordinates | tuple[int, int],
        pos2: Coordinates | tuple[int, int],
        value: int,
    ) -> None: ...
    def affects_movement(self, from_pos: Coordinates, to_pos: Coordinates) -> bool:
        """Check if this mud affects movement between two positions.

        Args:
            from_pos: Starting position
            to_pos: Target position

        Returns:
            True if this mud affects the movement, False otherwise
        """
        ...

    def blocks_movement(self, from_pos: Coordinates, to_pos: Coordinates) -> bool:
        """Alias for affects_movement for consistency with Wall API."""
        ...

    def __repr__(self) -> str: ...
    def __eq__(self, other: object) -> bool: ...
    def __ne__(self, other: object) -> bool: ...

def direction_to_name(direction: int) -> str:
    """Convert a Direction value to its string name.

    Args:
        direction: Direction value (Direction.UP, Direction.DOWN, etc.)

    Returns:
        String name of the direction ("UP", "DOWN", "LEFT", "RIGHT", "STAY")
        Returns "STAY" for invalid direction values.
    """
    ...

def name_to_direction(name: str) -> int:
    """Convert a direction name string to a Direction value.

    Args:
        name: Direction name string (case-sensitive uppercase: "UP", "DOWN", etc.)

    Returns:
        Direction value (Direction.UP, Direction.DOWN, etc.)
        Returns Direction.STAY for invalid names.
    """
    ...

def is_valid_direction(direction: int | None) -> bool:
    """Check if a direction value is valid.

    Args:
        direction: Direction value to validate

    Returns:
        True if the direction is valid (UP, DOWN, LEFT, RIGHT, or STAY),
        False otherwise.
    """
    ...
