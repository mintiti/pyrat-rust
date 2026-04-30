"""Shared test helpers — MockConnection and dict-based message builders.

Frames are built by serializing kind-tagged dicts through the same Rust
codec the SDK uses, so tests speak the new protocol natively.
"""

from __future__ import annotations

from typing import Any

from pyrat_sdk._engine import serialize_host_msg


class MockConnection:
    """In-memory Connection for testing _run_lifecycle without sockets."""

    def __init__(self, incoming: list[bytes]) -> None:
        self._incoming = list(incoming)
        self.sent: list[bytes] = []
        self._idx = 0

    def send_frame(self, payload: bytes) -> None:
        self.sent.append(payload)

    def recv_frame(self) -> bytes:
        if self._idx >= len(self._incoming):
            raise ConnectionError("no more frames")
        frame = self._incoming[self._idx]
        self._idx += 1
        return frame

    def close(self) -> None:
        pass


# ── Message builders ───────────────────────────────────


def host_frame(msg: dict[str, Any]) -> bytes:
    """Serialize a HostMsg dict into a wire frame."""
    return serialize_host_msg(msg)


def minimal_match_config(**overrides: Any) -> dict[str, Any]:
    """Minimal MatchConfig dict — 3x3 maze, single cheese."""
    cfg = {
        "width": 3,
        "height": 3,
        "max_turns": 10,
        "walls": [],
        "mud": [],
        "cheese": [(1, 1)],
        "player1_start": (0, 0),
        "player2_start": (2, 2),
        "timing": 0,  # Wait
        "move_timeout_ms": 1000,
        "preprocessing_timeout_ms": 1000,
    }
    cfg.update(overrides)
    return cfg


def empty_search_limits() -> dict[str, Any]:
    """Search limits with all fields unset — bot thinks until Stop."""
    return {"timeout_ms": None, "depth": None, "nodes": None}


def turn_state(**overrides: Any) -> dict[str, Any]:
    """TurnState dict with sensible defaults for the minimal config."""
    ts = {
        "turn": 1,
        "player1_position": (0, 0),
        "player2_position": (2, 2),
        "player1_score": 0.0,
        "player2_score": 0.0,
        "player1_mud_turns": 0,
        "player2_mud_turns": 0,
        "cheese": [(1, 1)],
        "player1_last_move": 4,  # STAY
        "player2_last_move": 4,
    }
    ts.update(overrides)
    return ts


def make_lifecycle_frames(
    *,
    slot: int = 0,
    configure_options: list[tuple[str, str]] | None = None,
    turn_count: int = 0,
    match_config: dict[str, Any] | None = None,
) -> list[bytes]:
    """Build a complete handshake + N Go frames + GameOver."""
    cfg = match_config if match_config is not None else minimal_match_config()
    frames = [
        host_frame({"kind": "Welcome", "player_slot": slot}),
        host_frame(
            {
                "kind": "Configure",
                "options": configure_options or [],
                "match_config": cfg,
            }
        ),
        host_frame({"kind": "GoPreprocess", "state_hash": 0}),
    ]
    for _ in range(turn_count):
        frames.append(
            host_frame(
                {
                    "kind": "Go",
                    "state_hash": 0,
                    "limits": empty_search_limits(),
                }
            )
        )
    frames.append(
        host_frame(
            {
                "kind": "GameOver",
                "result": 0,
                "player1_score": 0.0,
                "player2_score": 0.0,
            }
        )
    )
    return frames
