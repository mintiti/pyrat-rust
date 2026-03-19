"""Tests for Context — deadline and server stop flag."""

from __future__ import annotations

import threading
import time

from conftest import MockConnection

from pyrat_sdk.bot import Context


def test_should_stop_deadline_only():
    """Past deadline, no stop event -> should_stop returns True."""
    ctx = Context(0, MockConnection([]))
    time.sleep(0.001)
    assert ctx.should_stop() is True


def test_should_stop_flag_only():
    """Future deadline, stop event set -> should_stop returns True."""
    event = threading.Event()
    event.set()
    ctx = Context(10_000, MockConnection([]), event)
    assert ctx.should_stop() is True


def test_should_stop_neither():
    """Future deadline, stop event not set -> should_stop returns False."""
    event = threading.Event()
    ctx = Context(10_000, MockConnection([]), event)
    assert ctx.should_stop() is False


def test_time_remaining_returns_zero_when_stopped():
    """Stop event set -> time_remaining_ms returns 0."""
    event = threading.Event()
    event.set()
    ctx = Context(10_000, MockConnection([]), event)
    assert ctx.time_remaining_ms() == 0.0
