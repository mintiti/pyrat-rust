"""Tests for display behavior in non-TTY environments with debug override."""

from pyrat_engine import GameConfig
from pyrat_runner.display import Display


def test_display_renders_multiple_frames_with_debug_override(capsys, monkeypatch):
    # Ensure environment signals debug to force full rendering
    monkeypatch.setenv("PYRAT_DEBUG", "1")

    # Create a tiny game to initialize Display
    game = GameConfig.classic(3, 3, 1).create(seed=1)
    disp = Display(game_state=game, delay=0.0)

    # First render
    disp.render()
    first = capsys.readouterr().out
    assert "Turn:" in first

    # Second render should also print (no throttling when PYRAT_DEBUG=1)
    disp.render()
    second = capsys.readouterr().out
    assert second != ""
