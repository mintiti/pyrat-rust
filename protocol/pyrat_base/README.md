# PyRat Base Library

Base library for developing PyRat AIs that communicate via the PyRat protocol.

## Overview

The PyRat Base Library provides a foundation for building AIs that play PyRat. It handles:
- Protocol communication (text-based stdin/stdout)
- Game state management
- Non-blocking I/O for interrupt handling
- Common utilities for pathfinding and strategy

## Installation

### For AI Development
```bash
pip install pyrat-base
```

### For Development on the Library
```bash
# From repository root (recommended - uses workspace)
uv sync

# Or standalone
cd protocol/pyrat_base
uv sync
```

## Quick Start

Create your AI by extending the `PyRatAI` base class:

```python
from pyrat_base import PyRatAI, Direction

class MyAI(PyRatAI):
    def calculate_move(self, state):
        # Your AI logic here
        # state contains game information from AI's perspective
        return Direction.UP

if __name__ == "__main__":
    ai = MyAI()
    ai.run()
```

## Features

- **Protocol Handling**: Automatically manages handshake, game initialization, and turn communication
- **State Management**: Maintains game state with AI's perspective (my_position vs opponent_position)
- **Interrupt Support**: Handles stop commands during computation
- **Options System**: Support for configurable AI parameters
- **Debug Mode**: Built-in support for debug output via `info` messages

## Development

```bash
# Run tests
pytest tests -v

# Format code
ruff format pyrat_base

# Lint code
ruff check pyrat_base

# Type check
mypy pyrat_base
```

## Architecture

The library is organized into modules:
- `enums.py` - Protocol constants and enumerations
- `protocol.py` - Message parsing and formatting
- `protocol_state.py` - Game state from AI perspective
- `io_handler.py` - Non-blocking I/O management
- `base_ai.py` - Base class for AI implementation
- `utils.py` - Common utilities (pathfinding, etc.)

## Protocol Specification

See [protocol specification](../spec.md) for complete details on the communication protocol.
