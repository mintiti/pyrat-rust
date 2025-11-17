# PyRat Protocol

This directory contains the PyRat Communication Protocol specification and implementations.

## Contents

- **`spec.md`** - The official PyRat Communication Protocol specification (v1.0)
  - Defines how PyRat engines and AI players communicate
  - Text-based protocol inspired by UCI (Universal Chess Interface)
  - Language-agnostic design allowing AI development in any programming language

- **`pyrat_base/`** - Python base library for protocol-compliant AIs
  - Implementation of the PyRat protocol
  - Base class for AI development
  - Pathfinding utilities (Dijkstra's algorithm)
  - Non-blocking I/O with interrupt support
  - Test suite (126 tests)

## Protocol Overview

The PyRat Communication Protocol enables:
- Language-independent AI development
- Process isolation for stability and parallelism
- Robust error handling and recovery
- Tournament automation
- Real-time progress monitoring

## Key Features

- **Handshake & Initialization**: Standard connection sequence
- **Game Phases**: Preprocessing, gameplay, and postprocessing
- **Time Management**: Configurable time limits for each phase
- **Info Messages**: Optional progress reporting during calculation
- **Interrupt Support**: Stop command for immediate response
- **Options System**: Configurable AI parameters

## Python Base Library (pyrat_base)

The `pyrat_base` package provides everything needed to build PyRat AIs in Python:

### Installation
```bash
# From repository root (recommended - uses workspace)
uv sync

# Or standalone
cd protocol/pyrat_base
uv venv
source .venv/bin/activate
uv pip install -e .
```

### Quick Start
```python
from pyrat_base import PyRatAI, ProtocolState
from pyrat_engine.core import Direction

class MyAI(PyRatAI):
    def __init__(self):
        super().__init__("MyBot", "Your Name")

    def get_move(self, state: ProtocolState) -> Direction:
        # Your AI logic here
        return Direction.STAY

if __name__ == "__main__":
    ai = MyAI()
    ai.run()
```

### Features
- **Protocol Handling**: Management of protocol messages
- **State Management**: Player-perspective view of game state
- **Pathfinding**: Dijkstra's algorithm accounting for walls and mud
- **Non-blocking I/O**: Handles stop commands during computation
- **Example AIs**: Three examples (dummy, random, greedy)

### Example AIs

1. **`dummy_ai.py`** - Always returns STAY
2. **`random_ai.py`** - Chooses random effective moves
3. **`greedy_ai.py`** - Uses Dijkstra pathfinding to reach nearest cheese

## For AI Developers

### Python Developers
1. Install the `pyrat_base` package
2. Inherit from `PyRatAI` and implement `get_move()`
3. See example AIs in `pyrat_base/examples/` for patterns

### Other Languages
Read `spec.md` and implement the protocol directly. The protocol is designed to be simple to implement in any language with standard I/O support.

## Testing

```bash
# Run all protocol tests
make test-protocol

# Or directly
cd protocol/pyrat_base
pytest tests -v
```
