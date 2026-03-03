# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

PyRat is a monorepo containing the complete PyRat ecosystem for a competitive maze game. The repository is organized into multiple components:

- **engine/**: Rust game engine with PyO3 bindings - core game logic and Python API
- **host/**: Match hosting library — setup, turn loop, event streaming (Rust, `pyrat-host` crate)
- **headless/**: Headless match runner binary — launches bots, runs a match, outputs JSON (`pyrat-headless` crate)
- **wire/**: FlatBuffers schema and generated types, shared by host and SDKs (`pyrat-wire` crate)
- **sdk-rust/**: Rust bot SDK (`pyrat-sdk` crate)
- **sdk-python/**: Python bot SDK (`pyrat_sdk` package)

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
# - Installs all workspace members (engine, sdk-python)
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

# Rust checks (all run from repo root — Cargo workspace is at root)
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings -A non-local-definitions
cargo clippy -p pyrat-rust --all-targets --no-default-features -- -D warnings

# Run Python tests
cd engine && uv run pytest python/tests -v
```

### Building and Testing
```bash
# From repository root
make test          # Run all tests
make test-engine   # Run engine tests only
make test-wire     # Run wire protocol tests
make test-host     # Run host library tests
make test-headless # Run headless runner tests
make test-sdk-python # Run SDK Python tests
make bench         # Run benchmarks

# Rust commands (from repo root — Cargo workspace is at root)
cargo build -p pyrat-rust --release
cargo test -p pyrat-rust --lib --no-default-features
cargo test -p pyrat-wire
cargo test -p pyrat-host
cargo test -p pyrat-headless
cargo test -p pyrat-sdk
cargo bench -p pyrat-rust --bench game_benchmarks

# Build Python package with maturin
cd engine && uv run maturin develop --release
```

### Running Games
```bash
# Run a headless match between two Rust bots
cargo run -p pyrat-headless -- \
    "cargo run -p pyrat-sdk --example greedy" \
    "cargo run -p pyrat-sdk --example random"

# Run a match with Python bots
cargo run -p pyrat-headless -- \
    "uv run python sdk-python/examples/greedy.py" \
    "uv run python sdk-python/examples/smart_random.py"
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
Key properties and methods on `PyRat`:

```python
# Player positions (returns Coordinates)
pos1 = game.player1_position
pos2 = game.player2_position

# Mud status (0 if not in mud, >0 for turns remaining)
mud1 = game.player1_mud_turns
mud2 = game.player2_mud_turns

# Scores
score1 = game.player1_score
score2 = game.player2_score

# Cheese locations (returns list of Coordinates)
cheese = game.cheese_positions()

# Game state
turn = game.turn
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
- `cargo test -p pyrat-rust --lib --no-default-features` and `pytest` separately for each language layer
- Or use `make test-engine` from the repository root for both

## Component Details

### Host Library (`host/`)
Match hosting library. Manages bot connections, setup handshake, and the turn loop:
- `game_loop/` — Setup phases (connect → identify → configure → preprocess), playing loop, event streaming
- `session/` — Per-connection state machine, FlatBuffers wire codec
- `wire/` — FlatBuffers schema types re-exported for consumers
- `MatchEvent` — Event stream consumed by headless runner, GUI, or tournament systems

**Key pattern:** The host is a pipe — it streams `MatchEvent`s through a channel. Consumers decide what to record or display.

### Headless Runner (`headless/`)
CLI binary that launches bot subprocesses, runs a match via the host library, and optionally writes a JSON game record:
- `main.rs` — CLI parsing, bot launch, match orchestration, JSON output

Command: `cargo run -p pyrat-headless -- bot1_cmd bot2_cmd`

### SDKs

**Rust SDK (`sdk-rust/`):**
Bot SDK for writing Rust bots. Provides trait-based bot interface with FlatBuffers wire protocol.
- Examples: `cargo run -p pyrat-sdk --example greedy`

**Python SDK (`sdk-python/`):**
Bot SDK for writing Python bots. Uses PyO3/maturin for engine bindings.
- Examples: `uv run python sdk-python/examples/greedy.py`
