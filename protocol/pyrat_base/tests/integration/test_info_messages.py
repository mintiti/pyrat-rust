#!/usr/bin/env python3
"""Test info message functionality."""

import asyncio
import sys
from pathlib import Path
from typing import List, Optional

import pytest

# Get the examples directory path
EXAMPLES_DIR = Path(__file__).parent.parent.parent / "examples"


class InfoMessageTester:
    """Helper to test info messages."""

    def __init__(self, ai_script: str, debug: bool = True):
        self.ai_script = str(EXAMPLES_DIR / ai_script)
        self.process: Optional[asyncio.subprocess.Process] = None
        self.all_responses: List[str] = []
        self.info_messages: List[str] = []
        self.debug = debug

    async def start(self):
        """Start the AI subprocess."""
        import os

        # Set up environment
        env = os.environ.copy()
        repo_root = Path(__file__).parent.parent.parent.parent
        env["PYTHONPATH"] = str(repo_root)
        env["PYTHONUNBUFFERED"] = "1"
        if self.debug:
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

    async def send_and_read(
        self, command: str, wait_for: Optional[str] = None, timeout: float = 1.0
    ):
        """Send command and read responses until we see expected response."""
        # Send command
        if self.process and self.process.stdin:
            self.process.stdin.write(f"{command}\n".encode())
            await self.process.stdin.drain()

        responses = []
        end_time = asyncio.get_event_loop().time() + timeout

        while asyncio.get_event_loop().time() < end_time:
            try:
                if not self.process or not self.process.stdout:
                    break
                response_bytes = await asyncio.wait_for(
                    self.process.stdout.readline(),
                    timeout=min(0.1, end_time - asyncio.get_event_loop().time()),
                )
                if response_bytes:
                    response = response_bytes.decode().strip()
                    responses.append(response)
                    self.all_responses.append(response)

                    if response.startswith("info "):
                        self.info_messages.append(response)

                    if wait_for and wait_for in response:
                        break
            except asyncio.TimeoutError:
                if wait_for is None:
                    break
                continue

        return responses

    async def get_stderr(self):
        """Get stderr output."""
        try:
            stderr_data = await asyncio.wait_for(
                self.process.stderr.read(4096), timeout=0.1
            )
            return stderr_data.decode("utf-8", errors="replace") if stderr_data else ""
        except asyncio.TimeoutError:
            return ""

    async def cleanup(self):
        """Terminate the AI process."""
        if self.process and self.process.returncode is None:
            self.process.terminate()
            await self.process.wait()


@pytest.mark.asyncio
async def test_greedy_ai_sends_info_messages():
    """Test that greedy AI sends info messages during gameplay."""
    tester = InfoMessageTester("greedy_ai.py", debug=True)

    try:
        await tester.start()

        # Handshake
        responses = await tester.send_and_read("pyrat", "pyratready")
        assert any("pyratready" in r for r in responses)

        # Minimal game setup
        await tester.send_and_read("newgame")
        await tester.send_and_read("maze height:5 width:5")
        await tester.send_and_read("walls")
        await tester.send_and_read("mud")
        await tester.send_and_read("cheese (3,3) (1,1)")  # Multiple cheese
        await tester.send_and_read("player1 rat (0,0)")
        await tester.send_and_read("player2 python (4,4)")
        await tester.send_and_read("youare rat")

        # Start preprocessing
        responses = await tester.send_and_read(
            "startpreprocessing", "preprocessingdone", timeout=2.0
        )

        # Check for preprocessing info messages
        # With debug enabled, log() calls should create info messages
        preprocessing_infos = [
            msg
            for msg in tester.info_messages
            if "[DEBUG]" in msg
            and ("Preprocessing" in msg or "Maze size" in msg or "Total cheese" in msg)
        ]

        print(f"Info messages during preprocessing: {preprocessing_infos}")
        assert (
            len(preprocessing_infos) > 0
        ), "Should have debug info messages during preprocessing"

        # Request a move
        await tester.send_and_read("moves rat:STAY python:STAY")
        responses = await tester.send_and_read("go", "move", timeout=1.0)

        # Check for move-related info messages
        move_infos = [
            msg
            for msg in tester.info_messages
            if "target" in msg or "New target" in msg or "path" in msg
        ]

        print(f"Info messages during move: {move_infos}")

        # The greedy AI should send at least one info message about its target
        target_infos = [msg for msg in tester.info_messages if "target" in msg]
        assert len(target_infos) > 0, "Greedy AI should send info about its target"

        # Print all info messages for debugging
        print(f"\nAll info messages: {tester.info_messages}")

    finally:
        stderr = await tester.get_stderr()
        if stderr:
            print(f"\nStderr output:\n{stderr}")
        await tester.cleanup()


@pytest.mark.asyncio
async def test_info_message_format():
    """Test that info messages are properly formatted."""
    tester = InfoMessageTester(
        "greedy_ai.py", debug=False
    )  # No debug to see only real info messages

    try:
        await tester.start()

        # Quick setup to get to move calculation
        await tester.send_and_read("pyrat", "pyratready")
        await tester.send_and_read("newgame")
        await tester.send_and_read("maze height:5 width:5")
        await tester.send_and_read("walls")
        await tester.send_and_read("mud")
        await tester.send_and_read("cheese (2,2) (3,3)")
        await tester.send_and_read("player1 rat (0,0)")
        await tester.send_and_read("player2 python (4,4)")
        await tester.send_and_read("youare rat")
        await tester.send_and_read("startpreprocessing", "preprocessingdone")

        # Request move
        await tester.send_and_read("moves rat:STAY python:STAY")
        await tester.send_and_read("go", "move")

        # Check info message format
        for info_msg in tester.info_messages:
            assert info_msg.startswith(
                "info "
            ), f"Info message should start with 'info ': {info_msg}"

            # Check for known info types
            parts = info_msg.split()
            i = 1
            while i < len(parts):
                key = parts[i]
                if key in ["nodes", "depth", "time", "score"]:
                    # These should be followed by numbers
                    assert i + 1 < len(parts), f"Missing value for {key}"
                    assert parts[i + 1].isdigit(), f"Value for {key} should be numeric"
                    i += 2
                elif key == "target":
                    # Should be followed by coordinates (could be tuple format)
                    assert i + 1 < len(parts), "Missing target coordinates"
                    i += 2
                elif key == "string":
                    # Rest of message is the string
                    break
                else:
                    i += 1

    finally:
        await tester.cleanup()


@pytest.mark.asyncio
async def test_ai_without_debug_still_sends_important_info():
    """Test that AI sends important info messages even without debug mode."""
    tester = InfoMessageTester("greedy_ai.py", debug=False)

    try:
        await tester.start()

        # Setup
        await tester.send_and_read("pyrat", "pyratready")
        await tester.send_and_read("newgame")
        await tester.send_and_read("maze height:5 width:5")
        await tester.send_and_read("walls")
        await tester.send_and_read("mud")
        await tester.send_and_read("cheese (4,4) (2,2)")  # Multiple cheese
        await tester.send_and_read("player1 rat (0,0)")
        await tester.send_and_read("player2 python (2,2)")
        await tester.send_and_read("youare rat")
        await tester.send_and_read("startpreprocessing", "preprocessingdone")

        # Get move
        await tester.send_and_read("moves rat:STAY python:STAY")
        await tester.send_and_read("go", "move")

        # Even without debug, greedy AI sends strategic info via send_info()
        # Look for the explicit send_info call in greedy_ai.py
        strategic_infos = [
            msg
            for msg in tester.info_messages
            if "target" in msg or "Time to reach" in msg
        ]

        print(f"Strategic info messages: {strategic_infos}")
        assert (
            len(strategic_infos) > 0
        ), "Greedy AI should send strategic info about targets"

    finally:
        await tester.cleanup()


if __name__ == "__main__":
    pytest.main([__file__, "-v", "-s"])
