"""Tests for UCI-style bot options — descriptors, wire conversion, lifecycle."""

from __future__ import annotations

import pytest
from conftest import (
    MockConnection,
    build_host_packet,
    build_set_option,
    make_lifecycle_frames,
)

from pyrat_sdk._wire import codec
from pyrat_sdk._wire.protocol.BotMessage import BotMessage
from pyrat_sdk._wire.protocol.BotPacket import BotPacket
from pyrat_sdk._wire.protocol.HostMessage import HostMessage
from pyrat_sdk._wire.protocol.Identify import Identify
from pyrat_sdk.bot import _run_lifecycle, _validate_direction
from pyrat_sdk.options import (
    Check,
    Combo,
    Spin,
    Str,
    apply_set_option,
    collect_options,
    options_to_wire,
)
from pyrat_sdk.state import Direction

# ── Test bot classes ─────────────────────────────────────


class FakeBot:
    name = "TestBot"
    author = "Tester"
    depth = Spin(default=3, min=1, max=10)
    avoid_mud = Check(default=True)
    strategy = Combo(default="greedy", choices=["greedy", "defensive", "random"])
    model_path = Str(default="")


class PlainBot:
    """Bot with no options."""

    name = "Plain"
    author = "Nobody"


class ChildBot(FakeBot):
    """Subclass that overrides an option."""

    depth = Spin(default=5, min=1, max=20)


# ══════════════════════════════════════════════════════════
# 1. Descriptor behavior (pure Python)
# ══════════════════════════════════════════════════════════


class TestDescriptorBehavior:
    def test_class_level_returns_descriptor(self):
        assert isinstance(FakeBot.depth, Spin)
        assert isinstance(FakeBot.avoid_mud, Check)
        assert isinstance(FakeBot.strategy, Combo)
        assert isinstance(FakeBot.model_path, Str)

    def test_instance_returns_default(self):
        bot = FakeBot()
        assert bot.depth == 3
        assert bot.avoid_mud is True
        assert bot.strategy == "greedy"
        assert bot.model_path == ""

    def test_instance_assignment(self):
        bot = FakeBot()
        bot.depth = 7
        assert bot.depth == 7

    def test_two_instances_independent(self):
        a = FakeBot()
        b = FakeBot()
        a.depth = 9
        assert a.depth == 9
        assert b.depth == 3  # still default


# ══════════════════════════════════════════════════════════
# 2. Validation
# ══════════════════════════════════════════════════════════


class TestValidation:
    def test_spin_default_out_of_range(self):
        with pytest.raises(ValueError, match="not in"):
            s = Spin(default=20, min=1, max=10)
            s.name = "bad"
            s.validate_default()

    def test_spin_default_wrong_type(self):
        with pytest.raises(TypeError, match="must be int"):
            s = Spin(default=3.5, min=1, max=10)
            s.name = "bad"
            s.validate_default()

    def test_spin_bool_rejected(self):
        """bool is a subclass of int — should still be rejected."""
        with pytest.raises(TypeError, match="must be int"):
            s = Spin(default=True, min=0, max=1)
            s.name = "bad"
            s.validate_default()

    def test_combo_default_not_in_choices(self):
        with pytest.raises(ValueError, match="not in"):
            c = Combo(default="unknown", choices=["a", "b"])
            c.name = "bad"
            c.validate_default()

    def test_check_default_not_bool(self):
        with pytest.raises(TypeError, match="must be bool"):
            c = Check(default=1)
            c.name = "bad"
            c.validate_default()

    def test_str_default_not_str(self):
        with pytest.raises(TypeError, match="must be str"):
            s = Str(default=42)
            s.name = "bad"
            s.validate_default()


# ══════════════════════════════════════════════════════════
# 3. Wire string coercion
# ══════════════════════════════════════════════════════════


class TestCoercion:
    def test_spin_valid(self):
        s = Spin(default=3, min=1, max=10)
        s.name = "depth"
        assert s.coerce("5") == 5

    def test_spin_out_of_range(self):
        s = Spin(default=3, min=1, max=10)
        s.name = "depth"
        with pytest.raises(ValueError, match="not in"):
            s.coerce("20")

    def test_spin_not_a_number(self):
        s = Spin(default=3, min=1, max=10)
        s.name = "depth"
        with pytest.raises(ValueError, match="cannot convert"):
            s.coerce("abc")

    def test_check_true(self):
        c = Check(default=False)
        c.name = "flag"
        assert c.coerce("true") is True

    def test_check_false(self):
        c = Check(default=True)
        c.name = "flag"
        assert c.coerce("false") is False

    def test_check_invalid(self):
        c = Check(default=True)
        c.name = "flag"
        with pytest.raises(ValueError, match="expected 'true' or 'false'"):
            c.coerce("yes")

    def test_combo_valid(self):
        c = Combo(default="a", choices=["a", "b", "c"])
        c.name = "strat"
        assert c.coerce("b") == "b"

    def test_combo_invalid(self):
        c = Combo(default="a", choices=["a", "b", "c"])
        c.name = "strat"
        with pytest.raises(ValueError, match="not in"):
            c.coerce("z")

    def test_str_passthrough(self):
        s = Str(default="")
        s.name = "path"
        assert s.coerce("anything/goes") == "anything/goes"


# ══════════════════════════════════════════════════════════
# 4. Collection and wire conversion
# ══════════════════════════════════════════════════════════


class TestCollection:
    def test_collect_options_returns_all(self):
        opts = collect_options(FakeBot)
        assert set(opts.keys()) == {"depth", "avoid_mud", "strategy", "model_path"}
        assert isinstance(opts["depth"], Spin)
        assert isinstance(opts["avoid_mud"], Check)

    def test_collect_options_plain_bot(self):
        opts = collect_options(PlainBot)
        assert opts == {}

    def test_options_to_wire(self):
        opts = collect_options(FakeBot)
        wire = options_to_wire(opts)
        by_name = {w["name"]: w for w in wire}

        depth = by_name["depth"]
        assert depth["wire_type"] == 1
        assert depth["default_str"] == "3"
        assert depth["min"] == 1
        assert depth["max"] == 10

        avoid_mud = by_name["avoid_mud"]
        assert avoid_mud["wire_type"] == 0
        assert avoid_mud["default_str"] == "true"

        strategy = by_name["strategy"]
        assert strategy["wire_type"] == 2
        assert strategy["choices"] == ["greedy", "defensive", "random"]

        model_path = by_name["model_path"]
        assert model_path["wire_type"] == 3
        assert model_path["default_str"] == ""

    def test_inheritance_override(self):
        opts = collect_options(ChildBot)
        assert opts["depth"].max == 20
        assert opts["depth"].default == 5


# ══════════════════════════════════════════════════════════
# 5. apply_set_option
# ══════════════════════════════════════════════════════════


class TestApplySetOption:
    def test_happy_path(self):
        bot = FakeBot()
        opts = collect_options(FakeBot)
        apply_set_option(bot, opts, "depth", "7")
        assert bot.depth == 7

    def test_unknown_name(self, capsys):
        bot = FakeBot()
        opts = collect_options(FakeBot)
        apply_set_option(bot, opts, "nonexistent", "42")
        assert "unknown option" in capsys.readouterr().err

    def test_invalid_value(self, capsys):
        bot = FakeBot()
        opts = collect_options(FakeBot)
        apply_set_option(bot, opts, "depth", "abc")
        assert bot.depth == 3  # default preserved
        assert "keeping default" in capsys.readouterr().err


# ══════════════════════════════════════════════════════════
# 6. Codec roundtrip
# ══════════════════════════════════════════════════════════


class TestCodecRoundtrip:
    def test_encode_identify_with_options(self):
        opts = collect_options(FakeBot)
        wire = options_to_wire(opts)
        buf = codec.encode_identify("TestBot", "Tester", "", wire)

        # Decode the BotPacket to get to the Identify table.
        packet = BotPacket.GetRootAs(buf)
        assert packet.MessageType() == BotMessage.Identify

        ident = Identify()
        ident.Init(packet.Message().Bytes, packet.Message().Pos)

        assert ident.Name() == b"TestBot"
        assert ident.Author() == b"Tester"
        assert not ident.OptionsIsNone()
        assert ident.OptionsLength() == len(wire)

        # Verify each OptionDef.
        seen = set()
        for i in range(ident.OptionsLength()):
            od = ident.Options(i)
            name = od.Name()
            if isinstance(name, bytes):
                name = name.decode("utf-8")
            seen.add(name)

        assert seen == {"depth", "avoid_mud", "strategy", "model_path"}

    def test_encode_identify_without_options(self):
        buf = codec.encode_identify("Plain", "Nobody", "")

        packet = BotPacket.GetRootAs(buf)
        ident = Identify()
        ident.Init(packet.Message().Bytes, packet.Message().Pos)

        assert ident.OptionsIsNone()

    def test_extract_set_option(self):
        buf = build_host_packet(
            HostMessage.SetOption,
            lambda b: build_set_option(b, "depth", "7"),
        )
        msg_type, table = codec.decode_host_packet(buf)
        assert msg_type == HostMessage.SetOption
        name, value = codec.extract_set_option(table)
        assert name == "depth"
        assert value == "7"


# ══════════════════════════════════════════════════════════
# 7. Lifecycle integration (mock connection)
# ══════════════════════════════════════════════════════════


class TestLifecycleIntegration:
    def test_set_option_applied(self):
        bot = FakeBot()
        assert bot.depth == 3  # default

        frames = make_lifecycle_frames(set_options=[("depth", "7")])
        conn = MockConnection(frames)
        _run_lifecycle(
            conn,
            "",
            bot=bot,
            preprocess_fn=lambda state, ctx: None,
            turn_fn=lambda state, ctx, c: None,
        )
        assert bot.depth == 7

    def test_defaults_preserved_without_set_option(self):
        bot = FakeBot()
        frames = make_lifecycle_frames()
        conn = MockConnection(frames)
        _run_lifecycle(
            conn,
            "",
            bot=bot,
            preprocess_fn=lambda state, ctx: None,
            turn_fn=lambda state, ctx, c: None,
        )
        assert bot.depth == 3
        assert bot.avoid_mud is True
        assert bot.strategy == "greedy"

    def test_turn_fn_called(self):
        """TurnState frame before GameOver triggers turn_fn."""
        bot = PlainBot()
        calls = []

        def turn_fn(state, ctx, conn):
            calls.append(state.turn)

        frames = make_lifecycle_frames(turn_states=2)
        conn = MockConnection(frames)
        _run_lifecycle(
            conn,
            "",
            bot=bot,
            preprocess_fn=lambda state, ctx: None,
            turn_fn=turn_fn,
        )
        assert calls == [1, 2]

    def test_ping_gets_pong(self):
        """Ping frame triggers a Pong response."""
        bot = PlainBot()
        frames = make_lifecycle_frames(include_ping=True)
        conn = MockConnection(frames)
        _run_lifecycle(
            conn,
            "",
            bot=bot,
            preprocess_fn=lambda state, ctx: None,
            turn_fn=lambda state, ctx, c: None,
        )
        # Find the Pong in sent frames (after Identify, Ready, PreprocessingDone).
        pong_found = False
        for frame in conn.sent:
            packet = BotPacket.GetRootAs(frame)
            if packet.MessageType() == BotMessage.Pong:
                pong_found = True
                break
        assert pong_found


# ══════════════════════════════════════════════════════════
# 8. Bot._handle_turn and _validate_direction
# ══════════════════════════════════════════════════════════


class TestHandleTurn:
    def test_think_raises_sends_stay(self):
        """If think() raises, STAY is sent."""
        from pyrat_sdk._wire.protocol.Action import Action
        from pyrat_sdk.bot import Bot

        class CrashBot(Bot):
            name = "Crash"
            author = "Test"

            def think(self, state, ctx):
                raise RuntimeError("boom")

        bot = CrashBot()
        frames = make_lifecycle_frames(turn_states=1)
        conn = MockConnection(frames)
        _run_lifecycle(
            conn,
            "",
            bot=bot,
            preprocess_fn=bot.preprocess,
            turn_fn=bot._handle_turn,
        )
        # Last sent frame should be an Action with STAY (4).
        action_frame = conn.sent[-1]
        packet = BotPacket.GetRootAs(action_frame)
        assert packet.MessageType() == BotMessage.Action
        action = Action()
        action.Init(packet.Message().Bytes, packet.Message().Pos)
        assert action.Direction() == 4  # STAY


class TestValidateDirection:
    def test_direction_enum(self):
        assert _validate_direction(Direction.UP, "test") == Direction.UP
        assert _validate_direction(Direction.STAY, "test") == Direction.STAY

    def test_raw_int_0_to_4(self):
        for i in range(5):
            assert _validate_direction(i, "test") == Direction(i)

    def test_out_of_range_defaults_to_stay(self):
        assert _validate_direction(99, "test") == Direction.STAY

    def test_none_defaults_to_stay(self):
        assert _validate_direction(None, "test") == Direction.STAY

    def test_string_defaults_to_stay(self):
        assert _validate_direction("UP", "test") == Direction.STAY
