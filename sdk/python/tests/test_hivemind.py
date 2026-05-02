"""Tests for HivemindBot — multi-player action dispatch and error handling.

The dispatch tests still drive ``_handle_turn`` directly because the adapter
shape (think-returns-dict → two Action frames) is what'll need to keep
working when hivemind support comes back. The public ``run()`` path is
tested separately to assert the documented unsupported-protocol error.
"""

from __future__ import annotations

import pytest
from conftest import MockConnection, make_lifecycle_frames

from pyrat_sdk._engine import parse_bot_frame
from pyrat_sdk.bot import HivemindBot, _run_lifecycle
from pyrat_sdk.state import Direction, Player


class TestHivemindRunUnsupported:
    """Public ``HivemindBot.run()`` must raise ``RuntimeError`` until the new
    wire protocol grows hivemind support — the host's ``accept_players``
    rejects duplicate ``agent_id``, so a hivemind bot can't even connect."""

    def test_run_raises_runtime_error_with_clear_message(self):
        class MyHive(HivemindBot):
            name = "Hive"
            author = "Test"

            def think(self, state, ctx):
                return {Player.PLAYER1: Direction.UP, Player.PLAYER2: Direction.DOWN}

        with pytest.raises(RuntimeError, match="not supported"):
            MyHive().run()


def _extract_actions(conn: MockConnection) -> list[tuple[int, int]]:
    """Extract (direction, player) pairs from Action frames in conn.sent."""
    actions = []
    for frame in conn.sent:
        msg = parse_bot_frame(frame)
        if msg.get("kind") == "Action":
            actions.append((msg["direction"], msg["player"]))
    return actions


class TestHivemindHappyPath:
    def test_returns_both_actions(self):
        class MyHive(HivemindBot):
            name = "Hive"
            author = "Test"

            def think(self, state, ctx):
                return {Player.PLAYER1: Direction.UP, Player.PLAYER2: Direction.DOWN}

        bot = MyHive()
        frames = make_lifecycle_frames(turn_count=1)
        conn = MockConnection(frames)
        _run_lifecycle(
            conn,
            "",
            bot=bot,
            preprocess_fn=bot.preprocess,
            turn_fn=bot._handle_turn,
        )

        actions = _extract_actions(conn)
        assert len(actions) == 2
        assert (0, 0) in actions  # UP, PLAYER1
        assert (2, 1) in actions  # DOWN, PLAYER2


class TestHivemindMissingKey:
    def test_missing_player_defaults_to_stay(self):
        class PartialHive(HivemindBot):
            name = "Partial"
            author = "Test"

            def think(self, state, ctx):
                return {Player.PLAYER1: Direction.UP}

        bot = PartialHive()
        frames = make_lifecycle_frames(turn_count=1)
        conn = MockConnection(frames)
        _run_lifecycle(
            conn,
            "",
            bot=bot,
            preprocess_fn=bot.preprocess,
            turn_fn=bot._handle_turn,
        )

        actions = _extract_actions(conn)
        assert len(actions) == 2
        assert (0, 0) in actions  # UP, PLAYER1
        assert (4, 1) in actions  # STAY, PLAYER2


class TestHivemindNonDictReturn:
    def test_non_dict_defaults_to_stay(self, capsys):
        class BadHive(HivemindBot):
            name = "Bad"
            author = "Test"

            def think(self, state, ctx):
                return [Direction.UP, Direction.DOWN]

        bot = BadHive()
        frames = make_lifecycle_frames(turn_count=1)
        conn = MockConnection(frames)
        _run_lifecycle(
            conn,
            "",
            bot=bot,
            preprocess_fn=bot.preprocess,
            turn_fn=bot._handle_turn,
        )

        actions = _extract_actions(conn)
        assert len(actions) == 2
        assert (4, 0) in actions  # STAY, PLAYER1
        assert (4, 1) in actions  # STAY, PLAYER2
        assert "expected dict" in capsys.readouterr().err


class TestHivemindException:
    def test_exception_defaults_to_stay(self):
        class CrashHive(HivemindBot):
            name = "Crash"
            author = "Test"

            def think(self, state, ctx):
                raise ValueError("oops")

        bot = CrashHive()
        frames = make_lifecycle_frames(turn_count=1)
        conn = MockConnection(frames)
        _run_lifecycle(
            conn,
            "",
            bot=bot,
            preprocess_fn=bot.preprocess,
            turn_fn=bot._handle_turn,
        )

        actions = _extract_actions(conn)
        assert len(actions) == 2
        assert (4, 0) in actions  # STAY, PLAYER1
        assert (4, 1) in actions  # STAY, PLAYER2
