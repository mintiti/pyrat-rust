"""FlatBuffers codec — decode host packets, encode bot packets.

Hides the generated-code boilerplate so the rest of the SDK works with
plain Python types.
"""

from __future__ import annotations

import flatbuffers

from pyrat_sdk._wire.protocol import Action as ActionMod
from pyrat_sdk._wire.protocol import BotPacket as BotPacketMod
from pyrat_sdk._wire.protocol import Identify as IdentifyMod
from pyrat_sdk._wire.protocol import Info as InfoMod
from pyrat_sdk._wire.protocol import OptionDef as OptionDefMod
from pyrat_sdk._wire.protocol import Pong as PongMod
from pyrat_sdk._wire.protocol import PreprocessingDone as PreprocessingDoneMod
from pyrat_sdk._wire.protocol import Ready as ReadyMod
from pyrat_sdk._wire.protocol.BotMessage import BotMessage
from pyrat_sdk._wire.protocol.GameOver import GameOver as FBGameOver
from pyrat_sdk._wire.protocol.HostPacket import HostPacket
from pyrat_sdk._wire.protocol.MatchConfig import MatchConfig as FBMatchConfig
from pyrat_sdk._wire.protocol.SetOption import SetOption as FBSetOption
from pyrat_sdk._wire.protocol.Timeout import Timeout as FBTimeout
from pyrat_sdk._wire.protocol.TurnState import TurnState as FBTurnState
from pyrat_sdk._wire.protocol.Vec2 import CreateVec2

# ---------------------------------------------------------------------------
# Decoding (host → bot)
# ---------------------------------------------------------------------------


def decode_host_packet(buf: bytes) -> tuple[int, object]:
    """Parse a raw frame as a HostPacket.

    Returns ``(HostMessage type int, union table)`` where the union table
    is the raw flatbuffers Table from ``packet.Message()``.  Callers use
    ``extract_*`` helpers to get typed access.
    """
    packet = HostPacket.GetRootAs(buf)
    return packet.MessageType(), packet.Message()


def extract_match_config(table) -> dict:
    """Convert a raw union Table into a dict of match config values."""
    mc = FBMatchConfig()
    mc.Init(table.Bytes, table.Pos)

    walls = []
    for i in range(mc.WallsLength()):
        w = mc.Walls(i)
        p1, p2 = w.Pos1(), w.Pos2()
        walls.append(((p1.X(), p1.Y()), (p2.X(), p2.Y())))

    mud = []
    for i in range(mc.MudLength()):
        m = mc.Mud(i)
        p1, p2 = m.Pos1(), m.Pos2()
        mud.append(((p1.X(), p1.Y()), (p2.X(), p2.Y()), m.Value()))

    cheese = []
    for i in range(mc.CheeseLength()):
        c = mc.Cheese(i)
        cheese.append((c.X(), c.Y()))

    controlled = []
    for i in range(mc.ControlledPlayersLength()):
        controlled.append(mc.ControlledPlayers(i))

    p1s = mc.Player1Start()
    p2s = mc.Player2Start()

    return {
        "width": mc.Width(),
        "height": mc.Height(),
        "max_turns": mc.MaxTurns(),
        "walls": walls,
        "mud": mud,
        "cheese": cheese,
        "player1_start": (p1s.X(), p1s.Y()) if p1s else (0, 0),
        "player2_start": (p2s.X(), p2s.Y()) if p2s else (0, 0),
        "controlled_players": controlled,
        "move_timeout_ms": mc.MoveTimeoutMs(),
        "preprocessing_timeout_ms": mc.PreprocessingTimeoutMs(),
    }


def extract_turn_state(table) -> dict:
    """Convert a raw union Table into a dict of per-turn values."""
    ts = FBTurnState()
    ts.Init(table.Bytes, table.Pos)

    cheese = []
    for i in range(ts.CheeseLength()):
        c = ts.Cheese(i)
        cheese.append((c.X(), c.Y()))

    p1 = ts.Player1Position()
    p2 = ts.Player2Position()

    return {
        "turn": ts.Turn(),
        "player1_position": (p1.X(), p1.Y()) if p1 else (0, 0),
        "player2_position": (p2.X(), p2.Y()) if p2 else (0, 0),
        "player1_score": ts.Player1Score(),
        "player2_score": ts.Player2Score(),
        "player1_mud_turns": ts.Player1MudTurns(),
        "player2_mud_turns": ts.Player2MudTurns(),
        "cheese": cheese,
        "player1_last_move": ts.Player1LastMove(),
        "player2_last_move": ts.Player2LastMove(),
    }


def extract_game_over(table) -> dict:
    go = FBGameOver()
    go.Init(table.Bytes, table.Pos)
    return {
        "result": go.Result(),
        "player1_score": go.Player1Score(),
        "player2_score": go.Player2Score(),
    }


def extract_timeout(table) -> int:
    """Return the default_move Direction int from a Timeout message."""
    t = FBTimeout()
    t.Init(table.Bytes, table.Pos)
    return t.DefaultMove()


def extract_set_option(table) -> tuple[str, str]:
    """Extract (name, value) from a SetOption union table."""
    so = FBSetOption()
    so.Init(table.Bytes, table.Pos)
    name = so.Name()
    value = so.Value()
    return (
        name.decode("utf-8") if isinstance(name, bytes) else (name or ""),
        value.decode("utf-8") if isinstance(value, bytes) else (value or ""),
    )


# ---------------------------------------------------------------------------
# Encoding (bot → host)
# ---------------------------------------------------------------------------


def _build_bot_packet(msg_type: int, build_fn) -> bytes:
    """Build a BotPacket wrapping the inner message created by *build_fn*."""
    builder = flatbuffers.Builder(256)
    msg_offset = build_fn(builder)
    BotPacketMod.Start(builder)
    BotPacketMod.AddMessageType(builder, msg_type)
    BotPacketMod.AddMessage(builder, msg_offset)
    packet = BotPacketMod.End(builder)
    builder.Finish(packet)
    return bytes(builder.Output())


def encode_identify(
    name: str,
    author: str,
    agent_id: str = "",
    options: list[dict] | None = None,
) -> bytes:
    def build(b):
        # Pre-create all strings (FlatBuffers requires strings before Start()).
        n = b.CreateString(name)
        a = b.CreateString(author)
        aid = b.CreateString(agent_id) if agent_id else None

        # Build option defs if provided.
        opts_vec = None
        if options:
            opt_offsets = []
            for opt in options:
                oname = b.CreateString(opt["name"])
                odefault = b.CreateString(opt["default_str"])
                choice_offsets = []
                for c in opt.get("choices", []):
                    choice_offsets.append(b.CreateString(c))

                choices_vec = None
                if choice_offsets:
                    OptionDefMod.StartChoicesVector(b, len(choice_offsets))
                    for co in reversed(choice_offsets):
                        b.PrependUOffsetTRelative(co)
                    choices_vec = b.EndVector()

                OptionDefMod.Start(b)
                OptionDefMod.AddName(b, oname)
                OptionDefMod.AddType(b, opt["wire_type"])
                OptionDefMod.AddDefaultValue(b, odefault)
                if "min" in opt:
                    OptionDefMod.AddMin(b, opt["min"])
                if "max" in opt:
                    OptionDefMod.AddMax(b, opt["max"])
                if choices_vec is not None:
                    OptionDefMod.AddChoices(b, choices_vec)
                opt_offsets.append(OptionDefMod.End(b))

            IdentifyMod.StartOptionsVector(b, len(opt_offsets))
            for oo in reversed(opt_offsets):
                b.PrependUOffsetTRelative(oo)
            opts_vec = b.EndVector()

        IdentifyMod.Start(b)
        IdentifyMod.AddName(b, n)
        IdentifyMod.AddAuthor(b, a)
        if opts_vec is not None:
            IdentifyMod.AddOptions(b, opts_vec)
        if aid is not None:
            IdentifyMod.AddAgentId(b, aid)
        return IdentifyMod.End(b)

    return _build_bot_packet(BotMessage.Identify, build)


def encode_ready() -> bytes:
    def build(b):
        ReadyMod.Start(b)
        return ReadyMod.End(b)

    return _build_bot_packet(BotMessage.Ready, build)


def encode_preprocessing_done() -> bytes:
    def build(b):
        PreprocessingDoneMod.Start(b)
        return PreprocessingDoneMod.End(b)

    return _build_bot_packet(BotMessage.PreprocessingDone, build)


def encode_action(direction: int, player: int) -> bytes:
    def build(b):
        ActionMod.Start(b)
        ActionMod.AddDirection(b, direction)
        ActionMod.AddPlayer(b, player)
        return ActionMod.End(b)

    return _build_bot_packet(BotMessage.Action, build)


def encode_pong() -> bytes:
    def build(b):
        PongMod.Start(b)
        return PongMod.End(b)

    return _build_bot_packet(BotMessage.Pong, build)


def encode_info(
    *,
    player: int = 0,
    multipv: int = 0,
    target: tuple[int, int] | None = None,
    depth: int = 0,
    nodes: int = 0,
    score: float = 0.0,
    pv: list[int] | None = None,
    message: str = "",
) -> bytes:
    def build(b):
        # Pre-create strings and vectors before starting the table.
        msg_off = b.CreateString(message) if message else None

        pv_off = None
        if pv:
            InfoMod.InfoStartPvVector(b, len(pv))
            for d in reversed(pv):
                b.PrependUint8(d)
            pv_off = b.EndVector()

        InfoMod.Start(b)
        InfoMod.AddPlayer(b, player)
        InfoMod.AddMultipv(b, multipv)
        if target is not None:
            InfoMod.AddTarget(b, CreateVec2(b, target[0], target[1]))
        InfoMod.AddDepth(b, depth)
        InfoMod.AddNodes(b, nodes)
        InfoMod.AddScore(b, score)
        if pv_off is not None:
            InfoMod.AddPv(b, pv_off)
        if msg_off is not None:
            InfoMod.AddMessage(b, msg_off)
        return InfoMod.End(b)

    return _build_bot_packet(BotMessage.Info, build)
