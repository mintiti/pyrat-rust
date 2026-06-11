"""Tests for Context — deadline and server stop flag."""

from __future__ import annotations

import threading
import time

from conftest import MockConnection

from pyrat_sdk.bot import Context


def test_should_stop_deadline_only():
    """Past deadline, no stop event -> should_stop returns True."""
    ctx = Context(1, MockConnection([]))  # 1ms timeout
    time.sleep(0.01)
    assert ctx.should_stop() is True


def test_zero_timeout_is_infinite():
    """timeout_ms=0 is treated as infinite — should_stop is False."""
    ctx = Context(0, MockConnection([]))
    time.sleep(0.001)
    assert ctx.should_stop() is False


def test_safety_margin_shaves_deadline():
    """The deadline sits MOVE_SAFETY_MARGIN_MS before the budget, and a budget
    at or below the margin means an immediate deadline — not an infinite one."""
    ctx = Context(10_000, MockConnection([]))
    assert ctx.time_remaining_ms() <= 10_000 - 5
    ctx = Context(5, MockConnection([]))
    assert ctx.should_stop() is True


def test_budget_exceeded_is_not_should_stop():
    """A bot returning at the (margin-shaved) deadline has NOT overshot the
    budget; only running past the full budget counts. Zero budget never does."""
    ctx = Context(10_000, MockConnection([]))
    assert ctx._budget_exceeded() is False
    ctx = Context(1, MockConnection([]))
    time.sleep(0.01)
    assert ctx.should_stop() is True
    assert ctx._budget_exceeded() is True
    ctx = Context(0, MockConnection([]))
    assert ctx._budget_exceeded() is False


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
