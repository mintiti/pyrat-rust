"""
Integration tests for CLI + AI examples.

Tests that the CLI can successfully run games with the example AIs
and that the game completes without errors.
"""

import subprocess
import sys
from pathlib import Path
import pytest


def test_cli_runs_random_vs_random():
    """Test that CLI can run a game between two random AIs."""
    # Find the random AI example
    random_ai = Path(__file__).parent.parent.parent / "protocol/pyrat_base/pyrat_base/examples/random_ai.py"

    if not random_ai.exists():
        pytest.skip(f"Random AI not found at {random_ai}")

    # Run the CLI
    result = subprocess.run(
        [
            sys.executable, "-m", "pyrat_runner.cli",
            "--width", "11",
            "--height", "9",
            "--cheese", "5",
            "--seed", "42",
            "--timeout", "1.0",
            "--delay", "0",
            str(random_ai),
            str(random_ai),
        ],
        capture_output=True,
        text=True,
        timeout=30,
    )

    # Check that the game completed successfully
    assert result.returncode == 0, f"CLI failed with stderr: {result.stderr}"
    assert "GAME OVER" in result.stdout or "Final Score" in result.stdout


def test_cli_runs_greedy_vs_dummy():
    """Test that CLI can run a game between greedy and dummy AIs."""
    # Find the AI examples
    greedy_ai = Path(__file__).parent.parent.parent / "protocol/pyrat_base/pyrat_base/examples/greedy_ai.py"
    dummy_ai = Path(__file__).parent.parent.parent / "protocol/pyrat_base/pyrat_base/examples/dummy_ai.py"

    if not greedy_ai.exists():
        pytest.skip(f"Greedy AI not found at {greedy_ai}")
    if not dummy_ai.exists():
        pytest.skip(f"Dummy AI not found at {dummy_ai}")

    # Run the CLI
    result = subprocess.run(
        [
            sys.executable, "-m", "pyrat_runner.cli",
            "--width", "7",
            "--height", "5",
            "--cheese", "3",
            "--seed", "123",
            "--timeout", "1.0",
            "--delay", "0",
            str(greedy_ai),
            str(dummy_ai),
        ],
        capture_output=True,
        text=True,
        timeout=30,
    )

    # Check that the game completed successfully
    assert result.returncode == 0, f"CLI failed with stderr: {result.stderr}"


def test_cli_handles_timeout():
    """Test that CLI properly handles AI timeouts."""
    # Create a temporary AI that takes too long
    timeout_ai = Path(__file__).parent / "slow_ai.py"
    timeout_ai.write_text("""
from pyrat_base import PyRatAI
from pyrat_engine.core import Direction
import time

class SlowAI(PyRatAI):
    def __init__(self):
        super().__init__("SlowBot", "Test")

    def get_move(self, state):
        time.sleep(10)  # Intentionally too slow
        return Direction.UP

if __name__ == "__main__":
    ai = SlowAI()
    ai.run()
""")

    try:
        # Run CLI with very short timeout
        result = subprocess.run(
            [
                sys.executable, "-m", "pyrat_runner.cli",
                "--width", "7",
                "--height", "5",
                "--cheese", "1",
                "--timeout", "0.1",
                "--delay", "0",
                str(timeout_ai),
                str(timeout_ai),
            ],
            capture_output=True,
            text=True,
            timeout=30,
        )

        # CLI should complete even with timeouts (AIs default to STAY)
        assert result.returncode == 0
    finally:
        # Clean up
        if timeout_ai.exists():
            timeout_ai.unlink()
