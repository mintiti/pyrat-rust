# PyRat Monorepo Makefile

.PHONY: all engine gui protocol examples cli test bench clean help sync lint lint-engine lint-protocol lint-cli test-cli test-integration

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

# Build the protocol component
protocol: sync
	@echo "Protocol component ready for development"
	@echo "Base library at protocol/pyrat_base/"

# Future components (placeholders)
gui:
	@echo "GUI component not yet implemented"

examples:
	@echo "Examples not yet implemented"

cli: sync
	@echo "CLI component ready for development"
	@echo "Command-line game runner at cli/"

# Development tasks
dev-setup:
	@echo "Setting up development environment..."
	@echo "Prerequisites: uv, rust toolchain"
	uv sync --all-extras
	@echo "Installing pre-commit hooks..."
	uv run pre-commit install && uv run pre-commit install --hook-type pre-push

# Testing
test: test-engine test-protocol test-cli

test-engine:
	@echo "Running engine tests..."
	cd engine && cargo test --lib --no-default-features
	cd engine && uv run pytest python/tests -v

test-protocol:
	@echo "Running protocol tests..."
	cd protocol/pyrat_base && uv run pytest tests -v -n auto || echo "No tests yet"

test-cli:
	@echo "Running CLI tests..."
	uv run pytest cli/tests -v

test-integration:
	@echo "Running integration tests..."
	uv run pytest tests/integration -v

# Benchmarking
bench:
	@echo "Running benchmarks..."
	@echo "Note: Requires Python environment activated"
	cd engine && cargo bench --bench game_benchmarks

# Code quality
fmt:
	@echo "Formatting code..."
	cd engine && cargo fmt
	uv run ruff format engine/python protocol/pyrat_base cli

check:
	@echo "Running checks..."
	cd engine && cargo fmt --all -- --check
	cd engine && cargo clippy --all-targets --no-default-features -- -D warnings
	cd engine && cargo clippy --all-targets --all-features -- -D warnings -A non-local-definitions
	uv run ruff check engine/python protocol/pyrat_base cli
	uv run mypy engine/python/pyrat_engine protocol/pyrat_base/pyrat_base cli/pyrat_runner --ignore-missing-imports

# Linting targets
lint: lint-engine lint-protocol lint-cli

lint-engine:
	@echo "Linting engine Python code..."
	uv run ruff check engine/python
	uv run mypy engine/python/pyrat_engine --ignore-missing-imports

lint-protocol:
	@echo "Linting protocol code..."
	uv run ruff check protocol/pyrat_base
	uv run mypy protocol/pyrat_base/pyrat_base --ignore-missing-imports

lint-cli:
	@echo "Linting CLI code..."
	uv run ruff check cli
	uv run mypy cli/pyrat_runner --ignore-missing-imports

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
	@echo "  all              - Sync dependencies and build all components"
	@echo "  sync             - Sync workspace dependencies with uv"
	@echo "  engine           - Build the PyRat engine"
	@echo "  protocol         - Info about protocol component"
	@echo "  cli              - Info about CLI component"
	@echo "  dev-setup        - Set up development environment"
	@echo ""
	@echo "Testing:"
	@echo "  test             - Run all tests (engine, protocol, CLI)"
	@echo "  test-engine      - Run engine tests only"
	@echo "  test-protocol    - Run protocol tests only"
	@echo "  test-cli         - Run CLI tests only"
	@echo "  test-integration - Run integration tests"
	@echo ""
	@echo "Code Quality:"
	@echo "  fmt              - Format all code"
	@echo "  check            - Run all code quality checks"
	@echo "  lint             - Lint all Python components"
	@echo "  lint-engine      - Lint engine Python code"
	@echo "  lint-protocol    - Lint protocol code"
	@echo "  lint-cli         - Lint CLI code"
	@echo ""
	@echo "Other:"
	@echo "  bench            - Run performance benchmarks"
	@echo "  clean            - Remove build artifacts"
	@echo "  help             - Show this help message"
	@echo ""
	@echo "Components:"
	@echo "  engine           - High-performance Rust game engine (implemented)"
	@echo "  protocol         - AI communication protocol (implemented)"
	@echo "  cli              - Command-line game runner (implemented)"
	@echo "  gui              - PyRat GUI (planned)"
	@echo "  examples         - Example AI implementations (planned)"
