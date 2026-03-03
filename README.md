# PyRat Ecosystem

A high-performance game engine and ecosystem for the PyRat maze game, where a Rat and a Python compete to collect cheese.

## Repository Structure

This is a monorepo containing all PyRat ecosystem components:

- **[engine/](engine/)** - High-performance Rust game engine with Python bindings
- **[host/](host/)** - Match hosting library — setup, turn loop, event streaming
- **[headless/](headless/)** - Headless match runner binary — launches bots, runs a match, outputs JSON
- **[wire/](wire/)** - FlatBuffers schema and generated types, shared by host and SDKs
- **[sdk-rust/](sdk-rust/)** - Rust bot SDK
- **[sdk-python/](sdk-python/)** - Python bot SDK

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
# Run a headless match between two Rust bots
cargo run -p pyrat-headless -- "cargo run -p pyrat-sdk --example greedy" "cargo run -p pyrat-sdk --example random"

# Run a match with Python bots
cargo run -p pyrat-headless -- "uv run python sdk-python/examples/greedy.py" "uv run python sdk-python/examples/random_ai.py"
```

### Run Tests

```bash
# Test everything
make test

# Test specific components
make test-engine      # Engine tests
make test-host        # Host library tests
make test-headless    # Headless runner tests
make test-sdk-python  # SDK Python tests
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

### Host
Match hosting library. Manages bot connections, setup handshake, and the turn loop. Streams `MatchEvent`s through a channel for consumers (headless runner, GUI, tournament systems) to process.

### Headless Runner
CLI binary that launches bot subprocesses, runs a match via the host library, and optionally writes a JSON game record.

### SDKs
- **sdk-rust/** — Rust bot SDK. Example: `cargo run -p pyrat-sdk --example greedy`
- **sdk-python/** — Python bot SDK. Example: `uv run python sdk-python/examples/greedy.py`

## Development

The monorepo uses:
- **uv workspaces** for Python dependency management
- **Cargo** for Rust code
- **Pre-commit hooks** for code quality
- **GitHub Actions** for CI/CD

See individual component READMEs for specific development instructions.

## License

This project is licensed under the MIT License - see the LICENSE file for details.
