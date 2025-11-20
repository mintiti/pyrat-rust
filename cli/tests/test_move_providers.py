"""Unit tests for move provider abstraction."""

from typing import Optional

from pyrat_engine import PyRat
from pyrat_engine.core import Direction

from pyrat_runner.ai_process import AIInfo
from pyrat_runner.game_runner import run_game
from pyrat_runner.move_providers import MoveProvider


class MockMoveProvider:
    """Mock move provider for testing."""

    def __init__(self, name: str, moves: list[Optional[Direction]], alive: bool = True):
        """
        Initialize mock provider.

        Args:
            name: Provider name
            moves: List of moves to return (None = timeout/error)
            alive: Whether provider should report as alive
        """
        self._info = AIInfo(name=name, author="Test Author")
        self._moves = moves
        self._move_index = 0
        self._alive = alive
        self._started = False
        self._game_started = False
        self._game_over_called = False

    @property
    def info(self) -> AIInfo:
        return self._info

    def start(self) -> bool:
        self._started = True
        return True

    def send_game_start(self, game, preprocessing_time: float) -> None:
        self._game_started = True

    def get_move(
        self, rat_move: Direction, python_move: Direction
    ) -> Optional[Direction]:
        if self._move_index < len(self._moves):
            move = self._moves[self._move_index]
            self._move_index += 1
            return move
        return Direction.STAY

    def send_game_over(
        self, winner: str, rat_score: float, python_score: float
    ) -> None:
        self._game_over_called = True

    def stop(self) -> None:
        pass

    def is_alive(self) -> bool:
        return self._alive


class TestMoveProviderProtocol:
    """Test that MoveProvider protocol is correctly defined."""

    def test_mock_provider_implements_protocol(self):
        """MockMoveProvider should satisfy MoveProvider protocol."""
        provider: MoveProvider = MockMoveProvider("Test", [Direction.STAY])
        assert provider.info.name == "Test"
        assert provider.start()
        assert provider.is_alive()


class TestRunGameFunction:
    """Test the decoupled run_game function."""

    def test_run_game_basic(self):
        """Test basic game execution with mock providers."""
        game = PyRat(width=5, height=5, cheese_count=1, seed=42)

        # Create providers that will make moves until game ends
        rat_provider = MockMoveProvider("Rat", [Direction.STAY] * 100)
        python_provider = MockMoveProvider("Python", [Direction.STAY] * 100)

        success, winner, rat_score, python_score = run_game(
            game, rat_provider, python_provider, display=None, display_delay=0.0
        )

        assert success is True
        assert winner in ["rat", "python", "draw"]
        assert rat_score >= 0
        assert python_score >= 0
        assert rat_score + python_score <= 1  # Only 1 cheese

    def test_run_game_headless(self):
        """Test that run_game works without display (headless mode)."""
        game = PyRat(width=5, height=5, cheese_count=1, seed=42)

        rat_provider = MockMoveProvider("Rat", [Direction.RIGHT] * 100)
        python_provider = MockMoveProvider("Python", [Direction.UP] * 100)

        success, winner, rat_score, python_score = run_game(
            game, rat_provider, python_provider, display=None
        )

        assert success is True
        assert isinstance(winner, str)

    def test_run_game_provider_crash(self):
        """Test that run_game handles provider crash gracefully."""
        game = PyRat(width=5, height=5, cheese_count=1, seed=42)

        # Rat provider crashes after 5 moves
        rat_provider = MockMoveProvider(
            "Rat",
            [
                Direction.STAY,
                Direction.STAY,
                Direction.STAY,
                Direction.STAY,
                Direction.STAY,
                None,
            ],
        )
        rat_provider._alive = False  # Simulate crash

        python_provider = MockMoveProvider("Python", [Direction.STAY] * 100)

        success, winner, rat_score, python_score = run_game(
            game, rat_provider, python_provider, display=None, display_delay=0.0
        )

        # Should fail due to crash
        assert success is False
        # But still return a winner
        assert winner in ["rat", "python", "draw"]

    def test_run_game_timeout_continues(self):
        """Test that timeouts (None moves) are handled as STAY."""
        game = PyRat(width=5, height=5, cheese_count=1, seed=42)

        # Provider times out (returns None) but stays alive
        rat_provider = MockMoveProvider("Rat", [Direction.RIGHT, None, Direction.RIGHT])
        python_provider = MockMoveProvider("Python", [Direction.UP] * 100)

        success, winner, rat_score, python_score = run_game(
            game, rat_provider, python_provider, display=None, display_delay=0.0
        )

        # Should succeed - timeouts don't end the game
        assert success is True


class TestSubprocessMoveProvider:
    """Test SubprocessMoveProvider wrapper."""

    def test_subprocess_provider_exists(self):
        """Test that SubprocessMoveProvider can be imported."""
        from pyrat_runner.move_providers import SubprocessMoveProvider

        # Just test that it exists and can be instantiated
        # (We won't test actual subprocess behavior in unit tests)
        provider = SubprocessMoveProvider(
            script_path="/fake/path.py", player_name="rat", timeout=1.0
        )
        assert provider is not None


class TestGameRunnerRefactoring:
    """Test that GameRunner still works after refactoring."""

    def test_game_runner_headless_mode(self):
        """Test that GameRunner can run in headless mode."""
        from pyrat_runner.game_runner import GameRunner
        import tempfile
        import textwrap

        # Create temporary AI scripts
        with tempfile.NamedTemporaryFile(mode="w", suffix=".py", delete=False) as f1:
            f1.write(
                textwrap.dedent(
                    """
                from pyrat_base.examples.dummy_ai import DummyAI
                if __name__ == "__main__":
                    ai = DummyAI()
                    ai.run()
                """
                )
            )
            rat_script = f1.name

        with tempfile.NamedTemporaryFile(mode="w", suffix=".py", delete=False) as f2:
            f2.write(
                textwrap.dedent(
                    """
                from pyrat_base.examples.dummy_ai import DummyAI
                if __name__ == "__main__":
                    ai = DummyAI()
                    ai.run()
                """
                )
            )
            python_script = f2.name

        try:
            # This test just verifies that headless parameter is accepted
            runner = GameRunner(
                rat_script=rat_script,
                python_script=python_script,
                width=5,
                height=5,
                cheese_count=1,
                seed=42,
                headless=True,
            )
            assert runner.headless is True
            assert runner.display is None
        finally:
            import os

            os.unlink(rat_script)
            os.unlink(python_script)
