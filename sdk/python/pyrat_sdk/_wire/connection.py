"""Synchronous TCP socket with 4-byte BE length-prefix framing.

Mirrors the wire format from ``server/wire/src/framing.rs``:
``[u32 BE payload length][payload bytes]``
"""

import socket
import struct

MAX_PAYLOAD = 16 * 1024 * 1024  # 16 MB, matches DEFAULT_MAX_PAYLOAD in Rust


class Connection:
    """Length-prefixed TCP connection to the host."""

    def __init__(self, host: str, port: int) -> None:
        self._sock = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
        self._sock.setsockopt(socket.IPPROTO_TCP, socket.TCP_NODELAY, 1)
        self._sock.connect((host, port))

    def send_frame(self, payload: bytes) -> None:
        header = struct.pack(">I", len(payload))
        self._sock.sendall(header + payload)

    def recv_frame(self) -> bytes:
        header = self._recv_exact(4)
        (length,) = struct.unpack(">I", header)
        if length == 0:
            raise ConnectionError("received empty frame")
        if length > MAX_PAYLOAD:
            raise ConnectionError(
                f"payload too large: {length} bytes (max {MAX_PAYLOAD})"
            )
        return self._recv_exact(length)

    def close(self) -> None:
        self._sock.close()

    def _recv_exact(self, n: int) -> bytes:
        buf = bytearray()
        while len(buf) < n:
            chunk = self._sock.recv(n - len(buf))
            if not chunk:
                raise ConnectionError("host closed the connection")
            buf.extend(chunk)
        return bytes(buf)
