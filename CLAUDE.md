# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

PyRat Engine is a high-performance game engine implementation in Rust with Python bindings. It provides a PettingZoo-compatible interface for the PyRat maze game where two players compete to collect cheese.

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

### Environment Setup
```bash
# Install uv (if not already installed)
curl -LsSf https://astral.sh/uv/install.sh | sh

# Create Python virtual environment
uv venv
source .venv/bin/activate  # On Windows: .venv\Scripts\activate

# Install Python dependencies
uv pip install -e ".[dev]"

# Build and install the Rust extension
maturin develop --release

# Install pre-commit hooks (automatic formatting and linting)
pre-commit install
pre-commit install --hook-type pre-push
```

### Code Quality Checks
```bash
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
# Build the Rust library
cargo build --release

# Run Rust tests (must be in rust/ directory)
cd rust && cargo test --lib

# Run benchmarks
cargo bench

# Build Python package with maturin
maturin build --release
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
- Python dependencies are in `pyproject.toml` (not requirements.txt)
- Use `maturin develop` to build the Rust extension during development
- To manually run all pre-commit checks: `pre-commit run --all-files`

## Architecture

The codebase follows a hybrid Rust-Python architecture:

### Core Components
1. **Rust Engine** (`rust/src/`)
   - `game/`: Core game logic (board.rs, game_logic.rs, maze_generation.rs)
   - `bindings/`: PyO3 bindings exposing Rust to Python
   - Performance-critical operations: 10+ million moves/second

2. **Python Wrapper** (`python/pyrat/`)
   - `env.py`: PettingZoo ParallelEnv implementation
   - `game.py`: High-level game interface
   - Provides gymnasium/PettingZoo compatible API

### Key Design Patterns
- The Rust `PyGameState` maintains all game state and logic
- Python `PyRatEnv` wraps the Rust game for RL framework compatibility
- Observations are computed in Rust and converted to numpy arrays
- Zero-sum reward calculation happens in the Python layer

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
- Use `cargo test` and `pytest` separately for each language layer
