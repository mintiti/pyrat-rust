# PyRat Engine Module Migration Guide

## Overview

The PyRat engine module structure has been reorganized for better clarity and organization. The compiled Rust module is now `_core` (private) with a clean Python wrapper API in the `core` package.

## Module Structure

```
pyrat_engine/
├── core/                    # Low-level Rust bindings
│   ├── __init__.py         # Re-exports from _core
│   ├── types.py            # Core types (Coordinates, Direction, Wall, Mud)
│   ├── game.py             # Game state classes
│   ├── observation.py      # Observation classes
│   └── builder.py          # Game configuration builder
├── game.py                 # High-level PyRat API
├── env.py                  # PettingZoo environment
└── _core.so               # Compiled Rust module (implementation detail)
```

## Import Changes

### Old Imports
```python
from pyrat_engine._rust import PyGameState, Coordinates, Wall, Mud
```

### New Imports

**Option 1: Import from submodules (recommended)**
```python
from pyrat_engine.core.types import Coordinates, Direction, Wall, Mud
from pyrat_engine.core.game import GameState, MoveUndo
from pyrat_engine.core.observation import GameObservation, ObservationHandler
from pyrat_engine.core.builder import GameConfigBuilder
```

**Option 2: Import from core package**
```python
from pyrat_engine.core import (
    Coordinates, Direction, Wall, Mud,
    GameState, MoveUndo,
    GameObservation, ObservationHandler,
    GameConfigBuilder
)
```

## Class Name Changes

The `Py` prefix has been removed from class names for cleaner API:
- `PyGameState` → `GameState`
- `PyMoveUndo` → `MoveUndo`
- `PyGameObservation` → `GameObservation`
- `PyObservationHandler` → `ObservationHandler`
- `PyGameConfigBuilder` → `GameConfigBuilder`

Note: The old names are still available as aliases for backward compatibility.

## Key Benefits

1. **Cleaner imports**: No more underscore-prefixed modules in public API
2. **Better organization**: Types, game logic, observations, and builders are logically separated
3. **Type hints**: Full .pyi stub files for excellent IDE support
4. **Consistent naming**: No more `Py` prefixes on everything

## Example Usage

```python
# Creating a game with the new structure
from pyrat_engine.core.types import Coordinates, Wall, Mud
from pyrat_engine.core.game import GameState

# Create a game
game = GameState(width=21, height=15, seed=42)

# Access player positions (returns Coordinates objects)
pos = game.player1_position
print(f"Player 1 is at ({pos.x}, {pos.y})")

# Create custom game elements
wall = Wall(Coordinates(0, 0), Coordinates(0, 1))
mud = Mud(Coordinates(1, 1), Coordinates(2, 1), value=3)
```
