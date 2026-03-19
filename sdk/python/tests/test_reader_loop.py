"""Integration tests for _reader_loop (bot.py concurrent socket reader)."""

from __future__ import annotations

import queue
import threading

from conftest import (
    MockConnection,
    build_host_packet,
    build_ping,
    build_stop,
    build_timeout,
    build_turn_state,
)

from pyrat_sdk._wire.protocol.BotMessage import BotMessage
from pyrat_sdk._wire.protocol.BotPacket import BotPacket
from pyrat_sdk._wire.protocol.HostMessage import HostMessage
from pyrat_sdk.bot import _reader_loop


def _run(frames: list[bytes]):
    """Run _reader_loop to completion with the given frames.

    Returns (queue contents as list, stop_event, conn).
    """
    conn = MockConnection(frames)
    q: queue.Queue[tuple[int, object] | None] = queue.Queue()
    event = threading.Event()

    _reader_loop(conn, q, event)

    items = []
    while not q.empty():
        items.append(q.get_nowait())
    return items, event, conn


def test_stop_sets_event_and_queues():
    frame = build_host_packet(HostMessage.Stop, build_stop)
    items, event, _ = _run([frame])

    assert event.is_set()
    assert len(items) == 2
    msg_type, _table = items[0]
    assert msg_type == HostMessage.Stop
    assert items[1] is None  # sentinel


def test_timeout_sets_event_and_queues():
    frame = build_host_packet(HostMessage.Timeout, build_timeout)
    items, event, _ = _run([frame])

    assert event.is_set()
    assert len(items) == 2
    msg_type, _table = items[0]
    assert msg_type == HostMessage.Timeout
    assert items[1] is None


def test_ping_sends_pong_not_queued():
    frame = build_host_packet(HostMessage.Ping, build_ping)
    items, event, conn = _run([frame])

    assert not event.is_set()
    assert items == [None]  # only the sentinel
    assert len(conn.sent) == 1

    pong = BotPacket.GetRootAs(conn.sent[0])
    assert pong.MessageType() == BotMessage.Pong


def test_turnstate_queued_without_stop():
    frame = build_host_packet(
        HostMessage.TurnState, lambda b: build_turn_state(b, turn=1)
    )
    items, event, _ = _run([frame])

    assert not event.is_set()
    assert len(items) == 2
    msg_type, _table = items[0]
    assert msg_type == HostMessage.TurnState
    assert items[1] is None


def test_stop_after_turnstate():
    ts_frame = build_host_packet(
        HostMessage.TurnState, lambda b: build_turn_state(b, turn=1)
    )
    stop_frame = build_host_packet(HostMessage.Stop, build_stop)
    items, event, _ = _run([ts_frame, stop_frame])

    assert event.is_set()
    # TurnState, Stop, sentinel
    assert len(items) == 3
    assert items[0][0] == HostMessage.TurnState
    assert items[1][0] == HostMessage.Stop
    assert items[2] is None
