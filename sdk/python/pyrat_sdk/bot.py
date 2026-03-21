"""Bot base class and run() lifecycle.

Extend ``Bot`` and implement ``think(state, ctx) -> Direction``.
Call ``Bot().run()`` from ``__main__`` to connect and play.
"""

from __future__ import annotations

import enum
import os
import queue
import sys
import threading
import time
import traceback
from typing import Any

from pyrat_sdk._wire import codec
from pyrat_sdk._wire.connection import Connection
from pyrat_sdk._wire.protocol.HostMessage import HostMessage
from pyrat_sdk.options import apply_set_option, collect_options, options_to_wire
from pyrat_sdk.state import Direction, GameState, Player

# ── GameResult ────────────────────────────────────────


class GameResult(enum.IntEnum):
    """Outcome of a match.  Values match the wire protocol."""

    PLAYER1 = 0
    PLAYER2 = 1
    DRAW = 2


# ── Context ────────────────────────────────────────────


class Context:
    """Passed to ``think()`` and ``preprocess()``.  Provides timing and info sending."""

    def __init__(
        self,
        timeout_ms: int,
        conn: Connection,
        stop_event: threading.Event | None = None,
    ) -> None:
        self._deadline = time.monotonic() + (
            86400.0 if timeout_ms == 0 else timeout_ms / 1000.0
        )
        self._conn = conn
        self._stop_event = stop_event

    def time_remaining_ms(self) -> float:
        if self._stop_event is not None and self._stop_event.is_set():
            return 0.0
        return max(0.0, (self._deadline - time.monotonic()) * 1000.0)

    def should_stop(self) -> bool:
        return time.monotonic() >= self._deadline or (
            self._stop_event is not None and self._stop_event.is_set()
        )

    def send_info(
        self,
        *,
        player: Player,
        multipv: int = 0,
        target: tuple[int, int] | None = None,
        depth: int = 0,
        nodes: int = 0,
        score: float | None = None,
        pv: list[Direction] | None = None,
        message: str = "",
    ) -> None:
        """Send an Info message to the host (for GUI / debugging)."""
        try:
            self._conn.send_frame(
                codec.encode_info(
                    player=int(player),
                    multipv=multipv,
                    target=target,
                    depth=depth,
                    nodes=nodes,
                    score=score,
                    pv=pv,
                    message=message,
                )
            )
        except Exception as e:
            print(f"send_info() failed: {e}", file=sys.stderr)


# ── Bot base class ─────────────────────────────────────


class Bot:
    """Base class for a single-player PyRat bot.

    Override ``think()`` (required) and optionally ``preprocess()``.
    """

    name: str = "Unnamed Bot"
    author: str = "Unknown"

    def think(self, state: GameState, ctx: Context) -> Direction:
        """Return the direction to move this turn.  Must be overridden."""
        raise NotImplementedError(
            "Override think() in your Bot subclass. Return a Direction (e.g., Direction.UP)."
        )

    def preprocess(self, state: GameState, ctx: Context) -> None:
        """Optional — called once before the game starts."""

    def on_game_over(self, result: GameResult, scores: tuple[float, float]) -> None:
        """Optional — called when the game ends with the result and final scores."""

    def run(self) -> None:
        """Entry point.  Reads env vars, connects, plays, exits."""
        _run_bot(self, self.preprocess, self._handle_turn)

    def _handle_turn(self, state: GameState, ctx: Context, conn: Connection) -> None:
        try:
            direction = self.think(state, ctx)
        except Exception:
            traceback.print_exc()
            direction = Direction.STAY

        if ctx.should_stop():
            print(
                "think() exceeded time limit. The host may have used STAY.",
                file=sys.stderr,
            )

        direction = _validate_direction(direction, "think()")
        conn.send_frame(codec.encode_action(int(direction), int(state.my_player)))


# ── HivemindBot ───────────────────────────────────────


class HivemindBot:
    """Base class for a bot controlling both players.

    Override ``think()`` to return ``{Player.PLAYER1: dir, Player.PLAYER2: dir}``.
    """

    name: str = "Unnamed Hivemind"
    author: str = "Unknown"

    def think(self, state: GameState, ctx: Context) -> dict[Player, Direction]:
        raise NotImplementedError(
            "Override think() in your HivemindBot subclass. "
            "Return a dict mapping Player to Direction."
        )

    def preprocess(self, state: GameState, ctx: Context) -> None:
        pass

    def on_game_over(self, result: GameResult, scores: tuple[float, float]) -> None:
        """Optional — called when the game ends with the result and final scores."""

    def run(self) -> None:
        _run_bot(self, self.preprocess, self._handle_turn)

    def _handle_turn(self, state: GameState, ctx: Context, conn: Connection) -> None:
        try:
            moves = self.think(state, ctx)
        except Exception:
            traceback.print_exc()
            moves = {}

        if ctx.should_stop():
            print(
                "think() exceeded time limit. The host may have used STAY.",
                file=sys.stderr,
            )

        if not isinstance(moves, dict):
            print(
                f"think() returned {type(moves).__name__}, expected dict. "
                f"Defaulting to STAY for both players.",
                file=sys.stderr,
            )
            moves = {}

        for player in (Player.PLAYER1, Player.PLAYER2):
            direction = moves.get(player, Direction.STAY)
            direction = _validate_direction(direction, f"think()[{player.name}]")
            conn.send_frame(codec.encode_action(int(direction), int(player)))


# ── Shared lifecycle ──────────────────────────────────


def _parse_port() -> int:
    raw = os.environ.get("PYRAT_HOST_PORT", "0")
    try:
        port = int(raw)
    except ValueError:
        print(
            f"PYRAT_HOST_PORT={raw!r} is not a valid port number",
            file=sys.stderr,
        )
        sys.exit(1)
    if port == 0:
        print("PYRAT_HOST_PORT not set", file=sys.stderr)
        sys.exit(1)
    return port


def _validate_direction(value: Any, source: str) -> Direction:
    """Coerce a think() return to Direction. Defaults to STAY on failure."""
    try:
        return Direction(value)
    except (ValueError, TypeError):
        print(
            f"{source} returned {value!r}, expected a Direction (0-4). "
            f"Defaulting to STAY.",
            file=sys.stderr,
        )
        return Direction.STAY


def _reader_loop(
    conn: Connection,
    msg_queue: queue.Queue[tuple[int, object] | None],
    stop_event: threading.Event,
) -> None:
    """Background reader — forwards host messages, sets stop flag on Stop/Timeout."""
    while True:
        try:
            buf = conn.recv_frame()
        except (ConnectionError, OSError):
            break
        except Exception as e:
            print(f"[sdk] read error: {e}", file=sys.stderr)
            break

        try:
            msg_type, table = codec.decode_host_packet(buf)
        except Exception as e:
            print(f"[sdk] parse error: {e}", file=sys.stderr)
            continue

        if msg_type == HostMessage.Ping:
            try:
                conn.send_frame(codec.encode_pong())
            except Exception:
                pass
            continue

        if msg_type in (HostMessage.Stop, HostMessage.Timeout):
            stop_event.set()

        msg_queue.put((msg_type, table))

    msg_queue.put(None)


def _run_bot(bot: Any, preprocess_fn: Any, turn_fn: Any) -> None:
    """Shared entry point for Bot and HivemindBot."""
    port = _parse_port()
    agent_id = os.environ.get("PYRAT_AGENT_ID", "")

    try:
        conn = Connection("127.0.0.1", port)
    except OSError as e:
        print(
            f"Could not connect to host on port {port}: {e}\n"
            f"Make sure the host is running.",
            file=sys.stderr,
        )
        sys.exit(1)

    try:
        _run_lifecycle(
            conn,
            agent_id,
            bot=bot,
            preprocess_fn=preprocess_fn,
            turn_fn=turn_fn,
        )
    except ConnectionError as e:
        if str(e):
            print(f"Connection lost: {e}", file=sys.stderr)
    finally:
        conn.close()


def _run_lifecycle(
    conn: Connection,
    agent_id: str,
    *,
    bot: Any,
    preprocess_fn: Any,
    turn_fn: Any,
) -> None:
    """Shared connect → identify → ready → config → preprocess → turn-loop."""
    # 1. Collect options and Identify + Ready.
    option_defs = collect_options(type(bot))
    wire_options = options_to_wire(option_defs) if option_defs else None
    conn.send_frame(codec.encode_identify(bot.name, bot.author, agent_id, wire_options))
    conn.send_frame(codec.encode_ready())

    # 2. Wait for SetOption*, MatchConfig, and StartPreprocessing.
    config: dict[str, Any] | None = None
    while True:
        try:
            msg_type, table = codec.decode_host_packet(conn.recv_frame())
        except ConnectionError:
            raise
        except Exception as e:
            raise ConnectionError(f"Failed to decode host message: {e}") from e

        if msg_type == HostMessage.SetOption:
            name, value = codec.extract_set_option(table)
            apply_set_option(bot, option_defs, name, value)
        elif msg_type == HostMessage.MatchConfig:
            try:
                config = codec.extract_match_config(table)
            except Exception as e:
                raise ConnectionError(f"Failed to decode MatchConfig: {e}") from e
        elif msg_type == HostMessage.StartPreprocessing:
            break
        else:
            print(
                f"Unexpected message during setup: type={msg_type}. Ignoring.",
                file=sys.stderr,
            )

    if config is None:
        raise RuntimeError(
            "Protocol error: no MatchConfig before StartPreprocessing. "
            "This is a host bug, not your bot."
        )
    state = GameState(config)

    # Start background reader thread before preprocessing so the stop flag
    # works for long preprocess() calls too.
    stop_event = threading.Event()
    msg_queue: queue.Queue[tuple[int, object] | None] = queue.Queue()
    reader_thread = threading.Thread(
        target=_reader_loop,
        args=(conn, msg_queue, stop_event),
        daemon=True,
    )
    reader_thread.start()

    # 3. Preprocessing.
    ctx = Context(config["preprocessing_timeout_ms"], conn, stop_event)
    try:
        preprocess_fn(state, ctx)
    except Exception:
        traceback.print_exc()
        print("preprocess() crashed, but the game will continue.", file=sys.stderr)
    conn.send_frame(codec.encode_preprocessing_done())

    # 4. Turn loop — reads from queue instead of socket.
    while True:
        item = msg_queue.get()
        if item is None:
            break

        msg_type, table = item

        if msg_type == HostMessage.TurnState:
            try:
                ts = codec.extract_turn_state(table)
            except Exception as e:
                raise ConnectionError(f"Failed to decode TurnState: {e}") from e
            state.update(ts)
            stop_event.clear()
            ctx = Context(config["move_timeout_ms"], conn, stop_event)
            turn_fn(state, ctx, conn)
        elif msg_type == HostMessage.GameOver:
            try:
                go = codec.extract_game_over(table)
                bot.on_game_over(
                    GameResult(go["result"]),
                    (go["player1_score"], go["player2_score"]),
                )
            except Exception:
                traceback.print_exc()
            break
        elif msg_type == HostMessage.Stop:
            pass  # Non-terminal — flag already set by reader thread.
        elif msg_type == HostMessage.Timeout:
            pass  # Flag already set by reader thread.
