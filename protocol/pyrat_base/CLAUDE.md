# CLAUDE.md - PyRat Base Library

This file provides guidance to Claude Code when working with the PyRat base library (protocol SDK).

## Component Overview

The PyRat Base Library provides:
- Base classes for AI development
- Protocol parsing and formatting
- Game state management from AI perspective
- Non-blocking I/O handling
- Utilities for common AI tasks

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

### Key Files (to be implemented)
- `pyrat_base/enums.py` - Protocol enums and constants
- `pyrat_base/protocol.py` - Protocol parsing/formatting
- `pyrat_base/protocol_state.py` - Game state wrapper
- `pyrat_base/io_handler.py` - Non-blocking I/O
- `pyrat_base/base_ai.py` - Base class for AI development
- `pyrat_base/utils.py` - Common utilities

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

## CI/CD
The protocol package is tested with:
- Python 3.8-3.11 compatibility
- Ruff formatting and linting
- MyPy type checking
- pytest unit tests
