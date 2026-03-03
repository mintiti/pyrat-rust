"""Shared test helpers — MockConnection and FlatBuffers frame builders."""

from __future__ import annotations

import flatbuffers

from pyrat_sdk._wire.protocol import GameOver as GameOverMod
from pyrat_sdk._wire.protocol import HostPacket as HostPacketMod
from pyrat_sdk._wire.protocol import MatchConfig as MatchConfigMod
from pyrat_sdk._wire.protocol import Ping as PingMod
from pyrat_sdk._wire.protocol import SetOption as SetOptionMod
from pyrat_sdk._wire.protocol import StartPreprocessing as StartPreprocessingMod
from pyrat_sdk._wire.protocol import TurnState as TurnStateMod
from pyrat_sdk._wire.protocol.Vec2 import CreateVec2


class MockConnection:
    """Fake Connection for testing _run_lifecycle without sockets."""

    def __init__(self, incoming: list[bytes]) -> None:
        self._incoming = list(incoming)
        self.sent: list[bytes] = []
        self._idx = 0

    def send_frame(self, payload: bytes) -> None:
        self.sent.append(payload)

    def recv_frame(self) -> bytes:
        frame = self._incoming[self._idx]
        self._idx += 1
        return frame

    def close(self) -> None:
        pass


def build_host_packet(msg_type: int, build_fn) -> bytes:
    """Build a HostPacket wrapping the inner message created by *build_fn*."""
    builder = flatbuffers.Builder(256)
    msg_offset = build_fn(builder)
    HostPacketMod.Start(builder)
    HostPacketMod.AddMessageType(builder, msg_type)
    HostPacketMod.AddMessage(builder, msg_offset)
    packet = HostPacketMod.End(builder)
    builder.Finish(packet)
    return bytes(builder.Output())


def build_set_option(builder, name: str, value: str):
    n = builder.CreateString(name)
    v = builder.CreateString(value)
    SetOptionMod.Start(builder)
    SetOptionMod.AddName(builder, n)
    SetOptionMod.AddValue(builder, v)
    return SetOptionMod.End(builder)


def build_minimal_match_config(builder, *, controlled_player: int = 0):
    """Build a MatchConfig with just enough fields for GameState to initialize."""
    MatchConfigMod.StartControlledPlayersVector(builder, 1)
    builder.PrependUint8(controlled_player)
    cp_vec = builder.EndVector()

    MatchConfigMod.StartCheeseVector(builder, 1)
    CreateVec2(builder, 1, 1)
    cheese_vec = builder.EndVector()

    MatchConfigMod.Start(builder)
    MatchConfigMod.AddWidth(builder, 3)
    MatchConfigMod.AddHeight(builder, 3)
    MatchConfigMod.AddMaxTurns(builder, 10)
    MatchConfigMod.AddControlledPlayers(builder, cp_vec)
    MatchConfigMod.AddCheese(builder, cheese_vec)
    MatchConfigMod.AddPlayer1Start(builder, CreateVec2(builder, 0, 0))
    MatchConfigMod.AddPlayer2Start(builder, CreateVec2(builder, 2, 2))
    MatchConfigMod.AddMoveTimeoutMs(builder, 1000)
    MatchConfigMod.AddPreprocessingTimeoutMs(builder, 1000)
    return MatchConfigMod.End(builder)


def build_empty(builder, mod):
    mod.Start(builder)
    return mod.End(builder)


def build_game_over(builder, result: int = 0, p1: float = 0.0, p2: float = 0.0):
    GameOverMod.Start(builder)
    GameOverMod.AddResult(builder, result)
    GameOverMod.AddPlayer1Score(builder, p1)
    GameOverMod.AddPlayer2Score(builder, p2)
    return GameOverMod.End(builder)


def build_turn_state(
    builder,
    *,
    turn: int = 1,
    p1_pos: tuple[int, int] = (0, 0),
    p2_pos: tuple[int, int] = (2, 2),
    p1_score: float = 0.0,
    p2_score: float = 0.0,
    p1_mud: int = 0,
    p2_mud: int = 0,
    p1_last: int = 4,
    p2_last: int = 4,
    cheese: list[tuple[int, int]] | None = None,
):
    """Build a TurnState FlatBuffers table."""
    if cheese is None:
        cheese = [(1, 1)]

    TurnStateMod.StartCheeseVector(builder, len(cheese))
    for cx, cy in reversed(cheese):
        CreateVec2(builder, cx, cy)
    cheese_vec = builder.EndVector()

    TurnStateMod.Start(builder)
    TurnStateMod.AddTurn(builder, turn)
    TurnStateMod.AddPlayer1Position(builder, CreateVec2(builder, *p1_pos))
    TurnStateMod.AddPlayer2Position(builder, CreateVec2(builder, *p2_pos))
    TurnStateMod.AddPlayer1Score(builder, p1_score)
    TurnStateMod.AddPlayer2Score(builder, p2_score)
    TurnStateMod.AddPlayer1MudTurns(builder, p1_mud)
    TurnStateMod.AddPlayer2MudTurns(builder, p2_mud)
    TurnStateMod.AddPlayer1LastMove(builder, p1_last)
    TurnStateMod.AddPlayer2LastMove(builder, p2_last)
    TurnStateMod.AddCheese(builder, cheese_vec)
    return TurnStateMod.End(builder)


def build_ping(builder):
    PingMod.Start(builder)
    return PingMod.End(builder)


def make_lifecycle_frames(
    *,
    controlled_player: int = 0,
    set_options: list[tuple[str, str]] | None = None,
    turn_states: int = 0,
    include_ping: bool = False,
) -> list[bytes]:
    """Build a standard sequence of host frames for lifecycle tests."""
    from pyrat_sdk._wire.protocol.HostMessage import HostMessage

    frames: list[bytes] = []

    if set_options:
        for name, value in set_options:
            frames.append(
                build_host_packet(
                    HostMessage.SetOption,
                    lambda b, n=name, v=value: build_set_option(b, n, v),
                )
            )

    frames.append(
        build_host_packet(
            HostMessage.MatchConfig,
            lambda b: build_minimal_match_config(
                b, controlled_player=controlled_player
            ),
        )
    )
    frames.append(
        build_host_packet(
            HostMessage.StartPreprocessing,
            lambda b: build_empty(b, StartPreprocessingMod),
        )
    )

    for i in range(turn_states):
        frames.append(
            build_host_packet(
                HostMessage.TurnState,
                lambda b, t=i + 1: build_turn_state(b, turn=t),
            )
        )

    if include_ping:
        frames.append(build_host_packet(HostMessage.Ping, build_ping))

    frames.append(build_host_packet(HostMessage.GameOver, lambda b: build_game_over(b)))
    return frames
