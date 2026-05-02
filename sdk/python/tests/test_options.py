"""Tests for UCI-style bot options — descriptors, wire conversion, lifecycle."""

from __future__ import annotations

import pytest
from conftest import MockConnection, make_lifecycle_frames

from pyrat_sdk._engine import parse_bot_frame
from pyrat_sdk._wire import codec
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
        assert depth["option_type"] == 1  # Spin
        assert depth["default_value"] == "3"
        assert depth["min"] == 1
        assert depth["max"] == 10
        assert depth["choices"] == []

        avoid_mud = by_name["avoid_mud"]
        assert avoid_mud["option_type"] == 0  # Check
        assert avoid_mud["default_value"] == "true"

        strategy = by_name["strategy"]
        assert strategy["option_type"] == 2  # Combo
        assert strategy["choices"] == ["greedy", "defensive", "random"]

        model_path = by_name["model_path"]
        assert model_path["option_type"] == 3  # String
        assert model_path["default_value"] == ""

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
# 6. Codec roundtrip — Identify
# ══════════════════════════════════════════════════════════


class TestCodecRoundtrip:
    def test_encode_identify_with_options(self):
        opts = collect_options(FakeBot)
        wire = options_to_wire(opts)
        msg = parse_bot_frame(codec.encode_identify("TestBot", "Tester", "agent", wire))

        assert msg["kind"] == "Identify"
        assert msg["name"] == "TestBot"
        assert msg["author"] == "Tester"
        assert msg["agent_id"] == "agent"
        assert {o["name"] for o in msg["options"]} == {
            "depth",
            "avoid_mud",
            "strategy",
            "model_path",
        }

    def test_encode_identify_without_options(self):
        msg = parse_bot_frame(codec.encode_identify("Plain", "Nobody"))
        assert msg["kind"] == "Identify"
        assert msg["options"] == []


# ══════════════════════════════════════════════════════════
# 7. Lifecycle integration (mock connection)
# ══════════════════════════════════════════════════════════


class TestLifecycleIntegration:
    def test_configure_options_applied(self):
        bot = FakeBot()
        assert bot.depth == 3

        frames = make_lifecycle_frames(configure_options=[("depth", "7")])
        conn = MockConnection(frames)
        _run_lifecycle(
            conn,
            "",
            bot=bot,
            preprocess_fn=lambda state, ctx: None,
            turn_fn=lambda state, ctx, c: None,
        )
        assert bot.depth == 7

    def test_defaults_preserved_without_configure_options(self):
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
        """Each Go frame in the lifecycle invokes turn_fn once."""
        bot = PlainBot()
        calls = []

        def turn_fn(state, ctx, conn):
            calls.append(state.turn)

        frames = make_lifecycle_frames(turn_count=2)
        conn = MockConnection(frames)
        _run_lifecycle(
            conn,
            "",
            bot=bot,
            preprocess_fn=lambda state, ctx: None,
            turn_fn=turn_fn,
        )
        # State stays at turn 0 across consecutive Go frames (no Advance between
        # them in this simple lifecycle), so we only check the count.
        assert len(calls) == 2


# ══════════════════════════════════════════════════════════
# 8. Bot._handle_turn and _validate_direction
# ══════════════════════════════════════════════════════════


class TestHandleTurn:
    def test_think_raises_sends_stay(self):
        """If think() raises, STAY is sent."""
        from pyrat_sdk.bot import Bot

        class CrashBot(Bot):
            name = "Crash"
            author = "Test"

            def think(self, state, ctx):
                raise RuntimeError("boom")

        bot = CrashBot()
        frames = make_lifecycle_frames(turn_count=1)
        conn = MockConnection(frames)
        _run_lifecycle(
            conn,
            "",
            bot=bot,
            preprocess_fn=bot.preprocess,
            turn_fn=bot._handle_turn,
        )

        # Find the Action frame.
        actions = [parse_bot_frame(f) for f in conn.sent]
        action = next(a for a in actions if a.get("kind") == "Action")
        assert action["direction"] == 4  # STAY


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
