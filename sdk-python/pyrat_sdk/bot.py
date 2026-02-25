"""Bot base class and run() lifecycle.

Extend ``Bot`` and implement ``think(state, ctx) -> Direction``.
Call ``Bot().run()`` from ``__main__`` to connect and play.
"""

from __future__ import annotations

import os
import sys
import time
import traceback
from enum import IntEnum
from typing import TYPE_CHECKING

from pyrat_sdk._wire.connection import Connection
from pyrat_sdk._wire import codec
from pyrat_sdk.state import GameState

if TYPE_CHECKING:
    pass

# Ensure generated/ is importable.
import pyrat_sdk._wire  # noqa: F401
from pyrat.protocol.HostMessage import HostMessage


# ── Public enums ───────────────────────────────────────


class Direction(IntEnum):
    UP = 0
    RIGHT = 1
    DOWN = 2
    LEFT = 3
    STAY = 4


class Player(IntEnum):
    PLAYER1 = 0
    PLAYER2 = 1


# ── Context ────────────────────────────────────────────


class Context:
    """Passed to ``think()`` and ``preprocess()``.  Provides timing and info sending."""

    def __init__(self, timeout_ms: int, conn: Connection) -> None:
        self._deadline = time.monotonic() + timeout_ms / 1000.0
        self._conn = conn

    def time_remaining_ms(self) -> float:
        return max(0.0, (self._deadline - time.monotonic()) * 1000.0)

    def should_stop(self) -> bool:
        return time.monotonic() >= self._deadline

    def send_info(self, **kwargs) -> None:
        """Send an Info message to the host (for GUI / debugging)."""
        self._conn.send_frame(codec.encode_info(**kwargs))


# ── Bot base class ─────────────────────────────────────


class Bot:
    """Base class for a single-player PyRat bot.

    Override ``think()`` (required) and optionally ``preprocess()``.
    """

    name: str = "Unnamed Bot"
    author: str = "Unknown"

    def think(self, state: GameState, ctx: Context) -> Direction:
        """Return the direction to move this turn.  Must be overridden."""
        raise NotImplementedError

    def preprocess(self, state: GameState, ctx: Context) -> None:
        """Optional — called once before the game starts."""

    def run(self) -> None:
        """Entry point.  Reads env vars, connects, plays, exits."""
        port = int(os.environ.get("PYRAT_HOST_PORT", "0"))
        agent_id = os.environ.get("PYRAT_AGENT_ID", "")
        if port == 0:
            print("PYRAT_HOST_PORT not set", file=sys.stderr)
            sys.exit(1)

        conn = Connection("127.0.0.1", port)
        try:
            self._lifecycle(conn, agent_id)
        except ConnectionError:
            pass  # Host closed — normal shutdown.
        finally:
            conn.close()

    def _lifecycle(self, conn: Connection, agent_id: str) -> None:
        # 1. Identify + Ready
        conn.send_frame(codec.encode_identify(self.name, self.author, agent_id))
        conn.send_frame(codec.encode_ready())

        # 2. Wait for MatchConfig and StartPreprocessing.
        config: dict | None = None
        while True:
            msg_type, table = codec.decode_host_packet(conn.recv_frame())
            if msg_type == HostMessage.MatchConfig:
                config = codec.extract_match_config(table)
            elif msg_type == HostMessage.StartPreprocessing:
                break

        assert config is not None, "no MatchConfig received before StartPreprocessing"
        state = GameState(config)

        # 3. Preprocessing.
        ctx = Context(config["preprocessing_timeout_ms"], conn)
        try:
            self.preprocess(state, ctx)
        except Exception:
            traceback.print_exc()
        conn.send_frame(codec.encode_preprocessing_done())

        # 4. Turn loop.
        while True:
            msg_type, table = codec.decode_host_packet(conn.recv_frame())

            if msg_type == HostMessage.TurnState:
                ts = codec.extract_turn_state(table)
                state.update(ts)
                ctx = Context(config["move_timeout_ms"], conn)
                try:
                    direction = self.think(state, ctx)
                except Exception:
                    traceback.print_exc()
                    direction = Direction.STAY
                conn.send_frame(
                    codec.encode_action(int(direction), state.my_player)
                )
            elif msg_type == HostMessage.Ping:
                conn.send_frame(codec.encode_pong())
            elif msg_type in (HostMessage.GameOver, HostMessage.Stop):
                break
            elif msg_type == HostMessage.Timeout:
                pass  # Host handled it; we just note it.


# ── HivemindBot ───────────────────────────────────────


class HivemindBot:
    """Base class for a bot controlling both players.

    Override ``think()`` to return ``{Player.PLAYER1: dir, Player.PLAYER2: dir}``.
    """

    name: str = "Unnamed Hivemind"
    author: str = "Unknown"

    def think(self, state: GameState, ctx: Context) -> dict[Player, Direction]:
        raise NotImplementedError

    def preprocess(self, state: GameState, ctx: Context) -> None:
        pass

    def run(self) -> None:
        port = int(os.environ.get("PYRAT_HOST_PORT", "0"))
        agent_id = os.environ.get("PYRAT_AGENT_ID", "")
        if port == 0:
            print("PYRAT_HOST_PORT not set", file=sys.stderr)
            sys.exit(1)

        conn = Connection("127.0.0.1", port)
        try:
            self._lifecycle(conn, agent_id)
        except ConnectionError:
            pass
        finally:
            conn.close()

    def _lifecycle(self, conn: Connection, agent_id: str) -> None:
        conn.send_frame(codec.encode_identify(self.name, self.author, agent_id))
        conn.send_frame(codec.encode_ready())

        config: dict | None = None
        while True:
            msg_type, table = codec.decode_host_packet(conn.recv_frame())
            if msg_type == HostMessage.MatchConfig:
                config = codec.extract_match_config(table)
            elif msg_type == HostMessage.StartPreprocessing:
                break

        assert config is not None
        state = GameState(config)

        ctx = Context(config["preprocessing_timeout_ms"], conn)
        try:
            self.preprocess(state, ctx)
        except Exception:
            traceback.print_exc()
        conn.send_frame(codec.encode_preprocessing_done())

        while True:
            msg_type, table = codec.decode_host_packet(conn.recv_frame())

            if msg_type == HostMessage.TurnState:
                ts = codec.extract_turn_state(table)
                state.update(ts)
                ctx = Context(config["move_timeout_ms"], conn)
                try:
                    moves = self.think(state, ctx)
                except Exception:
                    traceback.print_exc()
                    moves = {
                        Player.PLAYER1: Direction.STAY,
                        Player.PLAYER2: Direction.STAY,
                    }
                for player, direction in moves.items():
                    conn.send_frame(
                        codec.encode_action(int(direction), int(player))
                    )
            elif msg_type == HostMessage.Ping:
                conn.send_frame(codec.encode_pong())
            elif msg_type in (HostMessage.GameOver, HostMessage.Stop):
                break
