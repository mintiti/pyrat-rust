# PyRat Engine

The game engine behind PyRat — a Rust implementation with Python bindings. Use it for reinforcement learning (PettingZoo), game tree search (MCTS, minimax), or embed it directly as a Rust crate.

Part of the [PyRat ecosystem](../README.md). If you're looking to write a bot, see the [SDKs](../sdk/) instead.

## Installation

Not published to PyPI or crates.io — install from source. Both paths need the [Rust toolchain](https://rustup.rs/).

### Python

Prerequisites: Python 3.8+, [uv](https://docs.astral.sh/uv/)

```bash
uv pip install "pyrat-engine @ git+https://github.com/mintiti/pyrat-rust.git#subdirectory=engine"
```

Or from a local clone:

```bash
git clone https://github.com/mintiti/pyrat-rust.git
cd pyrat-rust
uv pip install ./engine
```

```python
from pyrat_engine import GameConfig, Direction
```

### Rust

```toml
[dependencies]
pyrat = { git = "https://github.com/mintiti/pyrat-rust.git", package = "pyrat-rust", default-features = false }
```

```rust
use pyrat::{GameConfig, Direction};
```

## Creating games

All use cases start with `GameConfig` — a reusable recipe that stamps out game instances. Call `config.create(seed)` with different seeds to get different boards from the same settings.

### Presets

```python
from pyrat_engine import GameConfig

config = GameConfig.preset("large")
game = config.create(seed=42)
```

| Preset | Board | Cheese | Turns | Maze |
|--------|-------|--------|-------|------|
| `tiny` | 11x9 | 13 | 150 | classic |
| `small` | 15x11 | 21 | 200 | classic |
| `medium` | 21x15 | 41 | 300 | classic |
| `large` | 31x21 | 85 | 400 | classic |
| `huge` | 41x31 | 165 | 500 | classic |
| `open` | 21x15 | 41 | 300 | no walls, no mud |
| `asymmetric` | 21x15 | 41 | 300 | classic, no symmetry |

**Classic** maze: 0.7 wall density, 0.1 mud density, connected, 180° symmetric.

### Custom dimensions

```python
config = GameConfig.classic(21, 15, 41)  # width, height, cheese count
```

Uses classic maze settings with corner starting positions.

### Full control

```python
from pyrat_engine import GameBuilder

config = (GameBuilder(21, 15)
    .with_classic_maze()          # or with_open_maze(), with_random_maze(), with_custom_maze()
    .with_corner_positions()      # or with_random_positions(), with_custom_positions()
    .with_random_cheese(41)       # or with_custom_cheese()
    .build())
```

The Rust API is the same shape, with compile-time enforcement of the build sequence (maze → players → cheese):

```rust
use pyrat::{GameBuilder, GameConfig};

// Preset
let config = GameConfig::preset("large")?;

// Builder
let config = GameBuilder::new(21, 15)
    .with_classic_maze()
    .with_corner_positions()
    .with_random_cheese(41, true)
    .build();

let game = config.create(Some(42))?;
```

### Reuse for training

`GameConfig` is designed to be called repeatedly with different seeds — one config, many games:

```python
config = GameConfig.preset("medium")
for episode in range(10_000):
    game = config.create(seed=episode)
    # ... run episode
```

## RL training (PettingZoo)

The engine provides a [PettingZoo](https://pettingzoo.farama.org/) `ParallelEnv` for multi-agent RL.

```python
from pyrat_engine import GameConfig, Direction
from pyrat_engine.env import PyRatEnv

config = GameConfig.classic(15, 15, 21)
env = PyRatEnv(config)
obs, info = env.reset(seed=42)

while True:
    actions = {
        "player_1": Direction.RIGHT,  # your policy here
        "player_2": Direction.LEFT,
    }
    obs, rewards, terminations, truncations, infos = env.step(actions)
    if any(terminations.values()):
        break
```

Actions are integers 0–4: `UP=0, RIGHT=1, DOWN=2, LEFT=3, STAY=4`. `Direction` members are `IntEnum`, so they work directly.

### Observation space

Each agent receives a `Dict` observation:

| Key | Shape | Dtype | Description |
|-----|-------|-------|-------------|
| `player_position` | `(2,)` | `uint8` | `(x, y)` coordinates |
| `player_mud_turns` | `(1,)` | `uint8` | Turns remaining in mud (0 = free) |
| `player_score` | `(1,)` | `float32` | Current score |
| `opponent_position` | `(2,)` | `uint8` | Opponent's `(x, y)` |
| `opponent_mud_turns` | `(1,)` | `uint8` | Opponent's mud turns |
| `opponent_score` | `(1,)` | `float32` | Opponent's score |
| `current_turn` | `(1,)` | `uint16` | Current turn number |
| `max_turns` | `(1,)` | `uint16` | Turn limit |
| `cheese_matrix` | `(w, h)` | `uint8` | 1 where cheese exists, 0 otherwise |
| `movement_matrix` | `(w, h, 4)` | `int8` | Move costs per direction (see below) |

**`movement_matrix`** encodes what happens when you move from each cell. The last axis is `[UP, RIGHT, DOWN, LEFT]`:
- `-1` — wall or boundary (can't move)
- `0` — open passage
- `N >= 2` — mud, costs N turns to cross

```python
obs["movement_matrix"][3, 5, 0]  # cost of moving UP from (3, 5)
```

The movement matrix is computed once at game creation (walls and mud don't change during a game).

### Rewards

Zero-sum: each agent's reward is their score change minus the opponent's.

```
reward_p1 = delta_p1_score - delta_p2_score
reward_p2 = delta_p2_score - delta_p1_score
```

Both agents terminate simultaneously when the game ends. The turn limit fires as a termination (not truncation).

## Direct game control

For custom game loops and tree search. The Python API uses the `PyRat` class, the Rust API uses `GameState` directly.

### Game loop

```python
from pyrat_engine import GameConfig, Direction

game = GameConfig.classic(21, 15, 41).create(seed=42)

while True:
    game_over, collected = game.step(Direction.RIGHT, Direction.LEFT)
    if game_over:
        break

print(f"P1: {game.player1_score}, P2: {game.player2_score}")
```

```rust
use pyrat::{GameConfig, Direction};

let config = GameConfig::classic(21, 15, 41);
let mut game = config.create(Some(42)).unwrap();

loop {
    let result = game.process_turn(Direction::Right, Direction::Left);
    if result.game_over {
        break;
    }
}
println!("P1: {}, P2: {}", game.player1_score(), game.player2_score());
```

### Inspecting state

Both `PyRat` (Python) and `GameState` (Rust) expose the same information:

- **Positions** — `player1_position`, `player2_position` (returns `Coordinates` with `.x`, `.y`)
- **Scores** — `player1_score`, `player2_score`
- **Mud status** — `player1_mud_turns`, `player2_mud_turns` (0 = not in mud)
- **Cheese** — `cheese_positions()` returns current cheese locations
- **Walls** — `wall_entries()` returns all walls
- **Mud passages** — `mud_entries()` (Python) / `mud_positions()` (Rust)
- **Valid moves** — `effective_actions(pos)` returns `[u8; 5]` where blocked directions map to STAY

### Tree search

`make_move` / `unmake_move` lets you explore the game tree without cloning state. Undo objects must be applied in LIFO order.

```python
# Explore a branch
undo = game.make_move(Direction.RIGHT, Direction.LEFT)
score = evaluate(game)  # game state reflects the move
game.unmake_move(undo)  # game state is restored
```

Use `effective_actions_p1()` / `effective_actions_p2()` for move generation — they account for mud (all actions map to STAY when stuck).

## Rust crate

### Feature flags

| Feature | Default | Description |
|---------|---------|-------------|
| `python` | yes | PyO3 bindings. Disable with `default-features = false` for pure Rust. |

### Key types

All re-exported from the crate root (`use pyrat::*`):

| Type | Description |
|------|-------------|
| `GameState` | Game state — positions, scores, cheese, turn counter |
| `GameConfig` | Reusable game recipe, stamps out `GameState` via `create()` |
| `GameBuilder` | Typestate builder for `GameConfig` |
| `MazeParams` | Maze generation knobs (wall density, mud density, symmetry) |
| `Direction` | Movement enum: `Up`, `Down`, `Left`, `Right`, `Stay` |
| `Coordinates` | `(x, y)` board position |
| `MoveTable` | O(1) bitwise wall/boundary lookup |
| `CheeseBoard` | Bitboard for cheese positions |
| `MoveUndo` | Undo token for `make_move` / `unmake_move` |
| `PlayerState` | Player position, score, mud timer |
| `Wall` | Wall between two adjacent cells |
| `Mud` | Mud passage with traversal cost |
| `MudMap` | Bidirectional mud lookup |

### Performance

The engine processes 10+ million moves per second on standard configurations (benchmarked with criterion on an M-series Mac). Run `cargo bench -p pyrat-rust --bench game_benchmarks` to measure on your hardware.

## Game rules

### Setup

- **Grid**: Rectangular maze (default 21x15) with walls between cells
- **Players**: Player 1 starts at (0, 0), player 2 at (width-1, height-1)
- **Cheese**: Randomly placed, symmetric by default (180° rotation)
- **Maze**: Fully connected — there's always a path between any two cells

### Movement

- **Actions**: UP, DOWN, LEFT, RIGHT, or STAY
- **Invalid moves** (into walls or boundaries) count as STAY
- **Simultaneous**: Both players move at the same time
- **No blocking**: Players can occupy the same cell

### Mud

Some passages have mud with a cost N (number of turns to cross):

1. **Commit**: Player chooses a direction, stays in the starting cell
2. **Stuck**: For turns 2 through N-1, all actions are ignored. Player can't collect cheese.
3. **Arrive**: On turn N, player arrives at the destination

### Scoring

- **Normal**: 1 point when landing on a cheese
- **Simultaneous**: 0.5 points each if both players land on the same cheese on the same turn
- **Mud blocks collection**: Can't collect cheese while stuck in mud

### Game over

The game ends immediately when any of these is true:

1. A player scores more than half the total cheese
2. All cheese is collected
3. Turn limit reached (default 300)

Winner is the player with the higher score. Draws are possible.

## Development

```bash
# Build the Rust extension
cd engine && uv run maturin develop --release

# Run tests
make test-engine                                          # both Rust and Python
cargo test -p pyrat-rust --lib --no-default-features      # Rust only
cd engine && uv run pytest python/tests -v                # Python only
```

Run `make help` from the repository root for the full command list.
