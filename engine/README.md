# PyRat Engine
[![Python 3.7+](https://img.shields.io/badge/python-3.7+-blue.svg)](https://www.python.org/downloads/)
[![Rust](https://img.shields.io/badge/rust-1.75+-orange.svg)](https://www.rust-lang.org/)

A high-performance PyRat game engine implementation in Rust with Python bindings, providing a PettingZoo-compatible interface.

> **Note**: This is the engine component of the PyRat ecosystem monorepo. For the full ecosystem overview, see the [main README](../README.md).


## What is PyRat?

PyRat is a two-player maze game where players (a Rat and a Python) compete to collect cheese while navigating through a maze. The game features:
- Simultaneous movement
- Mud tiles that delay movement
- Symmetric or asymmetric mazes
- Shared cheese collection points

For a complete description of the game rules, see [Game Rules](#game-rules).

## Quick Start

### Installation

```bash
pip install pyrat-engine
```

## Usage

### Game Engine

The primary API is the `PyRat` class, which exposes the Rust game engine directly:

```python
from pyrat_engine import PyRat, Direction

# Create a game with default settings (21x15, 41 cheese)
game = PyRat(seed=42)

# Or customize the board
game = PyRat(
    width=31,
    height=21,
    cheese_count=85,
    symmetric=True,
    seed=42,
    max_turns=400,
    wall_density=0.5,   # Proportion of walls (default: 0.7)
    mud_density=0.2,    # Proportion of mud passages (default: 0.1)
)

# Run the game loop
while True:
    game_over, collected = game.step(Direction.RIGHT, Direction.LEFT)
    if game_over:
        break

print(f"Player 1: {game.player1_score}, Player 2: {game.player2_score}")
```

### Reinforcement Learning (PettingZoo)

For RL workflows, use the `PyRatEnv` wrapper which implements the PettingZoo Parallel API:

```python
from pyrat_engine.env import PyRatEnv
from pyrat_engine import Direction

env = PyRatEnv(width=21, height=15, cheese_count=41, seed=42)
observations, info = env.reset(seed=42)

terminated = truncated = False
while not (terminated or truncated):
    actions = {
        "player_1": Direction.RIGHT,
        "player_2": Direction.LEFT,
    }
    observations, rewards, terminations, truncations, infos = env.step(actions)
    terminated = any(terminations.values())
    truncated = any(truncations.values())
```

### Observation Space

Each observation is a dictionary containing:
```python
{
"player_position": (x, y), # Current player position
"player_mud_turns": int, # Turns remaining in mud (0 if not in mud)
"player_score": float, # Current player score
"opponent_position": (x, y), # Opponent position
"opponent_mud_turns": int, # Opponent mud turns
"opponent_score": float, # Opponent score
"current_turn": int, # Current game turn
"max_turns": int, # Maximum turns allowed
"cheese_matrix": np.ndarray, # Binary matrix showing cheese locations
"movement_matrix": np.ndarray # Shape (width, height, 4), see below
}
```

**`movement_matrix` format:** A 3D `int8` array of shape `(width, height, 4)`.
- The third dimension corresponds to directions: `[UP, RIGHT, DOWN, LEFT]`.
- Values:
  - `-1`: Invalid move (wall or out of bounds)
  - `0`: Valid immediate move
  - `N > 0`: Valid move with N turns of mud delay

```python
# Example: check if moving UP from position (3, 5) is valid
obs["movement_matrix"][3, 5, 0]  # 0 = valid, -1 = wall, N>0 = mud
```

## Game Rules

[Include a condensed version of the game rules from pyrat-game-specifications.txt]

## Technical Details

### Architecture

This implementation features:
- Core game engine written in Rust for maximum performance
- Python bindings using PyO3
- Numpy-based observation spaces
- Support for move undo/redo for efficient game tree exploration

### Performance

The Rust engine achieves exceptional performance:
- 10+ million moves/second on standard configurations
- Efficient memory usage through optimized data structures
- Zero-cost abstractions for core game mechanics

## Development

### Building from Source

Prerequisites:
- Python 3.7+
- Rust toolchain
- uv (for fast Python package management)

```bash
# Clone repository
git clone https://github.com/mintiti/pyrat-engine
cd pyrat-engine

# Install uv (if not already installed)
curl -LsSf https://astral.sh/uv/install.sh | sh

# Install dependencies and build
uv sync
uv run maturin develop --release
```

### Running Tests
```bash
# Run Python tests
pytest python/tests

# Run Rust tests (without Python features to avoid linking issues)
cargo test --lib --no-default-features
```
