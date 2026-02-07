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

### Setup
- **Grid**: Rectangular maze (default 21×15) with walls between cells
- **Players**: Rat starts at (0, 0), Python starts at (width-1, height-1)
- **Cheese**: Randomly placed on cells, symmetric by default
- **Maze**: Fully connected — there's always a path between any two cells

### Movement
- **Actions**: UP, DOWN, LEFT, RIGHT, or STAY
- **Invalid moves** (into walls or boundaries) count as STAY
- **Simultaneous**: Both players move at the same time
- **Collision**: Players can occupy the same cell (no blocking)

### Mud
- Some passages have mud with a value N (number of turns to cross)
- Turn 1: Player commits to a direction, stays in starting cell
- Turns 2 to N-1: Player is stuck, cannot collect cheese, all actions ignored
- Turn N: Player arrives at destination

### Scoring
- **Normal collection**: 1 point when landing on a cheese
- **Simultaneous collection**: 0.5 points each if both players land on the same cheese
- **Cannot collect while stuck in mud**

### Game Over
The game ends immediately when:
1. A player scores more than half the total cheese count
2. All cheese is collected
3. Maximum turns (default 300) reached

Winner has the higher score. Draws are possible.

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

## Using as a Rust Crate

The engine can be used as a standalone Rust library without Python. Disable the default `python` feature to avoid the PyO3 dependency:

```toml
[dependencies]
pyrat = { path = "engine", default-features = false }
```

### Minimal game loop

```rust
use pyrat::{GameState, Direction};

fn main() {
    let mut game = GameState::new_symmetric(
        Some(21), Some(15), Some(41), Some(42), None, None,
    );

    loop {
        let result = game.process_turn(Direction::Right, Direction::Left);
        if result.game_over {
            break;
        }
    }

    println!(
        "P1: {}, P2: {}",
        game.player1_score(),
        game.player2_score()
    );
}
```

### Key re-exported types

| Type | Description |
|------|-------------|
| `GameState` | Core game state and logic |
| `Direction` | Movement enum: Up, Down, Left, Right, Stay |
| `Coordinates` | (x, y) board position |
| `MoveTable` | Precomputed valid-move lookup table |
| `CheeseBoard` | Bitboard tracking cheese positions |
| `MazeConfig` | Maze generation parameters |
| `CheeseConfig` | Cheese placement parameters |
| `Wall` | Wall between two adjacent cells |
| `Mud` | Mud passage with traversal cost |

See `rust/src/lib.rs` for the full list of re-exports.

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
