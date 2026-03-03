# PyRat Monorepo Makefile

.PHONY: all engine gui examples test bench clean help sync lint lint-engine lint-sdk-python test-sdk-python test-wire test-host test-headless generate-protocol

# Default target
all: sync engine

# Sync workspace dependencies
sync:
	@echo "Syncing workspace dependencies..."
	uv sync --all-extras

# Build the engine component
engine: sync
	@echo "Building PyRat Engine..."
	cd engine && uv run maturin develop --release

# Future components (placeholders)
gui:
	@echo "GUI component not yet implemented"

examples:
	@echo "Examples not yet implemented"

# Development tasks
dev-setup:
	@echo "Setting up development environment..."
	@echo "Prerequisites: uv, rust toolchain"
	uv sync --all-extras
	@echo "Installing pre-commit hooks..."
	uv run pre-commit install && uv run pre-commit install --hook-type pre-push

# Testing
test: test-engine test-wire test-host test-headless test-sdk-python

test-engine:
	@echo "Running engine tests..."
	cargo test -p pyrat-rust --lib --no-default-features
	cd engine && uv run pytest python/tests -v

test-wire:
	@echo "Running wire protocol tests..."
	cargo test -p pyrat-wire

test-host:
	@echo "Running host library tests..."
	cargo test -p pyrat-host

test-headless:
	@echo "Running headless runner tests..."
	cargo test -p pyrat-headless

test-sdk-python:
	@echo "Running SDK Python tests..."
	cd sdk-python && uv run pytest tests -v

# Benchmarking
bench:
	@echo "Running benchmarks..."
	@echo "Note: Requires Python environment activated"
	cargo bench -p pyrat-rust --bench game_benchmarks

# Code quality
fmt:
	@echo "Formatting code..."
	cargo fmt --all
	uv run ruff format engine/python sdk-python/pyrat_sdk

check:
	@echo "Running checks..."
	cargo fmt --all -- --check
	cargo clippy -p pyrat-rust --all-targets --no-default-features -- -D warnings
	cargo clippy --all-targets --all-features -- -D warnings -A non-local-definitions
	uv run ruff check engine/python sdk-python/pyrat_sdk
	uv run mypy engine/python/pyrat_engine sdk-python/pyrat_sdk --ignore-missing-imports

# Linting targets
lint: lint-engine lint-sdk-python

lint-engine:
	@echo "Linting engine Python code..."
	uv run ruff check engine/python
	uv run mypy engine/python/pyrat_engine --ignore-missing-imports

lint-sdk-python:
	@echo "Linting SDK Python code..."
	uv run ruff check sdk-python
	uv run mypy sdk-python/pyrat_sdk --ignore-missing-imports

# Clean build artifacts
generate-protocol:
	@echo "Generating protocol FlatBuffers code..."
	./schema/generate.sh

clean:
	@echo "Cleaning build artifacts..."
	cargo clean
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
	@echo "  all              - Sync dependencies and build all components"
	@echo "  sync             - Sync workspace dependencies with uv"
	@echo "  engine           - Build the PyRat engine"
	@echo "  dev-setup        - Set up development environment"
	@echo ""
	@echo "Testing:"
	@echo "  test             - Run all tests"
	@echo "  test-engine      - Run engine tests only"
	@echo "  test-wire        - Run wire protocol tests"
	@echo "  test-host        - Run host library tests"
	@echo "  test-headless    - Run headless runner tests"
	@echo "  test-sdk-python  - Run SDK Python tests"
	@echo ""
	@echo "Code Quality:"
	@echo "  fmt              - Format all code"
	@echo "  check            - Run all code quality checks"
	@echo "  lint             - Lint all Python components"
	@echo "  lint-engine      - Lint engine Python code"
	@echo ""
	@echo "Other:"
	@echo "  bench              - Run performance benchmarks"
	@echo "  generate-protocol  - Regenerate FlatBuffers Rust code (requires flatc)"
	@echo "  clean              - Remove build artifacts"
	@echo "  help               - Show this help message"
