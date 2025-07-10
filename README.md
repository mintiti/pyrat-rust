# PyRat Ecosystem

A high-performance game engine and ecosystem for the PyRat maze game, where a Rat and a Python compete to collect cheese.

## Repository Structure

This is a monorepo containing all PyRat ecosystem components:

- **[engine/](engine/)** - High-performance Rust game engine with Python bindings
- **[gui/](gui/)** - Visualization and tournament management (coming soon)
- **[protocol/](protocol/)** - AI communication protocol specification (coming soon)
- **[examples/](examples/)** - Example AI implementations (coming soon)
- **[cli/](cli/)** - Command-line tools (coming soon)

## Quick Start

### Install the Engine

```bash
cd engine
uv venv
source .venv/bin/activate  # On Windows: .venv\Scripts\activate
uv pip install -e ".[dev]"
maturin develop --release
```

### Run Tests

```bash
# Test the engine
cd engine
pytest python/tests -v
cargo test
```

## Development

Each component has its own README with specific development instructions. The engine is currently the only implemented component, with others coming soon as part of the ecosystem expansion.

## License

This project is licensed under the MIT License - see the LICENSE file for details.
