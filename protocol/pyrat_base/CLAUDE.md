# CLAUDE.md - PyRat Base Library

This file provides guidance to Claude Code when working with the PyRat base library (protocol SDK).

## Component Overview

The PyRat Base Library provides a complete SDK for building PyRat AIs that communicate via the UCI-like protocol. It handles all protocol complexity so AI developers can focus on strategy.

### What's Included
- **PyRatAI base class**: Inherit this to create your AI
- **Protocol handling**: Automatic parsing and formatting of all 25+ protocol commands
- **Game state management**: Player-perspective view of the game
- **Non-blocking I/O**: Handles stdin/stdout with move interruption support
- **Pathfinding utilities**: Dijkstra-based algorithms that account for walls and mud
- **Replay system**: Read/write/analyze games in PyRat Replay Format (PRF)

## Quick Development Guide

### Setup (Workspace)
```bash
# From repository root
uv sync  # Installs all workspace dependencies
```

### Testing
```bash
# From protocol/pyrat_base directory
pytest tests -v

# Or from repo root
make test-protocol
```

### Key Files
- `pyrat_base/enums.py` - Protocol enums and constants
- `pyrat_base/protocol.py` - Protocol parsing/formatting
- `pyrat_base/protocol_state.py` - Game state wrapper
- `pyrat_base/io_handler.py` - Non-blocking I/O
- `pyrat_base/base_ai.py` - Base class for AI development
- `pyrat_base/utils.py` - Pathfinding and utilities
- `pyrat_base/replay.py` - Replay system (PRF format)

## Package Details
- **Package name**: `pyrat-base`
- **Import**: `from pyrat_base import PyRatAI`
- **Dependencies**: `pyrat-engine` (for game types)

## Implementation Guidelines

### Protocol Compliance
- Always follow the protocol specification in `../spec.md`
- Handle all required commands
- Ignore unknown commands gracefully
- Never crash on invalid input

### State Management
- AIs are stateful - maintain game state between turns
- Update state from move broadcasts
- Handle recovery after restart

### I/O Handling
- Must read stdin continuously (even while computing)
- Implement non-blocking I/O for interrupt handling
- Always respond to `isready` immediately

## Common Tasks

### Adding a new protocol command
1. Update enums with new command type
2. Add parser in protocol.py
3. Handle in base_ai.py main loop
4. Write tests for parsing/handling

### Testing protocol compliance
- Use mock stdin/stdout in tests
- Test full protocol sequences
- Verify timeout handling
- Test error recovery

## Creating an AI

### Basic Example
```python
from pyrat_base import PyRatAI, ProtocolState
from pyrat_engine.game import Direction

class MyAI(PyRatAI):
    def __init__(self):
        super().__init__(name="MyBot v1.0", author="Your Name")

    def get_move(self, state: ProtocolState) -> Direction:
        # Your strategy here
        if state.cheese:
            # Move toward nearest cheese
            return self.move_to_nearest_cheese(state)
        return Direction.STAY

if __name__ == "__main__":
    ai = MyAI()
    ai.run()  # Handles all protocol communication
```

### Using Pathfinding Utilities
```python
from pyrat_base.utils import find_nearest_cheese_by_time, get_direction_toward_target

def get_move(self, state: ProtocolState) -> Direction:
    # Find best cheese accounting for walls and mud
    target = find_nearest_cheese_by_time(
        state.my_position,
        state.cheese,
        state.movement_matrix
    )

    if target:
        # Get optimal direction accounting for obstacles
        return get_direction_toward_target(
            state.my_position,
            target,
            state.movement_matrix
        )
    return Direction.STAY
```

## Working with Replays

### Reading Replays
```python
from pyrat_base.replay import ReplayReader, ReplayPlayer

# Parse a replay file
reader = ReplayReader()
replay = reader.read_file("game.pyrat")

# Reconstruct and step through the game
player = ReplayPlayer(replay)
while player.step_forward():
    print(f"Turn {player.current_turn}: {player.game.player1_score} - {player.game.player2_score}")
```

### Writing Replays
```python
from pyrat_base.replay import StreamingReplayWriter

# During a game
writer = StreamingReplayWriter("output.pyrat", metadata)
writer.write_initial_state(initial_state)
writer.write_move(move_data)
writer.close()
```

## Test Coverage

The protocol package has comprehensive test coverage with 177 tests covering:
- Protocol parsing and formatting
- Non-blocking I/O and interruption
- Game state management
- Pathfinding algorithms
- Replay system with cross-platform support
- Input validation

## Architecture Notes

- **Threading**: IOHandler uses a background thread for continuous stdin reading
- **Interruption**: Move calculations can be interrupted by 'stop' command
- **State Management**: ProtocolState is a thin wrapper around PyGameState
- **Coordinate System**: (0,0) is bottom-left, UP increases y
- **Wall Format**: Walls are tuples of adjacent cells
- **Mud Mechanics**: Mud value N means N turns to traverse

## CI/CD
The protocol package is tested with:
- Python 3.8-3.11 compatibility
- Ruff formatting and linting
- MyPy type checking
- pytest unit tests
