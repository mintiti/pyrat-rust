"""Unit tests for move provider abstraction."""

from typing import Optional
from unittest.mock import MagicMock, patch

from pyrat_engine import PyRat
from pyrat_engine.core import Direction

from pyrat_runner.ai_process import AIInfo
from pyrat_runner.game_runner import run_game
from pyrat_runner.move_providers import SubprocessMoveProvider


# ============================================================================
# Test Utilities (Mocks - not tested themselves, used to test other code)
# ============================================================================


class MockMoveProvider:
    """Mock move provider for testing game logic.

    This is a test utility, not production code. It's used to test
    components that interact with MoveProviders (like run_game).
    """

    def __init__(self, name: str, moves: list[Optional[Direction]], alive: bool = True):
        self._info = AIInfo(name=name, author="Test")
        self._moves = moves
        self._move_index = 0
        self._alive = alive

    @property
    def info(self) -> AIInfo:
        return self._info

    def start(self) -> bool:
        return True

    def send_game_start(self, game, preprocessing_time: float) -> None:
        pass

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
        pass

    def stop(self) -> None:
        pass

    def is_alive(self) -> bool:
        return self._alive


# ============================================================================
# Unit Tests: SubprocessMoveProvider
# Purpose: Verify it correctly wraps AIProcess
# ============================================================================


class TestSubprocessMoveProvider:
    """Test that SubprocessMoveProvider correctly delegates to AIProcess."""

    @patch("pyrat_runner.move_providers.AIProcess")
    def test_delegates_all_methods_to_ai_process(self, mock_ai_process_class):
        """Test that all methods properly delegate to the underlying AIProcess."""
        # Setup mock
        mock_ai = MagicMock()
        mock_ai.info = AIInfo(name="TestAI", author="Author")
        mock_ai.start.return_value = True
        mock_ai.get_move.return_value = Direction.UP
        mock_ai.is_alive.return_value = True
        mock_ai_process_class.return_value = mock_ai

        # Create provider
        provider = SubprocessMoveProvider("/path/to/script.py", "rat", 1.5)

        # Verify constructor (allow optional logger kwarg)
        assert mock_ai_process_class.call_count == 1
        called_args, called_kwargs = mock_ai_process_class.call_args
        assert called_args == ("/path/to/script.py", "rat", 1.5)
        # SubprocessMoveProvider may pass a logger kwarg; accept presence with any value
        assert "logger" in called_kwargs

        # Test info property
        assert provider.info.name == "TestAI"

        # Test start
        assert provider.start() is True
        mock_ai.start.assert_called_once()

        # Test send_game_start
        game = PyRat(width=5, height=5, cheese_count=1, seed=42)
        provider.send_game_start(game, 3.0)
        mock_ai.send_game_start.assert_called_once_with(game, 3.0)

        # Test get_move
        move = provider.get_move(Direction.LEFT, Direction.RIGHT)
        assert move == Direction.UP
        mock_ai.get_move.assert_called_once_with(Direction.LEFT, Direction.RIGHT)

        # Test send_game_over
        provider.send_game_over("rat", 5.0, 3.0)
        mock_ai.send_game_over.assert_called_once_with("rat", 5.0, 3.0)

        # Test stop
        provider.stop()
        mock_ai.stop.assert_called_once()

        # Test is_alive
        assert provider.is_alive() is True
        mock_ai.is_alive.assert_called_once()


# ============================================================================
# Unit Tests: run_game() function
# Purpose: Verify game loop logic handles providers correctly
# ============================================================================


class TestRunGameFunction:
    """Test run_game() function with mock providers."""

    def test_runs_game_to_completion(self):
        """Test that run_game executes a full game and returns correct result."""
        game = PyRat(width=5, height=5, cheese_count=1, seed=42)

        # Providers that just stay still (game will eventually end)
        rat = MockMoveProvider("Rat", [Direction.STAY] * 500)
        python = MockMoveProvider("Python", [Direction.STAY] * 500)

        success, winner, rat_score, python_score = run_game(
            game, rat, python, display=None, display_delay=0.0
        )

        assert success is True
        assert winner in ["rat", "python", "draw"]
        assert rat_score >= 0
        assert python_score >= 0

    def test_headless_mode_no_display(self):
        """Test that run_game works without a display object."""
        game = PyRat(width=5, height=5, cheese_count=1, seed=42)
        rat = MockMoveProvider("Rat", [Direction.RIGHT] * 100)
        python = MockMoveProvider("Python", [Direction.UP] * 100)

        # Should not crash without display
        success, winner, rat_score, python_score = run_game(
            game, rat, python, display=None
        )

        assert success is True
        assert isinstance(winner, str)

    def test_handles_provider_crash(self):
        """Test that run_game handles provider crash (returns None + not alive)."""
        game = PyRat(width=5, height=5, cheese_count=1, seed=42)

        # Rat crashes after 3 moves
        rat = MockMoveProvider(
            "Rat",
            [Direction.RIGHT, Direction.RIGHT, Direction.RIGHT, None],
            alive=False,
        )
        python = MockMoveProvider("Python", [Direction.UP] * 100)

        success, winner, rat_score, python_score = run_game(
            game, rat, python, display=None
        )

        # Game should fail due to crash
        assert success is False
        # But still return a winner/scores
        assert winner in ["rat", "python", "draw"]
        assert isinstance(rat_score, float)
        assert isinstance(python_score, float)

    def test_handles_timeout_gracefully(self):
        """Test that timeouts (None move + still alive) are treated as STAY."""
        game = PyRat(width=5, height=5, cheese_count=1, seed=42)

        # Rat times out (returns None) but stays alive
        rat = MockMoveProvider(
            "Rat", [Direction.RIGHT, None, Direction.RIGHT], alive=True
        )
        python = MockMoveProvider("Python", [Direction.UP] * 100)

        success, winner, rat_score, python_score = run_game(
            game, rat, python, display=None
        )

        # Should continue and complete successfully
        assert success is True

    def test_returns_correct_scores(self):
        """Test that run_game returns valid scores."""
        game = PyRat(width=5, height=5, cheese_count=1, seed=42)

        rat = MockMoveProvider("Rat", [Direction.RIGHT] * 100)
        python = MockMoveProvider("Python", [Direction.UP] * 100)

        success, winner, rat_score, python_score = run_game(
            game, rat, python, display=None
        )

        # Scores should be valid numbers
        assert success is True
        assert isinstance(rat_score, float)
        assert isinstance(python_score, float)
        assert rat_score >= 0
        assert python_score >= 0
        # Total score should not exceed total cheese
        assert rat_score + python_score <= 1.0


# ============================================================================
# Integration Tests: Full System
# Purpose: Test real components working together
# ============================================================================


class TestGameRunnerIntegration:
    """Integration tests for the full game system."""

    def test_headless_mode_with_real_game_runner(self):
        """Test that GameRunner works in headless mode."""
        from pyrat_runner.game_runner import GameRunner
        import tempfile
        import textwrap

        # Create minimal AI scripts
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
            runner = GameRunner(
                rat_script=rat_script,
                python_script=python_script,
                width=5,
                height=5,
                cheese_count=1,
                seed=42,
                headless=True,
            )

            # Verify headless setup
            assert runner.headless is True
            assert runner.display is None

        finally:
            import os

            os.unlink(rat_script)
            os.unlink(python_script)

    def test_move_provider_abstraction_allows_mocking(self):
        """Test that we can inject mock providers for testing (key benefit of abstraction)."""
        from pyrat_runner.game_runner import GameRunner

        # This demonstrates the value of the abstraction:
        # We can create a GameRunner and replace its providers with mocks for testing
        runner = GameRunner(
            rat_script="/fake/path.py",  # Won't be used
            python_script="/fake/path.py",
            width=5,
            height=5,
            cheese_count=1,
            seed=42,
            headless=True,
        )

        # Replace with mock providers (this is the power of the abstraction!)
        runner.rat_provider = MockMoveProvider("MockRat", [Direction.STAY] * 100)
        runner.python_provider = MockMoveProvider("MockPython", [Direction.STAY] * 100)

        # Now we can test GameRunner logic without subprocess overhead
        assert runner.rat_provider.info.name == "MockRat"
        assert runner.python_provider.info.name == "MockPython"
