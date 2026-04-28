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
from pyrat_sdk.options import apply_set_option, collect_options, options_to_wire
from pyrat_sdk.state import Direction, GameState, Player

# ── GameResult ────────────────────────────────────────


class GameResult(enum.IntEnum):
    """Outcome of a match. Values match the wire protocol."""

    PLAYER1 = 0
    PLAYER2 = 1
    DRAW = 2


# ── Context ────────────────────────────────────────────


class Context:
    """Passed to ``think()`` and ``preprocess()``. Provides timing and info sending."""

    def __init__(
        self,
        timeout_ms: int,
        conn: Connection,
        stop_event: threading.Event | None = None,
        player: Player = Player.PLAYER1,
        turn: int = 0,
        state_hash: int = 0,
    ) -> None:
        self._think_start = time.monotonic()
        self._deadline = self._think_start + (
            86400.0 if timeout_ms == 0 else timeout_ms / 1000.0
        )
        self._conn = conn
        self._stop_event = stop_event
        self._player = player
        self._turn = turn
        self._state_hash = state_hash

    def time_remaining_ms(self) -> float:
        if self._stop_event is not None and self._stop_event.is_set():
            return 0.0
        return max(0.0, (self._deadline - time.monotonic()) * 1000.0)

    def should_stop(self) -> bool:
        return time.monotonic() >= self._deadline or (
            self._stop_event is not None and self._stop_event.is_set()
        )

    def think_elapsed_ms(self) -> int:
        """Milliseconds elapsed since think started."""
        return int((time.monotonic() - self._think_start) * 1000.0)

    def send_provisional(
        self, direction: Direction, player: Player | None = None
    ) -> None:
        """Send a provisional (best-so-far) action to the host.

        The host uses the latest provisional as fallback if the committed
        action doesn't arrive in time.
        """
        p = player if player is not None else self._player
        try:
            self._conn.send_frame(
                codec.encode_provisional(
                    int(direction),
                    int(p),
                    self._turn,
                    self._state_hash,
                )
            )
        except Exception as e:
            print(f"send_provisional() failed: {e}", file=sys.stderr)

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
                    pv=[int(d) for d in pv] if pv else None,
                    message=message,
                    turn=self._turn,
                    state_hash=self._state_hash,
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
        """Return the direction to move this turn. Must be overridden."""
        raise NotImplementedError(
            "Override think() in your Bot subclass. Return a Direction (e.g., Direction.UP)."
        )

    def preprocess(self, state: GameState, ctx: Context) -> None:
        """Optional — called once before the game starts."""

    def on_game_over(self, result: GameResult, scores: tuple[float, float]) -> None:
        """Optional — called when the game ends with the result and final scores."""

    def run(self) -> None:
        """Entry point. Reads env vars, connects, plays, exits."""
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
        think_ms = max(1, ctx.think_elapsed_ms())
        conn.send_frame(
            codec.encode_action(
                int(direction),
                int(state.my_player),
                state.turn,
                state.state_hash,
                think_ms=think_ms,
            )
        )


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

        think_ms = max(1, ctx.think_elapsed_ms())
        for player in (Player.PLAYER1, Player.PLAYER2):
            direction = moves.get(player, Direction.STAY)
            direction = _validate_direction(direction, f"think()[{player.name}]")
            conn.send_frame(
                codec.encode_action(
                    int(direction),
                    int(player),
                    state.turn,
                    state.state_hash,
                    think_ms=think_ms,
                )
            )


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
    msg_queue: queue.Queue[dict[str, Any] | None],
    stop_event: threading.Event,
) -> None:
    """Background reader — forwards host messages, sets stop flag on Stop."""
    while True:
        try:
            buf = conn.recv_frame()
        except (ConnectionError, OSError):
            break
        except Exception as e:
            print(f"[sdk] read error: {e}", file=sys.stderr)
            break

        try:
            msg = codec.parse_host_frame(buf)
        except Exception as e:
            print(f"[sdk] parse error: {e}", file=sys.stderr)
            continue

        if msg.get("kind") == "Stop":
            stop_event.set()

        msg_queue.put(msg)

    msg_queue.put(None)


def _read_one(conn: Connection) -> dict[str, Any]:
    """Read a single message synchronously. Used during pre-loop setup."""
    buf = conn.recv_frame()
    return codec.parse_host_frame(buf)


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
    """Handshake → preprocessing → turn loop."""
    option_defs = collect_options(type(bot))
    wire_options = options_to_wire(option_defs) if option_defs else None
    conn.send_frame(codec.encode_identify(bot.name, bot.author, agent_id, wire_options))

    # Welcome assigns the player slot.
    welcome = _read_one(conn)
    if welcome.get("kind") != "Welcome":
        raise ConnectionError(
            f"expected Welcome, got {welcome.get('kind')!r}"
        )
    slot = welcome["player_slot"]

    # Configure carries options + match config in one message.
    configure = _read_one(conn)
    if configure.get("kind") != "Configure":
        raise ConnectionError(
            f"expected Configure, got {configure.get('kind')!r}"
        )
    match_config = configure["match_config"]
    for name, value in configure.get("options", []):
        apply_set_option(bot, option_defs, name, value)

    state = GameState(slot, match_config)

    # Ready carries the bot's local state hash. Host verifies and replies
    # with GoPreprocess.
    conn.send_frame(codec.encode_ready(state.state_hash))

    go_pre = _read_one(conn)
    if go_pre.get("kind") != "GoPreprocess":
        raise ConnectionError(
            f"expected GoPreprocess, got {go_pre.get('kind')!r}"
        )
    if go_pre["state_hash"] != state.state_hash:
        print(
            f"[sdk] state_hash mismatch: bot {state.state_hash:#x} "
            f"vs host {go_pre['state_hash']:#x}",
            file=sys.stderr,
        )

    # Reader thread runs from here on so Stop during preprocessing is honored.
    stop_event = threading.Event()
    msg_queue: queue.Queue[dict[str, Any] | None] = queue.Queue()
    reader_thread = threading.Thread(
        target=_reader_loop,
        args=(conn, msg_queue, stop_event),
        daemon=True,
    )
    reader_thread.start()

    # Preprocessing.
    ctx = Context(
        match_config["preprocessing_timeout_ms"],
        conn,
        stop_event,
        player=state.my_player,
        turn=0,
        state_hash=state.state_hash,
    )
    try:
        preprocess_fn(state, ctx)
    except Exception:
        traceback.print_exc()
        print("preprocess() crashed, but the game will continue.", file=sys.stderr)
    conn.send_frame(codec.encode_preprocessing_done())

    # Turn loop.
    while True:
        item = msg_queue.get()
        if item is None:
            break

        kind = item.get("kind")

        if kind == "Advance":
            new_hash = state.apply_advance(item["p1_dir"], item["p2_dir"])
            if new_hash == item["new_hash"]:
                conn.send_frame(codec.encode_sync_ok(new_hash))
            else:
                conn.send_frame(codec.encode_resync(new_hash))

        elif kind == "Go":
            stop_event.clear()
            timeout_ms = (item.get("limits") or {}).get("timeout_ms") or match_config[
                "move_timeout_ms"
            ]
            ctx = Context(
                timeout_ms,
                conn,
                stop_event,
                player=state.my_player,
                turn=state.turn,
                state_hash=state.state_hash,
            )
            turn_fn(state, ctx, conn)

        elif kind == "GoState":
            state.load_turn_state(item["turn_state"])
            stop_event.clear()
            timeout_ms = (item.get("limits") or {}).get("timeout_ms") or match_config[
                "move_timeout_ms"
            ]
            ctx = Context(
                timeout_ms,
                conn,
                stop_event,
                player=state.my_player,
                turn=state.turn,
                state_hash=state.state_hash,
            )
            turn_fn(state, ctx, conn)

        elif kind == "FullState":
            new_match_config = item["match_config"]
            new_hash = state.load_full_state(new_match_config, item["turn_state"])
            match_config = new_match_config
            conn.send_frame(codec.encode_sync_ok(new_hash))

        elif kind == "Stop":
            # Reader already set stop_event; nothing more to do at the
            # dispatch level. The flag has either already been observed by
            # the just-finished think() or applies to the next one.
            pass

        elif kind == "GameOver":
            try:
                bot.on_game_over(
                    GameResult(item["result"]),
                    (item["player1_score"], item["player2_score"]),
                )
            except Exception:
                traceback.print_exc()
            break

        elif kind == "ProtocolError":
            print(
                f"[sdk] host reported protocol error: {item.get('reason', '')}",
                file=sys.stderr,
            )
            break

        else:
            print(f"[sdk] ignoring unexpected message kind: {kind!r}", file=sys.stderr)
