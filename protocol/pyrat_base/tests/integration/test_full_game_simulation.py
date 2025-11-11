#!/usr/bin/env python3
"""Test that AI examples work correctly in simulated games.

These are simplified integration tests that verify AIs can:
1. Complete the protocol handshake
2. Initialize a game
3. Send moves
4. Send info messages

We don't run full games to avoid complexity and timing issues.
"""

import asyncio
import sys
from pathlib import Path
from typing import List, Optional

import pytest

# Get the examples directory path
EXAMPLES_DIR = Path(__file__).parent.parent.parent / "pyrat_base" / "examples"


class QuickAITester:
    """Helper to quickly test AI functionality."""

    def __init__(self, ai_script: str):
        self.ai_script = str(EXAMPLES_DIR / ai_script)
        self.process: Optional[asyncio.subprocess.Process] = None
        self.info_messages: List[str] = []

    async def start(self):
        """Start the AI subprocess."""
        import os

        env = os.environ.copy()
        # Need to go up to the actual repo root (pyrat-rust), not just protocol
        repo_root = Path(__file__).parent.parent.parent.parent.parent
        env["PYTHONPATH"] = str(repo_root)
        env["PYTHONUNBUFFERED"] = "1"
        env["PYRAT_DEBUG"] = "1"

        self.process = await asyncio.create_subprocess_exec(
            sys.executable,
            "-u",
            self.ai_script,
            stdin=asyncio.subprocess.PIPE,
            stdout=asyncio.subprocess.PIPE,
            stderr=asyncio.subprocess.PIPE,
            env=env,
        )

    async def send_and_read(self, command: str, expected: str, timeout: float = 1.0):  # noqa: C901
        """Send command and read until we see expected response."""
        # Check stderr for any errors first
        if self.process and self.process.stderr:
            try:
                stderr_bytes = await asyncio.wait_for(
                    self.process.stderr.read(1024), timeout=0.1
                )
                if stderr_bytes:
                    print(f"STDERR: {stderr_bytes.decode()}")
            except asyncio.TimeoutError:
                pass

        if self.process and self.process.stdin:
            self.process.stdin.write(f"{command}\n".encode())
            await self.process.stdin.drain()

        # Read until we see expected response or timeout
        end_time = asyncio.get_event_loop().time() + timeout
        while asyncio.get_event_loop().time() < end_time:
            if not self.process or not self.process.stdout:
                break

            try:
                response_bytes = await asyncio.wait_for(
                    self.process.stdout.readline(),
                    timeout=min(0.1, end_time - asyncio.get_event_loop().time()),
                )
                if response_bytes:
                    response = response_bytes.decode().strip()
                    if response.startswith("info "):
                        self.info_messages.append(response)
                    if expected in response:
                        return True
            except asyncio.TimeoutError:
                continue

        return False

    async def read_available_output(self, timeout: float = 0.5):
        """Read any available output from stdout."""
        end_time = asyncio.get_event_loop().time() + timeout
        while asyncio.get_event_loop().time() < end_time:
            if not self.process or not self.process.stdout:
                break

            try:
                response_bytes = await asyncio.wait_for(
                    self.process.stdout.readline(),
                    timeout=min(0.1, end_time - asyncio.get_event_loop().time()),
                )
                if response_bytes:
                    response = response_bytes.decode().strip()
                    if response.startswith("info "):
                        self.info_messages.append(response)
            except asyncio.TimeoutError:
                break

    async def cleanup(self):
        """Terminate the AI process."""
        if self.process and self.process.returncode is None:
            self.process.terminate()
            await self.process.wait()


@pytest.mark.asyncio
@pytest.mark.timeout(5)
async def test_greedy_ai_basic_functionality():
    """Test greedy AI can complete handshake and make a move."""
    tester = QuickAITester("greedy_ai.py")

    try:
        await tester.start()

        # Handshake
        assert await tester.send_and_read("pyrat", "pyratready")

        # Minimal game setup - send commands without waiting for responses
        tester.process.stdin.write(b"newgame\n")
        tester.process.stdin.write(b"maze height:5 width:5\n")
        tester.process.stdin.write(b"walls\n")
        tester.process.stdin.write(b"mud\n")
        tester.process.stdin.write(b"cheese (2,2) (3,3)\n")
        tester.process.stdin.write(b"player1 rat (0,0)\n")
        tester.process.stdin.write(b"player2 python (4,4)\n")
        tester.process.stdin.write(b"youare rat\n")
        await tester.process.stdin.drain()

        # Skip preprocessing, go straight to move
        assert await tester.send_and_read("go", "move")

        # Read any remaining output to capture info messages
        await tester.read_available_output()

        # Check we got info messages
        assert len(tester.info_messages) > 0, "Greedy AI should send info messages"

    finally:
        await tester.cleanup()


@pytest.mark.asyncio
@pytest.mark.timeout(5)
async def test_random_ai_basic_functionality():
    """Test random AI can complete handshake and make a move."""
    tester = QuickAITester("random_ai.py")

    try:
        await tester.start()

        # Handshake
        assert await tester.send_and_read("pyrat", "pyratready")

        # Minimal game setup - send commands without waiting for responses
        tester.process.stdin.write(b"newgame\n")
        tester.process.stdin.write(b"maze height:5 width:5\n")
        tester.process.stdin.write(b"walls\n")
        tester.process.stdin.write(b"mud\n")
        tester.process.stdin.write(b"cheese (2,2)\n")
        tester.process.stdin.write(b"player1 rat (0,0)\n")
        tester.process.stdin.write(b"player2 python (4,4)\n")
        tester.process.stdin.write(b"youare rat\n")
        await tester.process.stdin.drain()

        # Request move
        assert await tester.send_and_read("go", "move")

    finally:
        await tester.cleanup()


@pytest.mark.asyncio
@pytest.mark.timeout(5)
async def test_dummy_ai_basic_functionality():
    """Test dummy AI always returns STAY."""
    tester = QuickAITester("dummy_ai.py")

    try:
        await tester.start()

        # Handshake
        assert await tester.send_and_read("pyrat", "pyratready")

        # Minimal game setup - send commands without waiting for responses
        tester.process.stdin.write(b"newgame\n")
        tester.process.stdin.write(b"maze height:5 width:5\n")
        tester.process.stdin.write(b"walls\n")
        tester.process.stdin.write(b"mud\n")
        tester.process.stdin.write(b"cheese (2,2)\n")
        tester.process.stdin.write(b"player1 rat (0,0)\n")
        tester.process.stdin.write(b"player2 python (4,4)\n")
        tester.process.stdin.write(b"youare rat\n")
        await tester.process.stdin.drain()

        # Request move - dummy should always return STAY
        assert await tester.send_and_read("go", "move STAY")

    finally:
        await tester.cleanup()


if __name__ == "__main__":
    pytest.main([__file__, "-v", "-s"])
