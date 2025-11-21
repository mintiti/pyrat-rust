"""Integration tests for CLI game runner."""

import tempfile
import textwrap

from pyrat_engine.core import Direction
from pyrat_runner.game_runner import GameRunner


class TestGameRunnerIntegration:
    """Integration tests with real AI processes."""

    def test_headless_mode(self):
        """Test headless mode with real AI scripts."""
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

            assert runner.headless is True
            assert runner.display is None

        finally:
            import os
            os.unlink(rat_script)
            os.unlink(python_script)

    def test_provider_injection(self):
        """Test that providers can be replaced for testing."""
        from pyrat_runner.ai_process import AIInfo

        class FakeProvider:
            def __init__(self, name):
                self._name = name

            @property
            def info(self):
                return AIInfo(name=self._name, author="Test")

            def start(self):
                return True

            def send_game_start(self, game, preprocessing_time):
                pass

            def get_move(self, rat_move, python_move):
                return Direction.STAY

            def send_game_over(self, winner, rat_score, python_score):
                pass

            def stop(self):
                pass

            def is_alive(self):
                return True

        runner = GameRunner(
            rat_script="/fake/path.py",
            python_script="/fake/path.py",
            width=5,
            height=5,
            cheese_count=1,
            seed=42,
            headless=True,
        )

        runner.rat_provider = FakeProvider("FakeRat")
        runner.python_provider = FakeProvider("FakePython")

        assert runner.rat_provider.info.name == "FakeRat"
        assert runner.python_provider.info.name == "FakePython"
