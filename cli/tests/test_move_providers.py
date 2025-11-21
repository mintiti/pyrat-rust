"""Unit tests for move provider abstraction."""

from typing import Optional
from unittest.mock import MagicMock, patch

from pyrat_engine import PyRat
from pyrat_engine.core import Direction

from pyrat_runner.ai_process import AIInfo
from pyrat_runner.game_runner import run_game, GameRunner
from pyrat_runner.move_providers import SubprocessMoveProvider
import time


class MockMoveProvider:
    """Mock move provider for testing game logic."""

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

    def notify_timeout(self, default_move: Direction) -> None:
        # No-op for mock
        pass


class TestSubprocessMoveProvider:
    """Test that SubprocessMoveProvider correctly delegates to AIProcess."""

    @patch("pyrat_runner.move_providers.AIProcess")
    def test_delegates_all_methods_to_ai_process(self, mock_ai_process_class):
        """Test that all methods properly delegate to the underlying AIProcess."""
        mock_ai = MagicMock()
        mock_ai.info = AIInfo(name="TestAI", author="Author")
        mock_ai.start.return_value = True
        mock_ai.get_move.return_value = Direction.UP
        mock_ai.is_alive.return_value = True
        mock_ai_process_class.return_value = mock_ai

        provider = SubprocessMoveProvider("/path/to/script.py", "rat", 1.5)

        # Verify constructor (allow optional logger kwarg)
        assert mock_ai_process_class.call_count == 1
        called_args, called_kwargs = mock_ai_process_class.call_args
        assert called_args == ("/path/to/script.py", "rat", 1.5)
        # SubprocessMoveProvider may pass a logger kwarg; accept presence with any value
        assert "logger" in called_kwargs

        assert provider.info.name == "TestAI"

        assert provider.start() is True
        mock_ai.start.assert_called_once()

        game = PyRat(width=5, height=5, cheese_count=1, seed=42)
        provider.send_game_start(game, 3.0)
        mock_ai.send_game_start.assert_called_once_with(game, 3.0)

        move = provider.get_move(Direction.LEFT, Direction.RIGHT)
        assert move == Direction.UP
        mock_ai.get_move.assert_called_once_with(Direction.LEFT, Direction.RIGHT)

        provider.send_game_over("rat", 5.0, 3.0)
        mock_ai.send_game_over.assert_called_once_with("rat", 5.0, 3.0)

        provider.stop()
        mock_ai.stop.assert_called_once()

        assert provider.is_alive() is True
        mock_ai.is_alive.assert_called_once()


class TestRunGameFunction:
    """Test run_game() function with mock providers."""

    def test_runs_game_to_completion(self):
        """Test that run_game executes a full game and returns correct result."""
        game = PyRat(width=5, height=5, cheese_count=1, seed=42)

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

        assert success is False
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

        assert success is True

    def test_returns_correct_scores(self):
        """Test that run_game returns valid scores."""
        game = PyRat(width=5, height=5, cheese_count=1, seed=42)

        rat = MockMoveProvider("Rat", [Direction.RIGHT] * 100)
        python = MockMoveProvider("Python", [Direction.UP] * 100)

        success, winner, rat_score, python_score = run_game(
            game, rat, python, display=None
        )

        assert success is True
        assert isinstance(rat_score, float)
        assert isinstance(python_score, float)
        assert rat_score >= 0
        assert python_score >= 0
        assert rat_score + python_score <= 1.0

    def test_notifies_timeout_when_provider_returns_none(self):
        """Provider.notify_timeout should be called when get_move returns None and provider is alive."""

        class SpyProvider(MockMoveProvider):
            def __init__(self, name, moves, alive=True):
                super().__init__(name, moves, alive)
                self.timeout_notified = 0

            def notify_timeout(self, default_move: Direction) -> None:
                self.timeout_notified += 1

        # Minimal fake game that ends after a single step
        class FakeGame:
            def __init__(self):
                self._done = False

            @property
            def scores(self):
                return (0.0, 0.0)

            def step(self, p1_move: Direction, p2_move: Direction):
                class R:
                    pass

                r = R()
                r.game_over = True  # end after first step
                return r

        game = FakeGame()
        rat = SpyProvider("Rat", [None], alive=True)
        python = SpyProvider("Python", [Direction.STAY], alive=True)

        success, _, _, _ = run_game(game, rat, python, display=None, display_delay=0.0)
        assert success is True
        assert rat.timeout_notified >= 1  # timeout reported to provider

    def test_requests_moves_concurrently(self):
        """Both providers are queried in parallel to bound per-turn wall time."""

        class SleepProvider(MockMoveProvider):
            def __init__(self, name, sleep_s: float):
                super().__init__(name, [Direction.STAY])
                self._sleep = sleep_s

            def get_move(self, rat_move: Direction, python_move: Direction):
                time.sleep(self._sleep)
                return Direction.STAY

        class FakeGame:
            @property
            def scores(self):
                return (0.0, 0.0)

            def step(self, p1_move: Direction, p2_move: Direction):
                class R:
                    pass

                r = R()
                r.game_over = True
                return r

        # If sequential, ~0.4s; if parallel, ~0.2s (allow generous margin for CI)
        game = FakeGame()
        rat = SleepProvider("Rat", 0.2)
        python = SleepProvider("Python", 0.2)
        t0 = time.time()
        run_game(game, rat, python, display=None, display_delay=0.0)
        elapsed = time.time() - t0
        assert elapsed < 0.45


class TestGameRunnerInjection:
    def test_provider_injection_in_game_runner(self, monkeypatch):
        """GameRunner should use injected providers instead of constructing subprocess providers."""

        # Fake providers that record calls
        class FakeProvider:
            def __init__(self, name):
                from pyrat_runner.ai_process import AIInfo

                self._info = AIInfo(name=name, author="Test")
                self.started = False
                self.started_game = False
                self.game_over = False
                self.stopped = False

            @property
            def info(self):
                return self._info

            def start(self):
                self.started = True
                return True

            def send_game_start(self, game, preprocessing_time):
                self.started_game = True

            def get_move(self, rat_move, python_move):
                return Direction.STAY

            def send_game_over(self, winner, rat_score, python_score):
                self.game_over = True

            def stop(self):
                self.stopped = True

            def is_alive(self):
                return True

            def notify_timeout(self, default_move: Direction) -> None:
                pass

        rat = FakeProvider("InjectedRat")
        python = FakeProvider("InjectedPython")

        # Stub run_game to avoid running the engine; return a fixed outcome
        import pyrat_runner.game_runner as gr_mod

        def fake_run_game(game, rp, pp, display, display_delay):
            return True, "draw", 0.0, 0.0

        monkeypatch.setattr(gr_mod, "run_game", fake_run_game)

        runner = GameRunner(
            rat_script="/ignored.py",
            python_script="/ignored.py",
            width=5,
            height=5,
            cheese_count=1,
            seed=42,
            headless=True,
            rat_provider=rat,
            python_provider=python,
        )

        assert runner.rat_provider is rat
        assert runner.python_provider is python

        assert runner.run() is True
        assert rat.started and python.started
        assert rat.started_game and python.started_game
        assert rat.game_over and python.game_over
        assert rat.stopped and python.stopped
