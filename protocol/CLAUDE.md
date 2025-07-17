# CLAUDE.md - PyRat Protocol

This file provides guidance to Claude Code when working with the PyRat protocol components.

## Component Overview

The protocol directory contains:
- Protocol specification (`spec.md`)
- Python base library (`pyrat_base/`)
- Example AI implementations
- Test suite

## Directory Structure

```
protocol/
├── spec.md              # Protocol specification
├── pyrat_base/         # Python implementation
│   ├── base_ai.py      # PyRatAI base class
│   ├── protocol.py     # Protocol parser/formatter
│   ├── protocol_state.py # Game state wrapper
│   ├── io_handler.py   # Non-blocking I/O
│   ├── utils.py        # Pathfinding utilities
│   ├── examples/       # Example AIs
│   └── tests/          # Unit tests
```

## Setup

From repository root:
```bash
uv sync  # Installs pyrat_base as part of workspace
```

Or standalone:
```bash
cd protocol/pyrat_base
uv venv
source .venv/bin/activate
uv pip install -e .
```

## Testing

```bash
# From repository root
make test-protocol

# Or directly
cd protocol/pyrat_base
pytest tests -v
```

## Implementation Details

### Protocol State Machine
1. INITIAL → HANDSHAKE (on "pyrat")
2. HANDSHAKE → READY (after "pyratready")
3. READY → GAME_INIT (on "maze")
4. GAME_INIT → PLAYING (on "startpreprocessing")
5. PLAYING → GAME_OVER (on "gameover")
6. GAME_OVER → READY (next game)

### Threading
- Main thread: Protocol state machine
- IOHandler thread: Reads stdin continuously
- CalculationThread: Runs get_move() with interrupt support

### Coordinate System
- (0,0) is bottom-left
- UP increases y
- DOWN decreases y
- RIGHT increases x
- LEFT decreases x

### Wall Specification
Walls are tuples of adjacent cells: `((x1, y1), (x2, y2))`
- Same x-coordinate: horizontal wall (blocks UP/DOWN)
- Same y-coordinate: vertical wall (blocks LEFT/RIGHT)

## Common Tasks

### Adding Protocol Commands
1. Update `enums.py` with new CommandType
2. Add parser in `protocol.py`
3. Handle in `base_ai.py` main loop
4. Add tests in `tests/test_protocol.py`

### Creating Example AIs
```python
from pyrat_base import PyRatAI, ProtocolState
from pyrat_engine.game import Direction

class MyAI(PyRatAI):
    def __init__(self):
        super().__init__("MyBot", "Author")

    def get_move(self, state: ProtocolState) -> Direction:
        # AI logic here
        return Direction.STAY
```

### Running Tests
The test suite includes:
- Enums: 20 tests
- Protocol parsing: 58 tests
- IOHandler: 23 tests
- ProtocolState: 13 tests
- Utils: 14 tests

## Key Concepts

### Movement
- No moves are "illegal" in PyRat
- Invalid moves (into walls) default to STAY
- "Effective moves" are those that result in actual movement

### Pathfinding
The utils module provides Dijkstra's algorithm that:
- Accounts for walls (impassable)
- Accounts for mud (increased turn cost)
- Returns optimal paths as Direction lists

### State Management
ProtocolState provides player-perspective view:
- `my_position` / `opponent_position`
- `my_score` / `opponent_score`
- `movement_matrix` with costs for each direction

## Dependencies
- `pyrat-engine`: For game types (Direction, etc.)
- Standard library only for other dependencies

## Known Issues
- Pathfinding is implemented in Python (could be faster in Rust)
- No built-in visualization tools
