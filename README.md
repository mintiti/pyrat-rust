# PyRat Ecosystem

A high-performance game engine and ecosystem for the PyRat maze game, where a Rat and a Python compete to collect cheese.

## Repository Structure

This is a monorepo containing all PyRat ecosystem components:

- **[engine/](engine/)** - High-performance Rust game engine with Python bindings
- **[protocol/](protocol/)** - AI communication protocol and base library
- **[cli/](cli/)** - Command-line game runner with enhanced visualization

## Quick Start

### Prerequisites

- Python 3.8+
- Rust toolchain
- [uv](https://docs.astral.sh/uv/) (Python package manager)

### Workspace Setup (Recommended)

This repository uses uv workspaces for seamless development across components:

```bash
# Clone the repository
git clone https://github.com/yourusername/pyrat-rust.git
cd pyrat-rust

# Sync all workspace dependencies with dev tools
uv sync --all-extras

# Build the Rust engine
cd engine
uv run maturin develop --release
```

### Run a Game

```bash
# Run a game between two AIs
pyrat-game protocol/pyrat_base/pyrat_base/examples/greedy_ai.py \
           protocol/pyrat_base/pyrat_base/examples/random_ai.py

# Custom configuration
pyrat-game --width 31 --height 21 --cheese 85 --seed 42 bot1.py bot2.py
```

### Run Tests

```bash
# Test everything
make test

# Test specific components
make test-engine    # Engine tests
make test-protocol  # Protocol tests
make test-cli       # CLI tests
```

### Development Commands

```bash
make help        # Show all available commands
make dev-setup   # Set up development environment
make fmt         # Format all code
make check       # Run all checks
```

## Components

### Engine
High-performance Rust game engine with Python bindings. Use it to:
- Build and train AI agents with reinforcement learning (PettingZoo/Gymnasium compatible)
- Simulate games programmatically
- Benchmark AI strategies

### Protocol & Base Library
Write AIs in any language using a simple text-based protocol (stdin/stdout):
- `protocol/spec.md` - Protocol specification
- `protocol/pyrat_base/` - Python SDK with base classes and helpers
- Example AIs included: dummy (stays still), random (random moves), greedy (Dijkstra pathfinding)

### CLI
Run and visualize games between AI scripts:
```bash
pyrat-game my_ai.py opponent_ai.py
```
Features color visualization, configurable game parameters, and graceful error handling. See [cli/README.md](cli/README.md) for details.

## Development

The monorepo uses:
- **uv workspaces** for Python dependency management
- **Cargo** for Rust code
- **Pre-commit hooks** for code quality
- **GitHub Actions** for CI/CD

See individual component READMEs for specific development instructions.

## License

This project is licensed under the MIT License - see the LICENSE file for details.
