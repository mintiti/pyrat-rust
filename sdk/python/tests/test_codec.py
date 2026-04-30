"""Tests for the codec — kind-tagged dicts ↔ wire frames.

Each variant round-trips through ``serialize_*`` then ``parse_*`` to verify
the dict shape stays stable across the bridge.
"""

from __future__ import annotations

from conftest import minimal_match_config, turn_state

from pyrat_sdk._engine import (
    parse_bot_frame,
    parse_host_frame,
    serialize_bot_msg,
    serialize_host_msg,
)
from pyrat_sdk._wire import codec


def _full_match_config() -> dict:
    """A non-trivial MatchConfig exercising walls, mud, multiple cheeses."""
    return {
        "width": 5,
        "height": 4,
        "max_turns": 100,
        "walls": [((0, 0), (0, 1)), ((2, 2), (3, 2))],
        "mud": [((1, 0), (1, 1), 3)],
        "cheese": [(0, 0), (2, 2), (4, 3)],
        "player1_start": (0, 0),
        "player2_start": (4, 3),
        "timing": 1,  # Clock
        "move_timeout_ms": 250,
        "preprocessing_timeout_ms": 5_000,
    }


# ══════════════════════════════════════════════════════════
# 1. HostMsg round-trips
# ══════════════════════════════════════════════════════════


class TestHostMsgRoundTrip:
    def _round_trip(self, msg: dict) -> dict:
        return parse_host_frame(serialize_host_msg(msg))

    def test_welcome(self):
        msg = {"kind": "Welcome", "player_slot": 1}
        assert self._round_trip(msg) == msg

    def test_configure(self):
        msg = {
            "kind": "Configure",
            "options": [("depth", "5"), ("avoid_mud", "true")],
            "match_config": _full_match_config(),
        }
        got = self._round_trip(msg)
        assert got["kind"] == "Configure"
        assert got["options"] == [("depth", "5"), ("avoid_mud", "true")]
        assert got["match_config"] == _full_match_config()

    def test_go_preprocess(self):
        msg = {"kind": "GoPreprocess", "state_hash": 0xDEAD_BEEF}
        assert self._round_trip(msg) == msg

    def test_advance(self):
        msg = {
            "kind": "Advance",
            "p1_dir": 1,
            "p2_dir": 4,
            "turn": 7,
            "new_hash": 0xCAFE_BABE,
        }
        assert self._round_trip(msg) == msg

    def test_go(self):
        msg = {
            "kind": "Go",
            "state_hash": 0xABCD,
            "limits": {"timeout_ms": 100, "depth": None, "nodes": 10_000},
        }
        assert self._round_trip(msg) == msg

    def test_go_state(self):
        msg = {
            "kind": "GoState",
            "turn_state": turn_state(turn=5),
            "state_hash": 0x1234,
            "limits": {"timeout_ms": None, "depth": 4, "nodes": None},
        }
        got = self._round_trip(msg)
        assert got["kind"] == "GoState"
        assert got["state_hash"] == 0x1234
        assert got["turn_state"] == turn_state(turn=5)
        assert got["limits"] == {"timeout_ms": None, "depth": 4, "nodes": None}

    def test_stop(self):
        msg = {"kind": "Stop"}
        assert self._round_trip(msg) == msg

    def test_full_state(self):
        msg = {
            "kind": "FullState",
            "match_config": _full_match_config(),
            "turn_state": turn_state(turn=3),
        }
        got = self._round_trip(msg)
        assert got["kind"] == "FullState"
        assert got["match_config"] == _full_match_config()
        assert got["turn_state"] == turn_state(turn=3)

    def test_protocol_error(self):
        msg = {"kind": "ProtocolError", "reason": "bad welcome order"}
        assert self._round_trip(msg) == msg

    def test_game_over(self):
        msg = {
            "kind": "GameOver",
            "result": 2,
            "player1_score": 3.5,
            "player2_score": 3.5,
        }
        assert self._round_trip(msg) == msg


# ══════════════════════════════════════════════════════════
# 2. BotMsg round-trips
# ══════════════════════════════════════════════════════════


class TestBotMsgRoundTrip:
    def _round_trip(self, msg: dict) -> dict:
        return parse_bot_frame(serialize_bot_msg(msg))

    def test_identify_no_options(self):
        msg = {
            "kind": "Identify",
            "name": "TestBot",
            "author": "Tester",
            "agent_id": "test/bot",
            "options": [],
        }
        assert self._round_trip(msg) == msg

    def test_identify_with_options(self):
        opt = {
            "name": "depth",
            "option_type": 1,  # Spin
            "default_value": "3",
            "min": 1,
            "max": 10,
            "choices": [],
        }
        msg = {
            "kind": "Identify",
            "name": "B",
            "author": "A",
            "agent_id": "",
            "options": [opt],
        }
        got = self._round_trip(msg)
        assert got["options"] == [opt]

    def test_ready(self):
        msg = {"kind": "Ready", "state_hash": 0xC001}
        assert self._round_trip(msg) == msg

    def test_preprocessing_done(self):
        msg = {"kind": "PreprocessingDone"}
        assert self._round_trip(msg) == msg

    def test_action(self):
        msg = {
            "kind": "Action",
            "direction": 0,
            "player": 1,
            "turn": 12,
            "state_hash": 0xBEEF,
            "think_ms": 47,
        }
        assert self._round_trip(msg) == msg

    def test_provisional(self):
        msg = {
            "kind": "Provisional",
            "direction": 2,
            "player": 0,
            "turn": 3,
            "state_hash": 0xFACE,
        }
        assert self._round_trip(msg) == msg

    def test_sync_ok(self):
        msg = {"kind": "SyncOk", "hash": 0x1111}
        assert self._round_trip(msg) == msg

    def test_resync(self):
        msg = {"kind": "Resync", "my_hash": 0x2222}
        assert self._round_trip(msg) == msg

    def test_info_minimal(self):
        msg = {
            "kind": "Info",
            "player": 0,
            "multipv": 0,
            "target": None,
            "depth": 0,
            "nodes": 0,
            "score": None,
            "pv": [],
            "message": "",
            "turn": 0,
            "state_hash": 0,
        }
        assert self._round_trip(msg) == msg

    def test_info_full(self):
        msg = {
            "kind": "Info",
            "player": 1,
            "multipv": 2,
            "target": (3, 4),
            "depth": 5,
            "nodes": 10_000,
            "score": 1.5,
            "pv": [0, 1, 2, 4],
            "message": "depth 5",
            "turn": 8,
            "state_hash": 0xDEAD,
        }
        assert self._round_trip(msg) == msg

    def test_render_commands(self):
        msg = {
            "kind": "RenderCommands",
            "player": 1,
            "turn": 5,
            "state_hash": 0xAAAA,
        }
        assert self._round_trip(msg) == msg


# ══════════════════════════════════════════════════════════
# 3. Encoder helpers (codec.encode_*)
# ══════════════════════════════════════════════════════════


class TestEncoderHelpers:
    def test_encode_identify(self):
        opts = [
            {
                "name": "depth",
                "option_type": 1,
                "default_value": "3",
                "min": 1,
                "max": 10,
                "choices": [],
            }
        ]
        buf = codec.encode_identify("Bot", "Auth", "agent-x", opts)
        msg = parse_bot_frame(buf)
        assert msg["kind"] == "Identify"
        assert msg["name"] == "Bot"
        assert msg["author"] == "Auth"
        assert msg["agent_id"] == "agent-x"
        assert msg["options"] == opts

    def test_encode_identify_default_options(self):
        buf = codec.encode_identify("Plain", "Nobody")
        msg = parse_bot_frame(buf)
        assert msg["options"] == []

    def test_encode_ready(self):
        msg = parse_bot_frame(codec.encode_ready(0x42))
        assert msg == {"kind": "Ready", "state_hash": 0x42}

    def test_encode_preprocessing_done(self):
        msg = parse_bot_frame(codec.encode_preprocessing_done())
        assert msg == {"kind": "PreprocessingDone"}

    def test_encode_action(self):
        msg = parse_bot_frame(
            codec.encode_action(direction=1, player=0, turn=3, state_hash=0xCC, think_ms=12)
        )
        assert msg == {
            "kind": "Action",
            "direction": 1,
            "player": 0,
            "turn": 3,
            "state_hash": 0xCC,
            "think_ms": 12,
        }

    def test_encode_provisional(self):
        msg = parse_bot_frame(
            codec.encode_provisional(direction=2, player=1, turn=5, state_hash=0x99)
        )
        assert msg == {
            "kind": "Provisional",
            "direction": 2,
            "player": 1,
            "turn": 5,
            "state_hash": 0x99,
        }

    def test_encode_sync_ok(self):
        msg = parse_bot_frame(codec.encode_sync_ok(0x77))
        assert msg == {"kind": "SyncOk", "hash": 0x77}

    def test_encode_resync(self):
        msg = parse_bot_frame(codec.encode_resync(0x88))
        assert msg == {"kind": "Resync", "my_hash": 0x88}

    def test_encode_info(self):
        msg = parse_bot_frame(
            codec.encode_info(
                player=0,
                target=(2, 3),
                depth=4,
                pv=[0, 1],
                score=0.5,
                message="hi",
                turn=7,
                state_hash=0xAB,
            )
        )
        assert msg["kind"] == "Info"
        assert msg["player"] == 0
        assert msg["target"] == (2, 3)
        assert msg["depth"] == 4
        assert msg["pv"] == [0, 1]
        assert msg["score"] == 0.5
        assert msg["message"] == "hi"
        assert msg["turn"] == 7
        assert msg["state_hash"] == 0xAB


# ══════════════════════════════════════════════════════════
# 4. minimal_match_config fixture parses cleanly
# ══════════════════════════════════════════════════════════


def test_minimal_match_config_round_trips():
    """The conftest minimal config should round-trip without changes."""
    msg = {
        "kind": "Configure",
        "options": [],
        "match_config": minimal_match_config(),
    }
    got = parse_host_frame(serialize_host_msg(msg))
    assert got["match_config"] == minimal_match_config()
