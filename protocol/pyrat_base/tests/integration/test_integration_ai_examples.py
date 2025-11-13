#!/usr/bin/env python3
"""Integration tests for example AIs with real subprocess communication."""

import asyncio
import sys
from pathlib import Path
from typing import List, Optional

import pytest

# Get the examples directory path
EXAMPLES_DIR = Path(__file__).parent.parent.parent / "pyrat_base" / "examples"


class AIProtocolTester:
    """Helper class to test AI protocols via subprocess."""

    def __init__(self, ai_script: str):
        self.ai_script = str(EXAMPLES_DIR / ai_script)
        self.process: Optional[asyncio.subprocess.Process] = None
        self.responses: List[str] = []

    async def start(self):
        """Start the AI subprocess."""
        import os

        # Set up environment with correct PYTHONPATH
        env = os.environ.copy()
        # Add the repository root to PYTHONPATH so imports work
        repo_root = Path(__file__).parent.parent.parent.parent
        env["PYTHONPATH"] = str(repo_root)
        env["PYTHONUNBUFFERED"] = "1"

        self.process = await asyncio.create_subprocess_exec(
            sys.executable,
            "-u",
            self.ai_script,
            stdin=asyncio.subprocess.PIPE,
            stdout=asyncio.subprocess.PIPE,
            stderr=asyncio.subprocess.PIPE,
            env=env,
        )

    async def send(self, command: str):
        """Send a command to the AI."""
        if self.process and self.process.stdin:
            if (
                hasattr(self.process.stdin, "is_closing")
                and self.process.stdin.is_closing()
            ):
                raise RuntimeError("AI stdin is closed")
            self.process.stdin.write(f"{command}\n".encode())
            await self.process.stdin.drain()

    async def read_until(self, expected: str, timeout: float = 0.2):
        """Read responses until we see the expected string."""
        responses = []
        try:
            while True:
                if not self.process or not self.process.stdout:
                    break
                response_bytes = await asyncio.wait_for(
                    self.process.stdout.readline(), timeout=timeout
                )
                if not response_bytes:
                    break
                response = response_bytes.decode().strip()
                responses.append(response)
                self.responses.append(response)
                if response == expected:
                    return responses
        except asyncio.TimeoutError:
            pass
        return responses

    async def get_stderr(self):
        """Get any stderr output."""
        if self.process.stderr:
            try:
                stderr_data = await asyncio.wait_for(
                    self.process.stderr.read(1024), timeout=0.1
                )
                return stderr_data.decode("utf-8", errors="replace")
            except asyncio.TimeoutError:
                return ""
        return ""

    async def cleanup(self):
        """Terminate the AI process."""
        if self.process:
            self.process.terminate()
            await self.process.wait()


@pytest.mark.flaky(reruns=2, reruns_delay=1)
@pytest.mark.asyncio
@pytest.mark.slow
async def test_ai_handshake():
    """Test that all example AIs complete handshake correctly."""
    for ai_name in ["dummy_ai.py", "random_ai.py", "greedy_ai.py"]:
        tester = AIProtocolTester(ai_name)
        try:
            await tester.start()
            await tester.send("pyrat")
            responses = await tester.read_until("pyratready", timeout=5.0)

            # Debug: print what we got
            print(f"\n{ai_name} responses: {responses}")

            # Check for errors
            stderr = await tester.get_stderr()
            if stderr:
                print(f"{ai_name} stderr: {stderr}")

            # Check we got the expected responses
            assert any("id name" in r for r in responses), f"{ai_name} didn't send name"
            assert "pyratready" in responses, f"{ai_name} didn't send pyratready"

        finally:
            await tester.cleanup()


@pytest.mark.asyncio
@pytest.mark.slow
async def test_ai_with_small_maze():
    """Test AIs can handle a small maze initialization."""
    tester = AIProtocolTester("dummy_ai.py")
    try:
        await tester.start()

        # Handshake
        await tester.send("pyrat")
        await tester.read_until("pyratready")

        # Send game configuration
        await tester.send("newgame")
        await tester.send("maze height:5 width:5")
        await tester.send("walls (0,0)-(0,1) (1,1)-(2,1)")
        await tester.send("cheese (2,2) (3,3)")
        await tester.send("player1 rat (0,0)")
        await tester.send("player2 python (4,4)")
        await tester.send("youare rat")
        await tester.send("startpreprocessing t:1000")

        # Wait for preprocessing done
        responses = await tester.read_until("preprocessingdone", timeout=0.5)
        assert "preprocessingdone" in responses

        # Check AI is still alive
        assert tester.process.returncode is None

    finally:
        await tester.cleanup()


@pytest.mark.asyncio
async def test_ai_with_large_maze():
    """Test AIs can handle a large maze with many walls."""
    from pyrat_engine.core.game import GameState as PyGameState

    # Create a real game to get realistic wall data
    game = PyGameState(width=21, height=15)
    walls = game.wall_entries()

    # Format walls for protocol
    wall_strings = [f"({x1},{y1})-({x2},{y2})" for (x1, y1), (x2, y2) in walls]
    walls_command = f"walls {' '.join(wall_strings)}"

    print(f"Testing with {len(walls)} walls, command length: {len(walls_command)}")

    tester = AIProtocolTester("dummy_ai.py")
    try:
        await tester.start()

        # Handshake
        await tester.send("pyrat")
        await tester.read_until("pyratready")

        # Send game configuration
        await tester.send("newgame")
        await tester.send("maze height:15 width:21")

        # Send the large walls command
        await tester.send(walls_command)

        # Check if AI crashed
        await asyncio.sleep(0.1)  # Give it time to crash if it's going to

        if tester.process.returncode is not None:
            stderr = await tester.get_stderr()
            pytest.fail(
                f"AI crashed after walls command. Exit code: {tester.process.returncode}\nStderr: {stderr}"
            )

        # Continue with game setup
        await tester.send("cheese (5,5) (10,10)")
        await tester.send("player1 rat (0,0)")
        await tester.send("player2 python (20,14)")
        await tester.send("youare rat")
        await tester.send("startpreprocessing t:1000")

        # Wait for preprocessing done
        responses = await tester.read_until("preprocessingdone", timeout=0.5)

        if "preprocessingdone" not in responses:
            stderr = await tester.get_stderr()
            pytest.fail(f"AI didn't complete preprocessing. Stderr: {stderr}")

    finally:
        await tester.cleanup()


@pytest.mark.asyncio
@pytest.mark.slow
async def test_ai_move_cycle():
    """Test AI can handle a complete move cycle."""
    tester = AIProtocolTester("random_ai.py")
    try:
        await tester.start()

        # Full initialization
        await tester.send("pyrat")
        await tester.read_until("pyratready")
        await tester.send("newgame")
        await tester.send("maze height:5 width:5")
        await tester.send("cheese (2,2)")
        await tester.send("player1 rat (0,0)")
        await tester.send("player2 python (4,4)")
        await tester.send("youare rat")
        await tester.send("startpreprocessing t:100")
        await tester.read_until("preprocessingdone")

        # Test move cycle
        await tester.send("moves rat:STAY python:STAY")
        await tester.send("go t:100")

        # Read move response
        responses = await tester.read_until("move", timeout=0.2)
        move_response = next((r for r in responses if r.startswith("move ")), None)

        assert move_response is not None, "AI didn't send a move"
        assert move_response.startswith(
            "move "
        ), f"Invalid move format: {move_response}"

    finally:
        await tester.cleanup()


if __name__ == "__main__":
    # Run the tests
    pytest.main([__file__, "-v"])
