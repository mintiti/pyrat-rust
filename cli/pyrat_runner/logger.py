"""Lightweight file-based logging for CLI protocol interactions.

Captures:
- Master events (lifecycle, timeouts, errors)
- Per-AI protocol I/O (→ engine→AI, ← AI→engine)
- Per-AI stderr stream

Opt-in via GameRunner(log_dir=...).
"""

from __future__ import annotations

import os
from dataclasses import dataclass
from datetime import datetime
from typing import IO


def _ts() -> str:
    return datetime.now().strftime("%Y-%m-%d %H:%M:%S.%f")[:-3]


@dataclass
class _AIChannels:
    protocol: IO[str]
    stderr: IO[str]


class GameLogger:
    """File-based logger for a single game run."""

    def __init__(self, root_dir: str):
        self.root_dir = root_dir
        os.makedirs(self.root_dir, exist_ok=True)

        # Master logs
        self._master_log = open(
            os.path.join(self.root_dir, "master.log"), "a", encoding="utf-8"
        )
        self._master_protocol = open(
            os.path.join(self.root_dir, "master.protocol"), "a", encoding="utf-8"
        )

        # Per-AI channels
        self._ai: dict[str, _AIChannels] = {}
        for player in ("rat", "python"):
            pdir = os.path.join(self.root_dir, player)
            os.makedirs(pdir, exist_ok=True)
            protocol_f = open(os.path.join(pdir, "protocol.log"), "a", encoding="utf-8")
            stderr_f = open(os.path.join(pdir, "stderr.txt"), "a", encoding="utf-8")
            self._ai[player] = _AIChannels(protocol=protocol_f, stderr=stderr_f)

    # Master events
    def event(self, message: str) -> None:
        self._master_log.write(f"[{_ts()}] {message}\n")
        self._master_log.flush()

    # Protocol lines
    def protocol(self, player: str, direction: str, line: str) -> None:
        if player in self._ai:
            self._ai[player].protocol.write(f"[{_ts()}] {direction} {line}\n")
            self._ai[player].protocol.flush()
        self._master_protocol.write(f"[{_ts()}] [{player}] {direction} {line}\n")
        self._master_protocol.flush()

    # Stderr lines
    def stderr(self, player: str, line: str) -> None:
        if player in self._ai:
            self._ai[player].stderr.write(line)
            if not line.endswith("\n"):
                self._ai[player].stderr.write("\n")
            self._ai[player].stderr.flush()

    def close(self) -> None:
        try:
            self._master_log.close()
        finally:
            self._master_protocol.close()
        for ch in self._ai.values():
            try:
                ch.protocol.close()
            finally:
                ch.stderr.close()
