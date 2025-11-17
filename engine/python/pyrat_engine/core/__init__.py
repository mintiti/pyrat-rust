"""PyRat Engine Core - Low-level Rust bindings.

This package provides direct access to the Rust game engine implementation.
For most use cases, prefer the high-level API in pyrat_engine.game.
"""

from typing import TYPE_CHECKING

# Import the compiled Rust module (private implementation detail)
import pyrat_engine._core as _impl

# Re-export submodules as module attributes for convenience
# Note: These are not real Python submodules, but attributes on the compiled module
types = _impl.types
game = _impl.game
observation = _impl.observation
builder = _impl.builder

# Also re-export commonly used classes at package level
# Since PyO3 submodules are just attributes, we access them this way
Coordinates = _impl.types.Coordinates
Direction = _impl.types.Direction
Wall = _impl.types.Wall
Mud = _impl.types.Mud

# Type alias for direction values (Direction.UP, Direction.RIGHT, etc. are ints)
# Use this in type hints when referring to direction values, not the Direction class
DirectionType = int

# Conditionally import types for type checking to avoid "not valid as a type" errors
if TYPE_CHECKING:
    from pyrat_engine.core.builder import PyGameConfigBuilder as GameConfigBuilder
    from pyrat_engine.core.game import PyGameState as GameState
    from pyrat_engine.core.game import PyMoveUndo as MoveUndo
    from pyrat_engine.core.observation import PyGameObservation as GameObservation
    from pyrat_engine.core.observation import PyObservationHandler as ObservationHandler
else:
    GameState = _impl.game.PyGameState
    MoveUndo = _impl.game.PyMoveUndo
    GameObservation = _impl.observation.PyGameObservation
    ObservationHandler = _impl.observation.PyObservationHandler
    GameConfigBuilder = _impl.builder.PyGameConfigBuilder

__all__ = [
    # Submodules
    "types",
    "game",
    "observation",
    "builder",
    # Types
    "Coordinates",
    "Direction",
    "DirectionType",
    "Wall",
    "Mud",
    # Game
    "GameState",
    "MoveUndo",
    # Observation
    "GameObservation",
    "ObservationHandler",
    # Builder
    "GameConfigBuilder",
]
