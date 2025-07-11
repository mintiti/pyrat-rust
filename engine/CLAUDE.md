# CLAUDE.md - PyRat Engine

This file provides guidance to Claude Code when working with the PyRat engine component.

## Component Overview

The PyRat Engine is the core game implementation, providing:
- High-performance Rust game logic
- Python bindings via PyO3
- PettingZoo-compatible environment interface
- Support for reinforcement learning research

## Quick Development Guide

### Setup (Standalone)
```bash
# From engine directory
uv venv
source .venv/bin/activate
uv pip install -e ".[dev]"
maturin develop --release
```

### Setup (Workspace - Recommended)
```bash
# From repository root
uv sync  # This installs engine and all dependencies
cd engine
maturin develop --release  # Build Rust extension
```

### Testing
```bash
# Rust tests
cd rust && cargo test --lib

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
- Rust linting (`cargo clippy`)
- Rust unit tests (`cargo test --lib`)
- Python integration tests (`pytest`)
