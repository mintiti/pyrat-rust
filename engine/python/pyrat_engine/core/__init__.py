"""PyRat Engine Core - Low-level Rust bindings.

This package provides direct access to the Rust game engine implementation.
For most use cases, prefer the high-level API in pyrat_engine.game.
"""

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
