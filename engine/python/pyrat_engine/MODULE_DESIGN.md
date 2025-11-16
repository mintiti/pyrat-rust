# PyRat Engine Module Restructuring Design

## Current Structure

Currently, all Rust types are exposed in a single `_rust` module:
```python
# Everything in one module
from pyrat_engine._rust import (
    Coordinates, Direction, Wall, Mud,
    PyGameState, PyMoveUndo, PyGameObservation,
    PyObservationHandler, PyGameConfigBuilder
)
```

## Proposed Structure

Split the module into logical submodules for better organization:

### Option 1: Submodules within _rust
```python
# Types submodule
from pyrat_engine._rust.types import Coordinates, Direction, Wall, Mud

# Game state submodule
from pyrat_engine._rust.game import PyGameState, PyMoveUndo

# Observation submodule
from pyrat_engine._rust.observation import PyGameObservation, PyObservationHandler

# Builder submodule
from pyrat_engine._rust.builder import PyGameConfigBuilder
```

### Option 2: Direct submodules (rename _rust to core)
```python
# More pythonic names
from pyrat_engine.core.types import Coordinates, Direction, Wall, Mud
from pyrat_engine.core.game import GameState, MoveUndo
from pyrat_engine.core.observation import GameObservation, ObservationHandler
from pyrat_engine.core.builder import GameConfigBuilder
```

### Option 3: Flat structure with better names
```python
# Everything at package level but better organized
from pyrat_engine.types import Coordinates, Direction, Wall, Mud
from pyrat_engine.engine import GameState, MoveUndo
from pyrat_engine.observation import GameObservation, ObservationHandler
from pyrat_engine.builder import GameConfigBuilder
```

## Implementation Approach

Using PyO3's submodule support:

```rust
// In lib.rs
#[pymodule]
fn pyrat_engine(py: Python, m: &PyModule) -> PyResult<()> {
    // Create submodules
    let types_module = PyModule::new(py, "types")?;
    types::register_module(&types_module)?;
    m.add_submodule(&types_module)?;

    let game_module = PyModule::new(py, "game")?;
    game::register_module(&game_module)?;
    m.add_submodule(&game_module)?;

    // etc...
    Ok(())
}
```

## Benefits

1. **Better Organization**: Types are logically grouped
2. **Clearer Imports**: Users know where to find specific types
3. **Easier Discovery**: IDE autocomplete shows submodules
4. **Future Proof**: Easy to add new submodules as needed

## Migration Strategy

1. Keep backward compatibility with `_rust` module initially
2. Add deprecation warnings for direct `_rust` imports
3. Update documentation to use new import paths
4. Remove old module in next major version

## Recommendation

**Option 2** with the `core` name is recommended because:
- More descriptive than `_rust`
- Still indicates these are low-level engine types
- Allows clean separation from high-level Python API
- Follows Python naming conventions (no leading underscore for public API)

The final structure would be:
```
pyrat_engine/
├── __init__.py          # High-level API exports
├── game.py              # High-level PyRat class
├── env.py               # PettingZoo environment
├── core/                # Low-level Rust bindings
│   ├── __init__.py
│   ├── types.pyi        # Coordinates, Direction, Wall, Mud
│   ├── game.pyi         # GameState, MoveUndo
│   ├── observation.pyi  # GameObservation, ObservationHandler
│   └── builder.pyi      # GameConfigBuilder
└── _rust.pyi            # Deprecated, for backward compatibility
```
