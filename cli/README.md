# PyRat Game Runner

Command-line tool to run and visualize PyRat games between two AI processes.

## Installation

From the repository root:

```bash
# Sync all workspace dependencies
uv sync --all-extras

# Activate the virtual environment
source .venv/bin/activate
```

The `pyrat-game` command will be available in your virtual environment.

## Basic Usage

```bash
# Run a game between two AIs
pyrat-game path/to/rat_ai.py path/to/python_ai.py

# Example with the provided example AIs
pyrat-game protocol/pyrat_base/pyrat_base/examples/random_ai.py \
           protocol/pyrat_base/pyrat_base/examples/greedy_ai.py
```

## Command-Line Options

### Required Arguments

- `rat_ai` - Path to Rat AI script (starts at top-right corner)
- `python_ai` - Path to Python AI script (starts at bottom-left corner)

### Maze Configuration

- `--width WIDTH` - Maze width (default: 21)
- `--height HEIGHT` - Maze height (default: 15)
- `--cheese COUNT` - Number of cheese pieces (default: 41)
- `--seed SEED` - Random seed for reproducibility (default: random)

### Time Controls

- `--timeout SECONDS` - AI response timeout in seconds (default: 1.0)
- `--preprocessing SECONDS` - Preprocessing time in seconds (default: 3.0)

### Display Options

- `--delay SECONDS` - Delay between turns for visualization (default: 0.3)

## Examples

### Run with Custom Maze Size

```bash
pyrat-game --width 31 --height 21 --cheese 85 bot1.py bot2.py
```

### Run with Longer Timeouts

```bash
pyrat-game --timeout 2.0 --preprocessing 5.0 bot1.py bot2.py
```

### Run with Faster Display

```bash
pyrat-game --delay 0.1 bot1.py bot2.py
```

### Run with Specific Seed for Reproducibility

```bash
pyrat-game --seed 42 bot1.py bot2.py
```

## Game Display

The terminal display shows:

- **Board**: ASCII representation of the maze
  - `R` - Rat player
  - `P` - Python player
  - `*` - Cheese
  - Walls shown as `|` and `+---+`
- **Turn Number**: Current turn count
- **Scores**: Current score for each player
- **Last Moves**: Last move made by each player

## Error Handling

The game runner handles:

- **AI Timeouts**: If an AI doesn't respond within the timeout, it defaults to STAY
- **AI Crashes**: If an AI process crashes, the game ends and an error is displayed
- **Invalid Moves**: Invalid moves are converted to STAY

## Creating Your Own AI

See the example AIs in `protocol/pyrat_base/pyrat_base/examples/`:

- `dummy_ai.py` - Always stays in place
- `random_ai.py` - Makes random valid moves
- `greedy_ai.py` - Uses pathfinding to collect nearest cheese

To create your own AI, extend the `PyRatAI` base class:

```python
from pyrat_base import PyRatAI, ProtocolState
from pyrat_engine.core import Direction

class MyAI(PyRatAI):
    def __init__(self):
        super().__init__("MyBot v1.0", "Your Name")

    def get_move(self, state: ProtocolState) -> Direction:
        # Your strategy here
        return Direction.UP

if __name__ == "__main__":
    ai = MyAI()
    ai.run()
```

Then run your AI:

```bash
pyrat-game my_ai.py protocol/pyrat_base/pyrat_base/examples/greedy_ai.py
```

## Protocol Specification

The game runner implements the PyRat protocol specification (see `protocol/spec.md`). Key features:

- Text-based stdin/stdout communication
- Preprocessing phase for maze analysis
- Turn-based move requests
- Info messages for debugging
- Graceful error recovery

## Performance

The underlying game engine is implemented in Rust and can execute 10+ million moves per second. The visualization delay (`--delay`) is the primary factor in game execution time.
