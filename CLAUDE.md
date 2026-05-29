# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

PyRat is a monorepo containing the complete PyRat ecosystem for a competitive maze game. The repository is organized into multiple components:

- **engine/**: Rust game engine with PyO3 bindings - core game logic and Python API
- **server/host/**: Match hosting library — setup, turn loop, event streaming (Rust, `pyrat-host` crate)
- **server/wire/**: FlatBuffers schema and generated types, shared by host and SDKs (`pyrat-wire` crate)
- **server/schema/**: FlatBuffers schema source and codegen script
- **sdk/rust/**: Rust bot SDK (`pyrat-sdk` crate)
- **sdk/python/**: Python bot SDK (`pyrat_sdk` package)
- **eval/orchestrator/**: Concurrent match executor (`pyrat-orchestrator` crate)
- **eval/store/**: SQLite-backed result store + Elo computation (`pyrat-eval-store` crate)
- **eval/session/**: Eval session crate + `pyrat-eval` CLI (`run-one` for single matches, `tournament run` for round-robin / gauntlet with Elo standings)

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
# - Installs all workspace members (engine, sdk/python)
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
make test-eval-cli # Run pyrat-eval CLI tests
make test-sdk-python  # Run SDK Python tests
make bench            # Run benchmarks

# Rust commands (from repo root — Cargo workspace is at root)
cargo build -p pyrat-rust --release
cargo test -p pyrat-rust --lib --no-default-features
cargo test -p pyrat-wire
cargo test -p pyrat-host
cargo test -p pyrat-eval
cargo test -p pyrat-sdk
cargo bench -p pyrat-rust --bench game_benchmarks

# Build Python package with maturin
cd engine && uv run maturin develop --release
```

### Running Games
```bash
# Run a single match between two Rust bots
cargo run -p pyrat-eval -- run-one \
    "cd botpack/greedy && cargo run --release" \
    "cd botpack/smart-random && cargo run --release"

# Run a match with Python bots
cargo run -p pyrat-eval -- run-one \
    "cd botpack/greedy-py && uv run python bot.py" \
    "cd botpack/smart-random-py && uv run python bot.py"

# Run a tournament (round-robin) from flags
cargo run -p pyrat-eval -- tournament run \
    --bot greedy=botpack/greedy \
    --bot smart_random=botpack/smart-random \
    --format round-robin --games 5

# Run a tournament from a TOML config (committed spec)
cargo run -p pyrat-eval -- tournament run --config ladder.toml

# Materialize a flag-driven tournament to TOML for reuse / source control
cargo run -p pyrat-eval -- tournament run --bot a=path --bot b=path --save-as spec.toml

# Resume an existing tournament by id (mutually exclusive with --save-as)
cargo run -p pyrat-eval -- tournament run --config ladder.toml --resume 7
```

The `tournament run` subcommand is library-first: the `pyrat-eval` crate exports `EvalSession`, `RoundRobinPlanner`, `GauntletPlanner`, etc., for GUI / alpharat / other Rust consumers. The CLI is an automation surface over it.

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

### Host Library (`server/host/`)
Match hosting library. Manages bot connections, setup handshake, and the turn loop:
- `match_host/` — `Match<S>` typestate (Created → Ready → Playing → Finished, with Thinking/Collected analysis sub-states), `FaultPolicy`, `MatchEvent`, `MatchError`
- `player/` — `Player` trait, `EmbeddedPlayer` (in-process bot), `TcpPlayer` + `accept_players` (concurrent agent_id-keyed handshake)
- `launch.rs` — Bot subprocess launching with RAII cleanup (`BotProcesses`)
- `match_config.rs` — Match config + builder
- `probe.rs` — Single-bot probe for GUI/bot-check
- `snapshot.rs` — Engine state reconstruction from `(MatchConfig, TurnState)` (private, used by `start_turn_with` and `EmbeddedPlayer`)
- `pub use pyrat_wire as wire` — Re-exports FlatBuffers schema types from `server/wire/` for consumers

**Key pattern:** The host is a pipe — it streams `MatchEvent`s through a channel. Consumers decide what to record or display.

### Eval CLI (`eval/session/`)
Single binary `pyrat-eval` with subcommands:
- `run-one bot1_cmd bot2_cmd`: launches two bot subprocesses, runs a match via the orchestrator + host library, and optionally writes a legacy-shape JSON game record. Replaced the former `pyrat-headless` crate.
- `tournament run`: round-robin or gauntlet between N bots. Library-first design — the CLI is a surface over `EvalSession`. Builds a `TournamentSpec` from flags (`--bot id=working_dir`) or a TOML config (`--config foo.toml`), resolves precedence (defaults → config → flags), and drives the session to completion. Supports `--save-as` (materialize the resolved spec for source control), `--resume <id>` (continue an aborted tournament; reuses stored seed and game_config_id), `--results-json` (Level-A summary), and `--replay-dir` (per-match forensic JSON via `ReplaySink`).

### SDKs

**Rust SDK (`sdk/rust/`):**
Bot SDK for writing Rust bots. Provides trait-based bot interface with FlatBuffers wire protocol.
- Example bots: `botpack/` (e.g. `cd botpack/greedy && cargo run --release`)

**Python SDK (`sdk/python/`):**
Bot SDK for writing Python bots. Uses PyO3/maturin for engine bindings.
- Example bots: `botpack/` (e.g. `cd botpack/greedy-py && uv run python bot.py`)
