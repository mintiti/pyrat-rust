"""Integration tests for _reader_loop (bot.py concurrent socket reader)."""

from __future__ import annotations

import queue
import threading

from conftest import MockConnection, host_frame

from pyrat_sdk.bot import _reader_loop


def _run(frames: list[bytes]):
    """Run _reader_loop to completion. Returns (queue items, stop_event, conn)."""
    conn = MockConnection(frames)
    q: queue.Queue[dict | None] = queue.Queue()
    event = threading.Event()

    _reader_loop(conn, q, event)

    items = []
    while not q.empty():
        items.append(q.get_nowait())
    return items, event, conn


def test_stop_sets_event_and_queues():
    items, event, _ = _run([host_frame({"kind": "Stop"})])
    assert event.is_set()
    assert len(items) == 2
    assert items[0]["kind"] == "Stop"
    assert items[1] is None  # sentinel


def test_advance_queued_without_stop():
    frame = host_frame(
        {
            "kind": "Advance",
            "p1_dir": 0,
            "p2_dir": 4,
            "turn": 1,
            "new_hash": 0xCAFE,
        }
    )
    items, event, _ = _run([frame])
    assert not event.is_set()
    assert len(items) == 2
    assert items[0]["kind"] == "Advance"
    assert items[1] is None


def test_go_queued_without_stop():
    frame = host_frame(
        {
            "kind": "Go",
            "state_hash": 0,
            "limits": {"timeout_ms": None, "depth": None, "nodes": None},
        }
    )
    items, event, _ = _run([frame])
    assert not event.is_set()
    assert len(items) == 2
    assert items[0]["kind"] == "Go"
    assert items[1] is None


def test_stop_after_advance():
    advance = host_frame(
        {
            "kind": "Advance",
            "p1_dir": 0,
            "p2_dir": 4,
            "turn": 1,
            "new_hash": 0,
        }
    )
    stop = host_frame({"kind": "Stop"})
    items, event, _ = _run([advance, stop])
    assert event.is_set()
    assert len(items) == 3
    assert items[0]["kind"] == "Advance"
    assert items[1]["kind"] == "Stop"
    assert items[2] is None
