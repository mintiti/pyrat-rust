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

The engine implements a PettingZoo Parallel environment interface. Here's how to use it:

   ```python
from pyrat_engine import PyRatEnv, Direction
# Create environment
env = PyRatEnv(
width=21, # Default maze width
height=15, # Default maze height
cheese_count=41, # Number of cheese pieces
symmetric=True, # Whether maze should be symmetric
seed=42 # Random seed for reproducibility
)
# Reset environment
observations, info = env.reset(seed=42)
# Environment follows PettingZoo Parallel API
terminated = truncated = False
while not (terminated or truncated):
# Make moves for both players
actions = {
"player_1": Direction.RIGHT, # Available: UP, RIGHT, DOWN, LEFT, STAY
"player_2": Direction.LEFT
}
observations, rewards, terminations, truncations, infos = env.step(actions)
terminated = any(terminations.values())
truncated = any(truncations.values())
# Access final scores
final_scores = {# TODO : This is a wrong reward, need to change that
"player_1": rewards["player_1"],
"player_2": rewards["player_2"]
}
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
"movement_matrix": np.ndarray # Matrix encoding valid moves
}
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

# Create virtual environment
uv venv

# Activate virtual environment
source .venv/bin/activate  # On Windows: .venv\Scripts\activate

# Install development dependencies from pyproject.toml
uv pip install -e ".[dev]"

# Build and install the Rust extension
maturin develop --release
```

### Running Tests
```bash
# Run Python tests
pytest python/tests

# Run Rust tests (without Python features to avoid linking issues)
cargo test --lib --no-default-features
```
