# PyRat Monorepo Makefile

.PHONY: all engine gui protocol examples cli test bench clean help sync

# Default target
all: sync engine

# Sync workspace dependencies
sync:
	@echo "Syncing workspace dependencies..."
	uv sync

# Build the engine component
engine: sync
	@echo "Building PyRat Engine..."
	source .venv/bin/activate && cd engine && maturin develop --release

# Build the protocol component
protocol: sync
	@echo "Protocol component ready for development"
	@echo "Base library at protocol/pyrat_base/"

# Future components (placeholders)
gui:
	@echo "GUI component not yet implemented"

examples:
	@echo "Examples not yet implemented"

cli:
	@echo "CLI tools not yet implemented"

# Development tasks
dev-setup:
	@echo "Setting up development environment..."
	@echo "Prerequisites: uv, rust toolchain"
	uv sync
	@echo "Installing pre-commit hooks..."
	source .venv/bin/activate && pre-commit install && pre-commit install --hook-type pre-push

# Testing
test: test-engine test-protocol

test-engine:
	@echo "Running engine tests..."
	cd engine && cargo test --lib
	source .venv/bin/activate && cd engine && pytest python/tests -v

test-protocol:
	@echo "Running protocol tests..."
	source .venv/bin/activate && cd protocol/pyrat_base && pytest tests -v || echo "No tests yet"

# Benchmarking
bench:
	@echo "Running benchmarks..."
	@echo "Note: Requires Python environment activated"
	cd engine && cargo bench --bench game_benchmarks

# Code quality
fmt:
	@echo "Formatting code..."
	cd engine && cargo fmt
	source .venv/bin/activate && ruff format engine/python protocol/pyrat_base

check:
	@echo "Running checks..."
	cd engine && cargo fmt --all -- --check
	cd engine && cargo clippy --all-targets --all-features -- -D warnings -A non-local-definitions
	source .venv/bin/activate && ruff check engine/python protocol/pyrat_base
	source .venv/bin/activate && mypy engine/python protocol/pyrat_base --ignore-missing-imports

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
	@echo "  all          - Sync dependencies and build all components"
	@echo "  sync         - Sync workspace dependencies with uv"
	@echo "  engine       - Build the PyRat engine"
	@echo "  protocol     - Info about protocol component"
	@echo "  dev-setup    - Set up development environment"
	@echo "  test         - Run all tests"
	@echo "  test-engine  - Run engine tests only"
	@echo "  test-protocol- Run protocol tests only"
	@echo "  bench        - Run performance benchmarks"
	@echo "  fmt          - Format all code"
	@echo "  check        - Run code quality checks"
	@echo "  clean        - Remove build artifacts"
	@echo "  help         - Show this help message"
	@echo ""
	@echo "Components:"
	@echo "  engine       - High-performance Rust game engine (implemented)"
	@echo "  protocol     - AI communication protocol (in development)"
	@echo "  gui          - PyRat GUI (planned)"
	@echo "  examples     - Example AI implementations (planned)"
	@echo "  cli          - Command-line tools (planned)"
