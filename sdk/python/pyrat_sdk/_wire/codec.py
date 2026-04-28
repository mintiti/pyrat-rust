"""Codec — kind-tagged dicts ↔ wire frames, via the Rust codec.

The native extension exposes ``parse_host_frame`` and ``serialize_bot_msg``
backed by ``pyrat-protocol`` (the same crate the host and Rust SDK use).
This module is the Python-friendly facade: ``parse_host_frame`` re-exports
the decoder and the ``encode_*`` helpers build kind-tagged dicts and serialize
them. Field names in dicts mirror the Rust enum exactly.
"""

from __future__ import annotations

from typing import TYPE_CHECKING, Any

from pyrat_sdk._engine import (
    parse_bot_frame,
    parse_host_frame,
    serialize_bot_msg,
    serialize_host_msg,
)

if TYPE_CHECKING:
    from collections.abc import Sequence

__all__ = [
    "encode_action",
    "encode_identify",
    "encode_info",
    "encode_preprocessing_done",
    "encode_provisional",
    "encode_ready",
    "encode_render_commands",
    "encode_resync",
    "encode_sync_ok",
    "parse_bot_frame",
    "parse_host_frame",
    "serialize_bot_msg",
    "serialize_host_msg",
]


# ── Bot → Host encoders ────────────────────────────────


def encode_identify(
    name: str,
    author: str,
    agent_id: str = "",
    options: list[dict[str, Any]] | None = None,
) -> bytes:
    """Build an Identify frame.

    *options* is a list of dicts shaped like ``OptionDef`` — keys
    ``name``, ``option_type``, ``default_value``, ``min``, ``max``,
    ``choices`` (always a list, possibly empty).
    """
    return serialize_bot_msg(
        {
            "kind": "Identify",
            "name": name,
            "author": author,
            "agent_id": agent_id,
            "options": options or [],
        }
    )


def encode_ready(state_hash: int) -> bytes:
    return serialize_bot_msg({"kind": "Ready", "state_hash": state_hash})


def encode_preprocessing_done() -> bytes:
    return serialize_bot_msg({"kind": "PreprocessingDone"})


def encode_action(
    direction: int,
    player: int,
    turn: int,
    state_hash: int,
    think_ms: int = 0,
) -> bytes:
    return serialize_bot_msg(
        {
            "kind": "Action",
            "direction": direction,
            "player": player,
            "turn": turn,
            "state_hash": state_hash,
            "think_ms": think_ms,
        }
    )


def encode_provisional(
    direction: int,
    player: int,
    turn: int,
    state_hash: int,
) -> bytes:
    return serialize_bot_msg(
        {
            "kind": "Provisional",
            "direction": direction,
            "player": player,
            "turn": turn,
            "state_hash": state_hash,
        }
    )


def encode_sync_ok(hash_: int) -> bytes:
    return serialize_bot_msg({"kind": "SyncOk", "hash": hash_})


def encode_resync(my_hash: int) -> bytes:
    return serialize_bot_msg({"kind": "Resync", "my_hash": my_hash})


def encode_info(
    *,
    player: int,
    multipv: int = 0,
    target: tuple[int, int] | None = None,
    depth: int = 0,
    nodes: int = 0,
    score: float | None = None,
    pv: Sequence[int] | None = None,
    message: str = "",
    turn: int = 0,
    state_hash: int = 0,
) -> bytes:
    return serialize_bot_msg(
        {
            "kind": "Info",
            "player": player,
            "multipv": multipv,
            "target": tuple(target) if target is not None else None,
            "depth": depth,
            "nodes": nodes,
            "score": score,
            "pv": list(pv) if pv else [],
            "message": message,
            "turn": turn,
            "state_hash": state_hash,
        }
    )


def encode_render_commands(player: int, turn: int, state_hash: int) -> bytes:
    return serialize_bot_msg(
        {
            "kind": "RenderCommands",
            "player": player,
            "turn": turn,
            "state_hash": state_hash,
        }
    )
