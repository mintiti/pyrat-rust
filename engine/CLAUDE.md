# CLAUDE.md - PyRat Engine

This file provides guidance to Claude Code when working with the PyRat engine component.

## Component Overview

The PyRat Engine is the core game implementation, providing:
- High-performance Rust game logic
- Python bindings via PyO3
- PettingZoo-compatible environment interface
- Support for reinforcement learning research
- Multiple game creation methods with presets and custom configurations

## Quick Development Guide

### Setup (Standalone)
```bash
# From engine directory
uv sync
uv run maturin develop --release
```

### Setup (Workspace - Recommended)
```bash
# From repository root
uv sync --all-extras  # This installs engine and all dependencies including dev tools
cd engine
uv run maturin develop --release  # Build Rust extension
```

### Testing
```bash
# Rust tests (without Python bindings)
cargo test --lib --no-default-features

# Python tests
pytest python/tests -v

# Or from repo root
make test-engine
```

### Key Files
- `rust/src/game/game_logic.rs` - Core game rules
- `rust/src/bindings/game.rs` - Python bindings
- `python/pyrat_engine/env.py` - PettingZoo environment
- `python/pyrat_engine/game.py` - High-level Python API

## Package Details
- **Package name**: `pyrat-engine` (was `pyrat` before monorepo)
- **Import**: `from pyrat_engine import PyRatEnv, Direction`
- **Rust module**: `pyrat_engine._rust`

## Performance Notes
- The engine achieves 10+ million moves/second
- Use `cargo bench` to run performance benchmarks
- Profile binaries available for detailed performance analysis

## Common Tasks

### Adding a new game feature
1. Implement in Rust (`rust/src/game/`)
2. Expose via bindings (`rust/src/bindings/`)
3. Add Python wrapper if needed (`python/pyrat_engine/`)
4. Write tests in both Rust and Python

### Debugging
- Use `RUST_LOG=debug` for Rust logging
- Python tests can be run with `-v` for verbose output
- The `profile_process_turn` binary helps analyze performance

## CI/CD
The engine is tested on Python 3.8-3.11 with:
- Rust formatting (`cargo fmt --check`)
- Rust linting (`cargo clippy`) - runs with and without Python features
- Rust unit tests (`cargo test --lib --no-default-features`)
- Python integration tests (`pytest`)

### Feature Flags
- `python` (default): Enables Python bindings via PyO3
- `flame`: Enables profiling with flame graphs

Rust tests run without Python features to avoid linking issues.

## Game Creation API

The engine provides multiple ways to create games, supporting various use cases from quick testing to precise control:

### 1. Basic Constructor
```python
from pyrat_engine import PyRat

# Default game (21x15, 41 cheese, symmetric)
game = PyRat()

# Custom parameters
game = PyRat(width=31, height=21, cheese_count=85, max_turns=500)
```

### 2. Preset Configurations
```python
from pyrat_engine.core.game import GameState as PyGameState

# Available presets:
# - "tiny": 11x9 board, 13 cheese, 150 turns
# - "small": 15x11 board, 21 cheese, 200 turns
# - "default": 21x15 board, 41 cheese, 300 turns
# - "large": 31x21 board, 85 cheese, 400 turns
# - "huge": 41x31 board, 165 cheese, 500 turns
# - "empty": 21x15, no walls/mud, for testing
# - "asymmetric": Standard size but asymmetric generation

game_state = PyGameState.create_preset("large", seed=42)
```

### 3. Custom Maze Layout
```python
# Define specific walls, generate random cheese
walls = [
    ((0, 0), (0, 1)),  # Wall between (0,0) and (0,1)
    ((1, 1), (2, 1)),  # Wall between (1,1) and (2,1)
]

game_state = PyGameState.create_from_maze(
    width=15,
    height=11,
    walls=walls,
    seed=42,        # For reproducible cheese placement
    max_turns=200
)
```

### 4. Custom Starting Positions
```python
# Use preset configuration but with custom player positions
game_state = PyGameState.create_with_starts(
    width=21,
    height=15,
    player1_start=(5, 5),
    player2_start=(15, 9),
    preset="default",
    seed=42
)
```

### 5. Full Custom Configuration
```python
# Complete control over all game elements
walls = [((0, 0), (0, 1)), ((1, 1), (2, 1))]
mud = [((2, 2), (3, 2), 3)]  # 3 turns to traverse
cheese = [(5, 5), (10, 10), (15, 7)]

game_state = PyGameState.create_custom(
    width=21,
    height=15,
    walls=walls,
    mud=mud,
    cheese=cheese,
    player1_pos=(0, 0),
    player2_pos=(20, 14),
    max_turns=300
)
```
