"""Tests for HivemindBot — multi-player action dispatch and error handling."""

from __future__ import annotations

from conftest import MockConnection, make_lifecycle_frames
from pyrat.protocol.Action import Action
from pyrat.protocol.BotMessage import BotMessage
from pyrat.protocol.BotPacket import BotPacket

from pyrat_sdk.bot import HivemindBot, _run_lifecycle
from pyrat_sdk.state import Direction, Player


def _extract_actions(conn: MockConnection) -> list[tuple[int, int]]:
    """Extract (direction, player) pairs from Action frames in conn.sent."""
    actions = []
    for frame in conn.sent:
        packet = BotPacket.GetRootAs(frame)
        if packet.MessageType() == BotMessage.Action:
            action = Action()
            action.Init(packet.Message().Bytes, packet.Message().Pos)
            actions.append((action.Direction(), action.Player()))
    return actions


class TestHivemindHappyPath:
    def test_returns_both_actions(self):
        class MyHive(HivemindBot):
            name = "Hive"
            author = "Test"

            def think(self, state, ctx):
                return {Player.PLAYER1: Direction.UP, Player.PLAYER2: Direction.DOWN}

        bot = MyHive()
        frames = make_lifecycle_frames(
            controlled_player=0,
            turn_states=1,
        )
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
        # Player 0 (PLAYER1) gets UP (0), Player 1 (PLAYER2) gets DOWN (2)
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
        frames = make_lifecycle_frames(turn_states=1)
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
        frames = make_lifecycle_frames(turn_states=1)
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
        frames = make_lifecycle_frames(turn_states=1)
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
