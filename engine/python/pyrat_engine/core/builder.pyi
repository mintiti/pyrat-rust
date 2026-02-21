"""Type stubs for the game builder and config classes."""

from pyrat_engine.core.game import PyRat
from pyrat_engine.core.types import Coordinates, Mud, Wall

class GameConfig:
    """Reusable game configuration. Stamps out PyRat instances via create().

    Use GameBuilder to construct, or use the preset()/classic() shortcuts.
    """

    @staticmethod
    def preset(name: str) -> GameConfig:
        """Look up a named preset configuration.

        Available presets: tiny, small, medium, large, huge, open, asymmetric.
        """
        ...

    @staticmethod
    def classic(width: int, height: int, cheese: int) -> GameConfig:
        """Standard game: classic maze, corner starts, symmetric random cheese."""
        ...

    def create(self, seed: int | None = None) -> PyRat:
        """Stamp out a new game from this config."""
        ...

    @property
    def width(self) -> int: ...
    @property
    def height(self) -> int: ...
    @property
    def max_turns(self) -> int: ...

class GameBuilder:
    """Builder for composing game configurations.

    Enforces that maze, players, and cheese strategies are all set before
    building. Each category must be set exactly once.

    Example:
        >>> config = (GameBuilder(21, 15)
        ...     .with_classic_maze()
        ...     .with_corner_positions()
        ...     .with_random_cheese(41)
        ...     .build())
        >>> game = config.create(seed=42)
    """

    def __init__(self, width: int, height: int) -> None: ...

    # Maze strategies (pick one)
    def with_classic_maze(self) -> GameBuilder:
        """Classic maze: 0.7 wall density, 0.1 mud density, connected, symmetric."""
        ...

    def with_open_maze(self) -> GameBuilder:
        """Open maze: no walls, no mud."""
        ...

    def with_random_maze(
        self,
        *,
        wall_density: float = 0.7,
        mud_density: float = 0.1,
        mud_range: int = 3,
        connected: bool = True,
        symmetric: bool = True,
    ) -> GameBuilder:
        """Random maze with custom parameters."""
        ...

    def with_custom_maze(
        self,
        walls: list[Wall] | list[tuple[tuple[int, int], tuple[int, int]]],
        mud: list[Mud] | list[tuple[tuple[int, int], tuple[int, int], int]] = [],
    ) -> GameBuilder:
        """Fixed maze layout from explicit walls and mud."""
        ...

    # Player strategies (pick one)
    def with_corner_positions(self) -> GameBuilder:
        """Player 1 at (0,0), player 2 at (width-1, height-1)."""
        ...

    def with_random_positions(self) -> GameBuilder:
        """Both players placed randomly (guaranteed different cells)."""
        ...

    def with_custom_positions(
        self,
        p1: Coordinates | tuple[int, int],
        p2: Coordinates | tuple[int, int],
    ) -> GameBuilder:
        """Place players at explicit positions."""
        ...

    # Cheese strategies (pick one)
    def with_random_cheese(self, count: int, symmetric: bool = True) -> GameBuilder:
        """Place count cheese randomly, optionally with 180 degree symmetry."""
        ...

    def with_custom_cheese(
        self,
        positions: list[Coordinates] | list[tuple[int, int]],
    ) -> GameBuilder:
        """Place cheese at exact positions."""
        ...

    # Other
    def with_max_turns(self, max_turns: int) -> GameBuilder:
        """Override the default max_turns (300)."""
        ...

    def build(self) -> GameConfig:
        """Consume the builder and produce a GameConfig.

        Raises ValueError if maze, players, or cheese strategy is not set.
        """
        ...
