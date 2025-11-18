# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

PyRat is a monorepo containing the complete PyRat ecosystem for a competitive maze game. The repository is organized into multiple components:

- **engine/**: Rust game engine with PyO3 bindings - core game logic and Python API
- **protocol/**: Text-based AI communication protocol and Python SDK (`pyrat_base`)
- **cli/**: Command-line game runner tool (`pyrat-game` command)

This monorepo structure enables clean separation of concerns while maintaining a cohesive ecosystem.

## What is PyRat?

PyRat is a competitive two-player maze game where a Rat and a Python race to collect the most cheese. The game features:

- **Simultaneous Movement**: Both players move at the same time, making it a game of prediction and strategy
- **Mud Mechanics**: Certain passages have mud that delays movement - players commit to a direction but take multiple turns to traverse
- **Symmetric Mazes**: The maze layout is mirrored for fairness between players
- **Shared Resources**: Players compete for the same cheese pieces scattered throughout the maze

## Game Rules

### Setup
- **Grid**: Rectangular maze (default 21×15) with walls between cells
- **Players**:
  - Rat starts at top-right corner (height-1, width-1)
  - Python starts at bottom-left corner (0, 0)
- **Cheese**: Randomly placed on cells in symmetric positions
- **Maze**: Fully connected (always a path between any two cells)

### Movement
- **Actions**: UP, DOWN, LEFT, RIGHT, or STAY
- **Invalid moves** (into walls/boundaries) default to STAY
- **Simultaneous**: Both players move at the same time
- **Collision**: Players can occupy the same cell (no blocking)

### Mud Mechanics
- Mud exists between connected cells (where there's no wall)
- Mud value N means it takes N turns to traverse:
  - Turn 1: Player commits to direction, stays in starting cell
  - Turns 2 to N-1: Player is "stuck" in mud, cannot collect cheese
  - Turn N: Player arrives at destination
- While in mud, all movement commands are ignored

### Scoring
- **Normal collection**: 1 point when reaching a cheese
- **Simultaneous collection**: 0.5 points each if both players collect same cheese
- **Cannot collect while stuck in mud**

### Victory Conditions
Game ends immediately when:
1. Any player scores > total_cheese/2
2. All cheese is collected
3. Maximum turns (300) reached

Winner is determined by highest score, with draws possible.

## Development Commands

### Monorepo Setup
```bash
# From repository root
make help  # Show all available commands
make all   # Build all components
```

### Workspace Development (Recommended)
This repository uses `uv` workspaces for managing the monorepo structure. This ensures proper dependency resolution between components.

```bash
# From repository root
uv sync --all-extras  # Sync all workspace dependencies with dev tools

# This automatically:
# - Creates a virtual environment at .venv
# - Installs all workspace members (engine, protocol/pyrat_base)
# - Resolves cross-dependencies correctly
# - Installs dev dependencies like maturin, pytest, ruff, etc.

# Install pre-commit hooks (required for all developers)
uv run pre-commit install
uv run pre-commit install --hook-type pre-push
```

### Engine Development
```bash
# Recommended: Use workspace setup from repository root
# Run `uv sync --all-extras` from root - this handles all components

# Or, for engine-specific development:
cd engine

# Install uv (if not already installed)
curl -LsSf https://astral.sh/uv/install.sh | sh

# Install Python dependencies and build Rust extension
uv sync
uv run maturin develop --release

# Install pre-commit hooks (from repository root)
cd ..
uv run pre-commit install
uv run pre-commit install --hook-type pre-push
```

### Code Quality Checks
```bash
# From repository root
make check  # Run all checks
make fmt    # Format all code

# From engine directory
cd engine

# Format Rust code
cargo fmt

# Check Rust formatting (CI will fail if not formatted)
cargo fmt --all -- --check

# Run Rust linter with all warnings as errors
cargo clippy --all-targets --all-features -- -D warnings

# Run Rust linter (ignoring PyO3 warnings for CI)
cargo clippy --all-targets --all-features -- -D warnings -A non-local-definitions

# Run Python tests
pytest python/tests -v

# Run specific test
pytest python/tests/test_env.py::test_custom_maze -v
```

### Building and Testing
```bash
# From repository root
make test        # Run all tests
make test-engine # Run engine tests only
make test-protocol # Run protocol tests only
make test-cli    # Run CLI tests only
make bench       # Run benchmarks

# From engine directory
cd engine

# Build the Rust library
cargo build --release

# Run Rust tests (without Python features to avoid linking issues)
cd engine && cargo test --lib --no-default-features

# Run benchmarks
cargo bench

# Build Python package with maturin
maturin build --release
```

### Running Games with CLI
```bash
# From repository root (after uv sync --all-extras)
# Run game between two AIs
pyrat-game protocol/pyrat_base/pyrat_base/examples/greedy_ai.py \
           protocol/pyrat_base/pyrat_base/examples/random_ai.py

# Custom game configuration
pyrat-game --width 31 --height 21 --cheese 85 --seed 42 \
           --delay 0.1 --timeout 1.0 bot1.py bot2.py

# See all options
pyrat-game --help
```

### CI Debugging
```bash
# View CI run details
gh run view <run-id>

# View only failed CI logs
gh run view <run-id> --log-failed
```

### Important Notes
- Pre-commit hooks automatically run `cargo fmt` and other checks before commits
- The CI will run both `cargo fmt --check` and `cargo clippy`
- Python dependencies are in `engine/pyproject.toml` (not requirements.txt)
- Use `maturin develop` to build the Rust extension during development
- To manually run all pre-commit checks: `pre-commit run --all-files`

## Architecture

The monorepo follows a component-based architecture with the engine at its core:

### Engine Architecture
Hybrid Rust-Python design with PyO3 bindings:

**Rust Core** (`engine/rust/src/`):
- `game/`: Core game logic (board.rs, game_logic.rs, maze_generation.rs)
- `bindings/`: PyO3 bindings exposing Rust types and functions to Python
- Performance: 10+ million moves/second for game simulations

**Python Layer** (`engine/python/pyrat_engine/`):
- `env.py`: PettingZoo ParallelEnv wrapper for RL frameworks
- `game.py`: High-level game interface
- Provides Gymnasium/PettingZoo compatible API

### Working with Types
Types are exposed directly from Rust to Python - no tuple conversions:

**Coordinates:**
```python
pos = game.player1_position()  # Returns Coordinates object
x = pos.x  # NOT pos[0]
y = pos.y  # NOT pos[1]

# Helper methods available:
neighbor = pos.get_neighbor(Direction.UP)
distance = pos.manhattan_distance(other_pos)
is_next = pos.is_adjacent_to(other_pos)
```

**Backward compatibility:** Functions accept tuples via `CoordinatesInput`, but return `Coordinates` objects.

**Other types:**
- `Direction` - Enum: UP, DOWN, LEFT, RIGHT, STAY
- `Wall` - Wall between two coordinates
- `Mud` - Mud passage with turn count

### Accessing Game State
Key properties and methods on `GameState`:

```python
# Player positions (returns Coordinates)
pos1 = game.player1_position()
pos2 = game.player2_position()

# Mud status (0 if not in mud, >0 for turns remaining)
mud1 = game.player1_mud_turns
mud2 = game.player2_mud_turns

# Scores
score1 = game.player1_score()
score2 = game.player2_score()

# Cheese locations (returns list of Coordinates)
cheese = game.cheese_locations()

# Game state
is_done = game.is_game_over()
turn = game.turn_count()
```

### Observation Space
Each player receives:
- `player_position`: Current (x,y) coordinates
- `player_mud_turns`: Remaining turns stuck in mud
- `player_score`: Current score
- `opponent_position`: Opponent's (x,y) coordinates
- `opponent_mud_turns`: Opponent's mud status
- `opponent_score`: Opponent's score
- `cheese_matrix`: Binary matrix of cheese locations
- `movement_matrix`: 3D array encoding valid moves and mud costs

## Testing Strategy

- Rust unit tests for core game logic
- Python integration tests for the PettingZoo interface
- Benchmarks for performance-critical paths (game_benchmarks.rs)
- Use `cargo test --lib --no-default-features` and `pytest` separately for each language layer
- Or use `make test-engine` from the repository root for both

## Component Details

### Protocol (`protocol/`)
Text-based stdin/stdout protocol for AI communication. The `pyrat_base` package provides:
- `BaseAI` class - Extend this to implement your AI
- `IOHandler` - Manages command queue and async communication
- `ProtocolState` - Tracks game state from protocol messages
- Example AIs in `pyrat_base/examples/`: `dummy_ai.py`, `random_ai.py`, `greedy_ai.py`

**Key pattern:** Commands arriving during move calculation are re-queued to prevent state desynchronization.

### CLI (`cli/`)
Game runner subprocess manager. Architecture:
- `cli.py` - Entry point and argparse configuration
- `ai_process.py` - Subprocess communication via protocol
- `game_runner.py` - Game loop orchestration, handles AI failures
- `display.py` - Terminal rendering with ANSI colors and Unicode

Command: `pyrat-game bot1.py bot2.py`

**Key pattern:** AI crashes and timeouts default to STAY action to keep game running.
