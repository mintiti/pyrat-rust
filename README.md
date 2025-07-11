# PyRat Ecosystem

A high-performance game engine and ecosystem for the PyRat maze game, where a Rat and a Python compete to collect cheese.

## Repository Structure

This is a monorepo containing all PyRat ecosystem components:

- **[engine/](engine/)** - High-performance Rust game engine with Python bindings
- **[protocol/](protocol/)** - AI communication protocol and base library (in development)
- **[gui/](gui/)** - Visualization and tournament management (coming soon)
- **[examples/](examples/)** - Example AI implementations (coming soon)
- **[cli/](cli/)** - Command-line tools (coming soon)

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

# Sync all workspace dependencies
uv sync

# Build the Rust engine
source .venv/bin/activate  # On Windows: .venv\Scripts\activate
cd engine
maturin develop --release
```

### Run Tests

```bash
# Test everything
make test

# Test specific components
make test-engine    # Engine tests
make test-protocol  # Protocol tests
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
The core game implementation in Rust with Python bindings. Provides high-performance game state management and PettingZoo-compatible environment.

### Protocol
Text-based communication protocol for AI development. Includes:
- Protocol specification (`protocol/spec.md`)
- Base library for AI development (`protocol/pyrat_base/`)
- Language-agnostic design for AI implementation in any language

### Future Components
- **GUI**: Game visualization and tournament management
- **Examples**: Reference AI implementations
- **CLI**: Command-line tools for running games

## Development

The monorepo uses:
- **uv workspaces** for Python dependency management
- **Cargo** for Rust code
- **Pre-commit hooks** for code quality
- **GitHub Actions** for CI/CD

See individual component READMEs for specific development instructions.

## License

This project is licensed under the MIT License - see the LICENSE file for details.
