"""Unit tests for move provider abstraction."""

from typing import Optional
from unittest.mock import MagicMock, patch

from pyrat_engine import PyRat
from pyrat_engine.core import Direction

from pyrat_runner.ai_process import AIInfo
from pyrat_runner.game_runner import run_game
from pyrat_runner.move_providers import MoveProvider, SubprocessMoveProvider


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

    def test_protocol_has_all_required_methods(self):
        """Verify MoveProvider protocol defines all required methods."""
        # Check that the protocol has all expected attributes
        from pyrat_runner.move_providers import MoveProvider

        # These should all exist on the protocol
        assert hasattr(MoveProvider, "info")
        assert hasattr(MoveProvider, "start")
        assert hasattr(MoveProvider, "send_game_start")
        assert hasattr(MoveProvider, "get_move")
        assert hasattr(MoveProvider, "send_game_over")
        assert hasattr(MoveProvider, "stop")
        assert hasattr(MoveProvider, "is_alive")

    def test_mock_provider_has_all_protocol_methods(self):
        """Verify MockMoveProvider has all methods defined by the protocol."""
        provider = MockMoveProvider("Test", [Direction.UP])

        # Verify all methods exist and are callable
        assert hasattr(provider, "info")
        assert hasattr(provider, "start")
        assert callable(provider.start)
        assert hasattr(provider, "send_game_start")
        assert callable(provider.send_game_start)
        assert hasattr(provider, "get_move")
        assert callable(provider.get_move)
        assert hasattr(provider, "send_game_over")
        assert callable(provider.send_game_over)
        assert hasattr(provider, "stop")
        assert callable(provider.stop)
        assert hasattr(provider, "is_alive")
        assert callable(provider.is_alive)

    def test_mock_provider_info_property(self):
        """Test MockMoveProvider info property."""
        provider = MockMoveProvider("TestBot", [Direction.STAY])

        assert isinstance(provider.info, AIInfo)
        assert provider.info.name == "TestBot"
        assert provider.info.author == "Test Author"

    def test_mock_provider_lifecycle(self):
        """Test MockMoveProvider lifecycle methods."""
        provider = MockMoveProvider("TestBot", [Direction.UP, Direction.DOWN])

        # Test start
        assert provider.start() is True
        assert provider._started is True

        # Test send_game_start
        game = PyRat(width=5, height=5, cheese_count=1, seed=42)
        provider.send_game_start(game, 3.0)
        assert provider._game_started is True

        # Test get_move
        move = provider.get_move(Direction.STAY, Direction.STAY)
        assert move == Direction.UP
        move = provider.get_move(Direction.UP, Direction.STAY)
        assert move == Direction.DOWN

        # Test is_alive
        assert provider.is_alive() is True

        # Test send_game_over
        provider.send_game_over("rat", 5.0, 3.0)
        assert provider._game_over_called is True

        # Test stop
        provider.stop()  # Should not raise

    def test_mock_provider_moves_exhausted(self):
        """Test MockMoveProvider behavior when moves are exhausted."""
        provider = MockMoveProvider("TestBot", [Direction.UP])

        # First move from list
        move = provider.get_move(Direction.STAY, Direction.STAY)
        assert move == Direction.UP

        # Subsequent moves default to STAY
        move = provider.get_move(Direction.UP, Direction.STAY)
        assert move == Direction.STAY

    def test_mock_provider_with_none_move(self):
        """Test MockMoveProvider can return None (simulating timeout)."""
        provider = MockMoveProvider("TestBot", [Direction.UP, None, Direction.DOWN])

        move1 = provider.get_move(Direction.STAY, Direction.STAY)
        assert move1 == Direction.UP

        move2 = provider.get_move(Direction.UP, Direction.STAY)
        assert move2 is None

        move3 = provider.get_move(Direction.UP, Direction.STAY)
        assert move3 == Direction.DOWN

    def test_mock_provider_dead_state(self):
        """Test MockMoveProvider when marked as not alive."""
        provider = MockMoveProvider("TestBot", [Direction.UP], alive=False)

        assert provider.is_alive() is False


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

    def test_subprocess_provider_implements_protocol(self):
        """Test that SubprocessMoveProvider satisfies MoveProvider protocol."""
        provider: MoveProvider = SubprocessMoveProvider(
            script_path="/fake/path.py", player_name="rat", timeout=1.0
        )
        # If this compiles without type errors, the protocol is satisfied
        assert hasattr(provider, "info")
        assert hasattr(provider, "start")
        assert hasattr(provider, "send_game_start")
        assert hasattr(provider, "get_move")
        assert hasattr(provider, "send_game_over")
        assert hasattr(provider, "stop")
        assert hasattr(provider, "is_alive")

    @patch("pyrat_runner.move_providers.AIProcess")
    def test_subprocess_provider_delegates_info(self, mock_ai_process_class):
        """Test that info property delegates to AIProcess."""
        mock_ai_instance = MagicMock()
        mock_info = AIInfo(name="TestAI", author="Test Author")
        mock_ai_instance.info = mock_info
        mock_ai_process_class.return_value = mock_ai_instance

        provider = SubprocessMoveProvider(
            script_path="/fake/path.py", player_name="rat", timeout=1.0
        )

        assert provider.info == mock_info
        assert provider.info.name == "TestAI"
        assert provider.info.author == "Test Author"

    @patch("pyrat_runner.move_providers.AIProcess")
    def test_subprocess_provider_delegates_start(self, mock_ai_process_class):
        """Test that start() delegates to AIProcess.start()."""
        mock_ai_instance = MagicMock()
        mock_ai_instance.start.return_value = True
        mock_ai_process_class.return_value = mock_ai_instance

        provider = SubprocessMoveProvider(
            script_path="/fake/path.py", player_name="rat", timeout=1.0
        )

        result = provider.start()

        assert result is True
        mock_ai_instance.start.assert_called_once()

    @patch("pyrat_runner.move_providers.AIProcess")
    def test_subprocess_provider_delegates_send_game_start(self, mock_ai_process_class):
        """Test that send_game_start() delegates to AIProcess."""
        mock_ai_instance = MagicMock()
        mock_ai_process_class.return_value = mock_ai_instance

        provider = SubprocessMoveProvider(
            script_path="/fake/path.py", player_name="rat", timeout=1.0
        )

        game = PyRat(width=5, height=5, cheese_count=1, seed=42)
        provider.send_game_start(game, preprocessing_time=3.0)

        mock_ai_instance.send_game_start.assert_called_once_with(game, 3.0)

    @patch("pyrat_runner.move_providers.AIProcess")
    def test_subprocess_provider_delegates_get_move(self, mock_ai_process_class):
        """Test that get_move() delegates to AIProcess.get_move()."""
        mock_ai_instance = MagicMock()
        mock_ai_instance.get_move.return_value = Direction.UP
        mock_ai_process_class.return_value = mock_ai_instance

        provider = SubprocessMoveProvider(
            script_path="/fake/path.py", player_name="rat", timeout=1.0
        )

        move = provider.get_move(Direction.STAY, Direction.LEFT)

        assert move == Direction.UP
        mock_ai_instance.get_move.assert_called_once_with(Direction.STAY, Direction.LEFT)

    @patch("pyrat_runner.move_providers.AIProcess")
    def test_subprocess_provider_delegates_send_game_over(self, mock_ai_process_class):
        """Test that send_game_over() delegates to AIProcess."""
        mock_ai_instance = MagicMock()
        mock_ai_process_class.return_value = mock_ai_instance

        provider = SubprocessMoveProvider(
            script_path="/fake/path.py", player_name="rat", timeout=1.0
        )

        provider.send_game_over("rat", 10.0, 5.0)

        mock_ai_instance.send_game_over.assert_called_once_with("rat", 10.0, 5.0)

    @patch("pyrat_runner.move_providers.AIProcess")
    def test_subprocess_provider_delegates_stop(self, mock_ai_process_class):
        """Test that stop() delegates to AIProcess.stop()."""
        mock_ai_instance = MagicMock()
        mock_ai_process_class.return_value = mock_ai_instance

        provider = SubprocessMoveProvider(
            script_path="/fake/path.py", player_name="rat", timeout=1.0
        )

        provider.stop()

        mock_ai_instance.stop.assert_called_once()

    @patch("pyrat_runner.move_providers.AIProcess")
    def test_subprocess_provider_delegates_is_alive(self, mock_ai_process_class):
        """Test that is_alive() delegates to AIProcess.is_alive()."""
        mock_ai_instance = MagicMock()
        mock_ai_instance.is_alive.return_value = True
        mock_ai_process_class.return_value = mock_ai_instance

        provider = SubprocessMoveProvider(
            script_path="/fake/path.py", player_name="rat", timeout=1.0
        )

        result = provider.is_alive()

        assert result is True
        mock_ai_instance.is_alive.assert_called_once()

    @patch("pyrat_runner.move_providers.AIProcess")
    def test_subprocess_provider_passes_init_params(self, mock_ai_process_class):
        """Test that SubprocessMoveProvider passes correct params to AIProcess."""
        SubprocessMoveProvider(
            script_path="/path/to/script.py", player_name="python", timeout=2.5
        )

        mock_ai_process_class.assert_called_once_with(
            "/path/to/script.py", "python", 2.5
        )


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
