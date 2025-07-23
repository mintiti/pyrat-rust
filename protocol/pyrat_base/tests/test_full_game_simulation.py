#!/usr/bin/env python3
"""Test full game simulation with info messages."""

import asyncio
import sys
from pathlib import Path
from typing import List, Optional

import pytest

# Get the examples directory path
EXAMPLES_DIR = Path(__file__).parent.parent / "examples"


class GameSimulator:
    """Simulates a full PyRat game for testing."""

    def __init__(self, ai_script: str):
        self.ai_script = str(EXAMPLES_DIR / ai_script)
        self.process: Optional[asyncio.subprocess.Process] = None
        self.all_responses: List[str] = []
        self.info_messages: List[str] = []

    async def start(self):
        """Start the AI subprocess."""
        import os

        # Set up environment with correct PYTHONPATH
        env = os.environ.copy()
        repo_root = Path(__file__).parent.parent.parent.parent
        env["PYTHONPATH"] = str(repo_root)
        env["PYTHONUNBUFFERED"] = "1"
        env["PYRAT_DEBUG"] = "1"  # Enable debug for better visibility

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
            self.process.stdin.write(f"{command}\n".encode())
            await self.process.stdin.drain()

    async def read_responses(self, timeout: float = 0.5):
        """Read all available responses."""
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
                self.all_responses.append(response)
                if response.startswith("info "):
                    self.info_messages.append(response)
        except asyncio.TimeoutError:
            pass
        return responses

    async def cleanup(self):
        """Terminate the AI process."""
        if self.process:
            self.process.terminate()
            await self.process.wait()


@pytest.mark.asyncio
async def test_greedy_ai_full_game_with_info():
    """Test greedy AI playing a full game and sending info messages."""
    sim = GameSimulator("greedy_ai.py")
    try:
        await sim.start()

        # Handshake
        await sim.send("pyrat")
        responses = await sim.read_responses()
        assert any("id name" in r for r in responses)
        assert any("pyratready" in r for r in responses)

        # Game setup
        await sim.send("newgame")
        await sim.send("maze height:7 width:7")
        await sim.send("walls (2,2)-(2,3) (2,3)-(3,3) (3,3)-(4,3)")
        await sim.send("mud (1,1)-(2,1):3 (0,3)-(0,4):2")
        await sim.send("cheese (0,0) (3,2) (6,6)")
        await sim.send("player1 rat (0,0)")
        await sim.send("player2 python (6,6)")
        await sim.send("youare rat")

        # Wait for AI to process game setup
        await asyncio.sleep(0.1)

        # Preprocessing
        await sim.send("startpreprocessing t:1000")
        await asyncio.sleep(0.1)  # Give time for preprocessing
        responses = await sim.read_responses()
        assert any("preprocessingdone" in r for r in responses)

        # Verify preprocessing info messages
        preprocessing_info = [
            msg
            for msg in sim.info_messages
            if "Preprocessing" in msg or "Maze size" in msg
        ]
        assert len(preprocessing_info) > 0, "AI should send info during preprocessing"

        # Play several moves
        moves_made = []
        for turn in range(5):
            # Send move broadcast
            if turn == 0:
                await sim.send("moves rat:STAY python:STAY")
            else:
                last_move = moves_made[-1] if moves_made else "STAY"
                await sim.send(f"moves rat:{last_move} python:STAY")

            # Request move
            await sim.send("go t:500")
            responses = await sim.read_responses()

            # Find move response
            move_response = next((r for r in responses if r.startswith("move ")), None)
            assert move_response is not None, f"No move response on turn {turn}"

            move = move_response.split()[1]
            moves_made.append(move)

            # Note: Could check for strategy info messages in sim.info_messages

        # Verify we got strategic info messages
        assert (
            len(sim.info_messages) > 0
        ), "AI should send info messages during gameplay"

        # Check types of info messages
        has_target_info = any("target" in msg for msg in sim.info_messages)
        has_string_info = any("string" in msg for msg in sim.info_messages)

        assert (
            has_target_info or has_string_info
        ), "AI should send target or strategy info"

    finally:
        await sim.cleanup()


@pytest.mark.asyncio
async def test_greedy_ai_mud_handling_with_logging():
    """Test greedy AI handles mud correctly and logs about it."""
    sim = GameSimulator("greedy_ai.py")
    try:
        await sim.start()

        # Setup game with mud in the path
        await sim.send("pyrat")
        await sim.read_responses()

        await sim.send("newgame")
        await sim.send("maze height:5 width:5")
        await sim.send("walls")  # No walls
        await sim.send("mud (0,0)-(1,0):3")  # 3-turn mud right of start
        await sim.send("cheese (2,0)")  # Cheese requires going through mud
        await sim.send("player1 rat (0,0)")
        await sim.send("player2 python (4,4)")
        await sim.send("youare rat")
        await sim.send("startpreprocessing t:100")
        await sim.read_responses()

        # First move - should enter mud
        await sim.send("moves rat:STAY python:STAY")
        await sim.send("go t:100")
        responses = await sim.read_responses()

        move1 = next((r for r in responses if r.startswith("move ")), None)
        assert "RIGHT" in move1, "Should move right into mud"

        # Check for mud-related info
        mud_info = [msg for msg in sim.info_messages if "mud" in msg.lower()]
        assert len(mud_info) > 0, "Should have info about entering mud"

        # Next 3 moves - should be stuck in mud
        for turn in range(3):
            await sim.send("moves rat:RIGHT python:STAY")
            await sim.send("go t:100")
            responses = await sim.read_responses()

            move = next((r for r in responses if r.startswith("move ")), None)
            assert "STAY" in move, f"Should STAY while stuck in mud (turn {turn+1})"

            # Should log about being stuck
            stuck_info = [msg for msg in sim.info_messages if "stuck" in msg.lower()]
            assert len(stuck_info) > 0, "Should log about being stuck in mud"

    finally:
        await sim.cleanup()


@pytest.mark.asyncio
async def test_multiple_ais_with_info_messages():
    """Test multiple AIs playing and sending info messages."""
    # This would require a more complex setup with two AI processes
    # For now, just test that different AIs can send info

    for ai_name in ["greedy_ai.py", "random_ai.py"]:
        sim = GameSimulator(ai_name)
        try:
            await sim.start()

            # Quick handshake and game setup
            await sim.send("pyrat")
            await sim.read_responses()
            await sim.send("newgame")
            await sim.send("maze height:5 width:5")
            await sim.send("cheese (2,2)")
            await sim.send("player1 rat (0,0)")
            await sim.send("player2 python (4,4)")
            await sim.send("youare rat")
            await sim.send("startpreprocessing t:100")
            await sim.read_responses()

            # Make a move
            await sim.send("moves rat:STAY python:STAY")
            await sim.send("go t:100")
            await sim.read_responses()

            # Greedy AI should send info, random AI might not
            if "greedy" in ai_name:
                assert (
                    len(sim.info_messages) > 0
                ), f"{ai_name} should send info messages"

        finally:
            await sim.cleanup()


if __name__ == "__main__":
    # Run the tests
    pytest.main([__file__, "-v"])
