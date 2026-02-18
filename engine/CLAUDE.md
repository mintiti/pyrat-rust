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
- `cargo bench` runs criterion benchmarks (game init + full game across preset sizes and wall/mud combos)
- `cargo run --bin profile_game --release --no-default-features` prints a throughput table for all scenarios
- `cargo run --bin profile_game --release --no-default-features -- <size>/<combo>` runs a single scenario in a tight loop for profiling (e.g. `default/default`, `large/walls_only`)
- The per-turn hot path is documented in `rust/src/game/game_logic.rs` (module-level doc comment)

### Profiling with samply
```bash
# One-time setup
cargo install samply

# Add to ~/.cargo/config.toml (creates debug symbols without full debug overhead):
# [profile.profiling]
# inherits = "release"
# debug = true

# Build and profile (table mode — runs all scenarios, then open the profile)
cargo build --profile profiling --bin profile_game --no-default-features
samply record ./target/profiling/profile_game

# Single-scenario mode — attach profiler, Ctrl+C when done
samply record ./target/profiling/profile_game default/default
```

## Common Tasks

### Adding a new game feature
1. Implement in Rust (`rust/src/game/`)
2. Expose via bindings (`rust/src/bindings/`)
3. Add Python wrapper if needed (`python/pyrat_engine/`)
4. Write tests in both Rust and Python

### Debugging
- Use `RUST_LOG=debug` for Rust logging
- Python tests can be run with `-v` for verbose output
- The `profile_game` binary helps analyze performance (see Performance Notes)

## CI/CD
The engine is tested on Python 3.8-3.11 with:
- Rust formatting (`cargo fmt --check`)
- Rust linting (`cargo clippy`) - runs with and without Python features
- Rust unit tests (`cargo test --lib --no-default-features`)
- Python integration tests (`pytest`)

### Feature Flags
- `python` (default): Enables Python bindings via PyO3

Rust tests run without Python features to avoid linking issues.

## Game Creation API

### Terminology

Presets are defined along two axes:

| Axis | Values | Meaning |
|------|--------|---------|
| **Size** | `tiny`, `small`, `medium`, `large`, `huge` | Board dimensions, cheese count, max turns |
| **Maze type** | `classic`, `open` | Wall/mud density |

- **classic** — 0.7 wall density, 0.1 mud density (the `MazeParams` default)
- **open** — no walls, no mud
- **corner starts** — p1 at (0,0), p2 at (width-1, height-1) (all presets use this)

### Rust: Typestate Builder + GameConfig

The Rust API uses a two-phase system:
1. **`GameBuilder`** — assembles a `GameConfig` through a compile-time enforced sequence (maze → players → cheese)
2. **`GameConfig`** — stamps out `GameState` instances via `create(Option<u64>)`, enabling reuse for RL training

```rust
use pyrat_engine::{GameBuilder, GameConfig, MazeParams};

// Quick classic game
let config = GameConfig::classic(21, 15, 41);
let game = config.create(Some(42));

// Builder with named maze constructors
let config = GameBuilder::new(21, 15)
    .with_classic_maze()                     // or with_open_maze(), with_random_maze(params), with_custom_maze(walls, mud)
    .with_corner_positions()                 // or with_random_positions() / with_custom_positions(p1, p2)
    .with_random_cheese(41, true)            // or with_custom_cheese(vec![...])
    .build();

// Named presets: tiny, small, medium, large, huge, open, asymmetric
let config = GameConfig::preset("large").unwrap();
let game = config.create(Some(42));
```

`MazeParams` named constructors: `MazeParams::classic()`, `MazeParams::open()`. Fields: `target_density` (wall prob, 0–1), `connected` (bool), `symmetry` (bool), `mud_density` (0–1), `mud_range` (max mud cost).

`GameState` constructors (`new`, `new_with_config`, etc.) are `pub(crate)` — use the builder from outside the crate.

### Python API

```python
from pyrat_engine import PyRat

# Default game (21x15, 41 cheese, symmetric, classic maze)
game = PyRat()

# Custom parameters
game = PyRat(width=31, height=21, cheese_count=85, max_turns=500)

# Presets: tiny, small, medium, large, huge, open, asymmetric
game = PyRat.create_preset("large", seed=42)

# Custom maze layout
game = PyRat.create_from_maze(width=15, height=11, walls=[((0,0),(0,1))], seed=42)

# Custom starting positions
game = PyRat.create_with_starts(21, 15, (5,5), (15,9), preset="medium", seed=42)

# Full custom
game = PyRat.create_custom(
    width=21, height=15,
    walls=[((0,0),(0,1))], mud=[((2,2),(3,2),3)],
    cheese=[(5,5),(10,10)], player1_pos=(0,0), player2_pos=(20,14)
)
```
