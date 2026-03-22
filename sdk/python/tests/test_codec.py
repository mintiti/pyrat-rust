"""Tests for FlatBuffers codec — decode host packets, encode bot packets."""

from __future__ import annotations

import flatbuffers

from pyrat_sdk._wire import codec
from pyrat_sdk._wire.protocol import GameOver as GameOverMod
from pyrat_sdk._wire.protocol import HostPacket as HostPacketMod
from pyrat_sdk._wire.protocol import MatchConfig as MatchConfigMod
from pyrat_sdk._wire.protocol import Mud as MudMod
from pyrat_sdk._wire.protocol import Timeout as TimeoutMod
from pyrat_sdk._wire.protocol import TurnState as TurnStateMod
from pyrat_sdk._wire.protocol import Wall as WallMod
from pyrat_sdk._wire.protocol.Action import Action
from pyrat_sdk._wire.protocol.BotMessage import BotMessage
from pyrat_sdk._wire.protocol.BotPacket import BotPacket
from pyrat_sdk._wire.protocol.HostMessage import HostMessage
from pyrat_sdk._wire.protocol.Info import Info
from pyrat_sdk._wire.protocol.Vec2 import CreateVec2

# ── Helpers ───────────────────────────────────────────


def _build_host_packet(msg_type: int, build_fn) -> bytes:
    builder = flatbuffers.Builder(512)
    msg_offset = build_fn(builder)
    HostPacketMod.Start(builder)
    HostPacketMod.AddMessageType(builder, msg_type)
    HostPacketMod.AddMessage(builder, msg_offset)
    packet = HostPacketMod.End(builder)
    builder.Finish(packet)
    return bytes(builder.Output())


# ══════════════════════════════════════════════════════════
# 1. extract_match_config
# ══════════════════════════════════════════════════════════


class TestExtractMatchConfig:
    def test_full_config(self):
        """Build a MatchConfig with walls, mud, cheese, player starts — verify every field."""

        def build(b):
            # Wall between (0,0) and (0,1)
            WallMod.Start(b)
            WallMod.AddPos1(b, CreateVec2(b, 0, 0))
            WallMod.AddPos2(b, CreateVec2(b, 0, 1))
            wall = WallMod.End(b)

            MatchConfigMod.StartWallsVector(b, 1)
            b.PrependUOffsetTRelative(wall)
            walls_vec = b.EndVector()

            # Mud between (1,0) and (1,1) with cost 3
            MudMod.Start(b)
            MudMod.AddPos1(b, CreateVec2(b, 1, 0))
            MudMod.AddPos2(b, CreateVec2(b, 1, 1))
            MudMod.AddValue(b, 3)
            mud = MudMod.End(b)

            MatchConfigMod.StartMudVector(b, 1)
            b.PrependUOffsetTRelative(mud)
            mud_vec = b.EndVector()

            # Cheese at (2,1) and (1,2)
            MatchConfigMod.StartCheeseVector(b, 2)
            CreateVec2(b, 1, 2)
            CreateVec2(b, 2, 1)
            cheese_vec = b.EndVector()

            # Controlled players: [0]
            MatchConfigMod.StartControlledPlayersVector(b, 1)
            b.PrependUint8(0)
            cp_vec = b.EndVector()

            MatchConfigMod.Start(b)
            MatchConfigMod.AddWidth(b, 5)
            MatchConfigMod.AddHeight(b, 4)
            MatchConfigMod.AddMaxTurns(b, 200)
            MatchConfigMod.AddWalls(b, walls_vec)
            MatchConfigMod.AddMud(b, mud_vec)
            MatchConfigMod.AddCheese(b, cheese_vec)
            MatchConfigMod.AddPlayer1Start(b, CreateVec2(b, 0, 0))
            MatchConfigMod.AddPlayer2Start(b, CreateVec2(b, 4, 3))
            MatchConfigMod.AddControlledPlayers(b, cp_vec)
            MatchConfigMod.AddMoveTimeoutMs(b, 500)
            MatchConfigMod.AddPreprocessingTimeoutMs(b, 2000)
            return MatchConfigMod.End(b)

        buf = _build_host_packet(HostMessage.MatchConfig, build)
        _, table = codec.decode_host_packet(buf)
        config = codec.extract_match_config(table)

        assert config["width"] == 5
        assert config["height"] == 4
        assert config["max_turns"] == 200
        assert config["player1_start"] == (0, 0)
        assert config["player2_start"] == (4, 3)
        assert config["controlled_players"] == [0]
        assert config["move_timeout_ms"] == 500
        assert config["preprocessing_timeout_ms"] == 2000

        assert len(config["walls"]) == 1
        assert config["walls"][0] == ((0, 0), (0, 1))

        assert len(config["mud"]) == 1
        p1, p2, cost = config["mud"][0]
        assert p1 == (1, 0)
        assert p2 == (1, 1)
        assert cost == 3

        assert set(config["cheese"]) == {(2, 1), (1, 2)}


# ══════════════════════════════════════════════════════════
# 2. extract_turn_state
# ══════════════════════════════════════════════════════════


class TestExtractTurnState:
    def test_non_trivial_values(self):
        def build(b):
            TurnStateMod.StartCheeseVector(b, 2)
            CreateVec2(b, 3, 1)
            CreateVec2(b, 2, 0)
            cheese_vec = b.EndVector()

            TurnStateMod.Start(b)
            TurnStateMod.AddTurn(b, 42)
            TurnStateMod.AddPlayer1Position(b, CreateVec2(b, 1, 2))
            TurnStateMod.AddPlayer2Position(b, CreateVec2(b, 3, 0))
            TurnStateMod.AddPlayer1Score(b, 5.5)
            TurnStateMod.AddPlayer2Score(b, 3.0)
            TurnStateMod.AddPlayer1MudTurns(b, 2)
            TurnStateMod.AddPlayer2MudTurns(b, 0)
            TurnStateMod.AddPlayer1LastMove(b, 0)  # UP
            TurnStateMod.AddPlayer2LastMove(b, 3)  # LEFT
            TurnStateMod.AddCheese(b, cheese_vec)
            return TurnStateMod.End(b)

        buf = _build_host_packet(HostMessage.TurnState, build)
        _, table = codec.decode_host_packet(buf)
        ts = codec.extract_turn_state(table)

        assert ts["turn"] == 42
        assert ts["player1_position"] == (1, 2)
        assert ts["player2_position"] == (3, 0)
        assert ts["player1_score"] == 5.5
        assert ts["player2_score"] == 3.0
        assert ts["player1_mud_turns"] == 2
        assert ts["player2_mud_turns"] == 0
        assert ts["player1_last_move"] == 0
        assert ts["player2_last_move"] == 3
        assert set(ts["cheese"]) == {(2, 0), (3, 1)}


# ══════════════════════════════════════════════════════════
# 3. extract_game_over
# ══════════════════════════════════════════════════════════


class TestExtractGameOver:
    def test_result_and_scores(self):
        def build(b):
            GameOverMod.Start(b)
            GameOverMod.AddResult(b, 1)  # Player1Won
            GameOverMod.AddPlayer1Score(b, 10.0)
            GameOverMod.AddPlayer2Score(b, 3.5)
            return GameOverMod.End(b)

        buf = _build_host_packet(HostMessage.GameOver, build)
        _, table = codec.decode_host_packet(buf)
        go = codec.extract_game_over(table)

        assert go["result"] == 1
        assert go["player1_score"] == 10.0
        assert go["player2_score"] == 3.5


# ══════════════════════════════════════════════════════════
# 4. extract_timeout
# ══════════════════════════════════════════════════════════


class TestExtractTimeout:
    def test_default_move(self):
        def build(b):
            TimeoutMod.Start(b)
            TimeoutMod.AddDefaultMove(b, 4)  # STAY
            return TimeoutMod.End(b)

        buf = _build_host_packet(HostMessage.Timeout, build)
        _, table = codec.decode_host_packet(buf)
        default_move = codec.extract_timeout(table)
        assert default_move == 4


# ══════════════════════════════════════════════════════════
# 5. encode_action roundtrip
# ══════════════════════════════════════════════════════════


class TestEncodeAction:
    def test_roundtrip(self):
        buf = codec.encode_action(direction=2, player=1, turn=42)  # DOWN, PLAYER2
        packet = BotPacket.GetRootAs(buf)
        assert packet.MessageType() == BotMessage.Action

        action = Action()
        action.Init(packet.Message().Bytes, packet.Message().Pos)
        assert action.Direction() == 2
        assert action.Player() == 1
        assert action.Turn() == 42

    def test_stay(self):
        buf = codec.encode_action(direction=4, player=0)
        packet = BotPacket.GetRootAs(buf)
        action = Action()
        action.Init(packet.Message().Bytes, packet.Message().Pos)
        assert action.Direction() == 4
        assert action.Player() == 0
        assert action.Turn() == 0


# ══════════════════════════════════════════════════════════
# 6. encode_info roundtrip
# ══════════════════════════════════════════════════════════


class TestEncodeInfo:
    def test_all_fields(self):
        buf = codec.encode_info(
            player=1,
            multipv=2,
            target=(5, 3),
            depth=10,
            nodes=1000,
            score=42.5,
            pv=[0, 3],  # UP, LEFT
            message="hello",
        )
        packet = BotPacket.GetRootAs(buf)
        assert packet.MessageType() == BotMessage.Info

        info = Info()
        info.Init(packet.Message().Bytes, packet.Message().Pos)

        assert info.Player() == 1
        assert info.Multipv() == 2
        assert info.Target().X() == 5
        assert info.Target().Y() == 3
        assert info.Depth() == 10
        assert info.Nodes() == 1000
        assert info.Score() == 42.5
        assert info.PvLength() == 2
        assert info.Pv(0) == 0  # UP
        assert info.Pv(1) == 3  # LEFT
        msg = info.Message()
        assert (msg.decode("utf-8") if isinstance(msg, bytes) else msg) == "hello"

    def test_minimal(self):
        """Only default values — no target, pv, or message."""
        buf = codec.encode_info()
        packet = BotPacket.GetRootAs(buf)
        info = Info()
        info.Init(packet.Message().Bytes, packet.Message().Pos)

        assert info.Player() == 0
        assert info.Multipv() == 0
        assert info.Target() is None
        assert info.Depth() == 0
        assert info.Nodes() == 0
        assert info.PvIsNone()

    def test_with_target_only(self):
        buf = codec.encode_info(target=(7, 2))
        packet = BotPacket.GetRootAs(buf)
        info = Info()
        info.Init(packet.Message().Bytes, packet.Message().Pos)

        assert info.Target().X() == 7
        assert info.Target().Y() == 2
        assert info.PvIsNone()
