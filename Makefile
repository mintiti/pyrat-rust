# PyRat Monorepo Makefile

.PHONY: all engine gui protocol examples cli test bench clean help

# Default target
all: engine

# Build the engine component
engine:
	@echo "Building PyRat Engine..."
	cd engine && source .venv/bin/activate && maturin develop --release

# Future components (placeholders)
gui:
	@echo "GUI component not yet implemented"

protocol:
	@echo "Protocol component not yet implemented"

examples:
	@echo "Examples not yet implemented"

cli:
	@echo "CLI tools not yet implemented"

# Development tasks
dev-setup:
	@echo "Setting up development environment..."
	@echo "Prerequisites: uv, rust toolchain"
	cd engine && uv venv && source .venv/bin/activate && uv pip install -e ".[dev]"

# Testing
test: test-engine

test-engine:
	@echo "Running engine tests..."
	cd engine && cargo test --lib
	cd engine && source .venv/bin/activate && pytest python/tests -v

# Benchmarking
bench:
	@echo "Running benchmarks..."
	@echo "Note: Requires Python environment activated"
	cd engine && cargo bench --bench game_benchmarks

# Code quality
fmt:
	@echo "Formatting code..."
	cd engine && cargo fmt

check:
	@echo "Running checks..."
	cd engine && cargo fmt --all -- --check
	cd engine && cargo clippy --all-targets --all-features -- -D warnings -A non-local-definitions

# Clean build artifacts
clean:
	@echo "Cleaning build artifacts..."
	cd engine && cargo clean
	find . -type d -name "__pycache__" -exec rm -rf {} +
	find . -type d -name ".pytest_cache" -exec rm -rf {} +
	find . -type d -name "*.egg-info" -exec rm -rf {} +

# Help target
help:
	@echo "PyRat Monorepo Build System"
	@echo ""
	@echo "Prerequisites:"
	@echo "  - Rust toolchain"
	@echo "  - Python 3.8+"
	@echo "  - uv (Python package manager)"
	@echo ""
	@echo "Available targets:"
	@echo "  all         - Build all components (currently just engine)"
	@echo "  engine      - Build the PyRat engine (requires .venv activated)"
	@echo "  dev-setup   - Set up development environment"
	@echo "  test        - Run all tests"
	@echo "  test-engine - Run engine tests only"
	@echo "  bench       - Run performance benchmarks (requires Python env)"
	@echo "  fmt         - Format all code"
	@echo "  check       - Run code quality checks"
	@echo "  clean       - Remove build artifacts"
	@echo "  help        - Show this help message"
	@echo ""
	@echo "Future components (not yet implemented):"
	@echo "  gui         - PyRat GUI"
	@echo "  protocol    - Protocol specification and SDK"
	@echo "  examples    - Example AI implementations"
	@echo "  cli         - Command-line tools"
