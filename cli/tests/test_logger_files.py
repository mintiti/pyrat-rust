"""Tests for GameLogger basic file layout and writing.

Verifies that master and per-AI files are created and accept writes.
"""

from __future__ import annotations


from pyrat_runner.logger import GameLogger


def test_game_logger_creates_expected_files(tmp_path) -> None:
    root = tmp_path / "run"
    logger = GameLogger(str(root))

    # Write sample entries
    logger.event("Starting")
    logger.protocol("rat", "→", "pyrat")
    logger.protocol("rat", "←", "pyratready")
    logger.stderr("python", "some error line")

    # Validate file existence
    assert (root / "master.log").exists()
    assert (root / "master.protocol").exists()
    assert (root / "rat" / "protocol.log").exists()
    assert (root / "python" / "stderr.txt").exists()

    # Read and assert contents contain markers
    assert "Starting" in (root / "master.log").read_text()
    mp = (root / "master.protocol").read_text()
    assert "[rat] → pyrat" in mp or "→ pyrat" in mp
    assert (
        (root / "python" / "stderr.txt").read_text().strip().endswith("some error line")
    )

    logger.close()
