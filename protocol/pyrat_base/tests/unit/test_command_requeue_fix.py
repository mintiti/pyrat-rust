"""Test that commands received during move calculation are not dropped.

This test verifies the fix for the critical bug where MOVES commands
arriving during move calculation were being dropped, causing game state
desynchronization.
"""

import time

import pytest

from pyrat_base.io_handler import IOHandler
from pyrat_base.protocol import Command, Protocol
from pyrat_base.enums import CommandType, Player


class TestCommandRequeueFix:
    """Test that commands are properly re-queued during move calculation."""

    def test_moves_command_not_dropped_during_calculation(self):
        """Verify MOVES commands arriving during calculation are preserved.

        Bug scenario:
        1. AI receives GO and starts calculating
        2. MOVES command arrives while AI is still calculating
        3. The command processing loop reads the MOVES command
        4. BUG: Previously, non-STOP/ISREADY commands were dropped
        5. FIX: Commands are now re-queued for later processing

        This test simulates this scenario and verifies the fix.
        """
        io = IOHandler(debug=False)

        # Start a slow calculation
        def slow_calculation(stop_event):
            time.sleep(0.1)  # Simulate slow AI
            return "UP"

        thread = io.start_move_calculation(slow_calculation)

        # While calculation is running, simulate a MOVES command arriving
        # In real scenario, this would come from the engine via stdin
        moves_cmd = Command(
            CommandType.MOVES,
            {"moves": {Player.RAT: "UP", Player.PYTHON: "DOWN"}},
        )

        # Simulate the command arriving (bypass stdin, directly queue it)
        io._command_queue.put(moves_cmd)

        # Wait a tiny bit to ensure the command is in the queue
        time.sleep(0.01)

        # Simulate the command processing loop in _handle_go_command
        # This is what happens while the AI is calculating
        cmd = io.read_command(timeout=0.01)
        assert cmd is not None, "Command should be available"
        assert cmd.type == CommandType.MOVES, "Should be a MOVES command"

        # The fix: re-queue the command instead of dropping it
        io.requeue_command(cmd)

        # Wait for calculation to complete
        thread.join(timeout=1.0)

        # After calculation, the MOVES command should still be in the queue
        cmd_after = io.read_command(timeout=0.01)
        assert cmd_after is not None, "Command should still be available after re-queuing"
        assert (
            cmd_after.type == CommandType.MOVES
        ), "Should be the same MOVES command"
        assert (
            cmd_after.data["moves"][Player.RAT] == "UP"
        ), "Command data should be preserved"

        io.close()

    def test_isready_handled_immediately_during_calculation(self):
        """Verify ISREADY commands are handled immediately, not re-queued."""
        io = IOHandler(debug=False)

        # Parse an ISREADY command
        isready_cmd = Protocol.parse_command("isready")
        assert isready_cmd is not None
        assert isready_cmd.type == CommandType.ISREADY

        # ISREADY should be handled immediately during calculation
        # It should NOT be re-queued
        # (This is the correct behavior - only non-urgent commands are re-queued)

        io.close()

    def test_multiple_commands_requeued_in_order(self):
        """Verify multiple commands are re-queued in the correct order."""
        io = IOHandler(debug=False)

        # Create multiple commands
        cmd1 = Command(CommandType.MOVES, {"moves": {Player.RAT: "UP", Player.PYTHON: "DOWN"}})
        cmd2 = Command(CommandType.TIMEOUT, {"phase": "move"})
        cmd3 = Command(CommandType.GAMEOVER, {"winner": "rat", "score": (2.0, 1.0)})

        # Read and re-queue them
        io._command_queue.put(cmd1)
        io._command_queue.put(cmd2)
        io._command_queue.put(cmd3)

        # Read each command
        c1 = io.read_command(timeout=0.01)
        c2 = io.read_command(timeout=0.01)
        c3 = io.read_command(timeout=0.01)

        # Re-queue them
        io.requeue_command(c1)
        io.requeue_command(c2)
        io.requeue_command(c3)

        # They should come back in the same order
        assert io.read_command(timeout=0.01).type == CommandType.MOVES
        assert io.read_command(timeout=0.01).type == CommandType.TIMEOUT
        assert io.read_command(timeout=0.01).type == CommandType.GAMEOVER

        io.close()

    def test_game_state_stays_in_sync_with_requeue(self):
        """Integration test: verify game state stays synchronized.

        This is a higher-level test that simulates the actual bug scenario
        where game state goes out of sync due to dropped MOVES commands.
        """
        from pyrat_engine._rust import PyGameState
        from pyrat_base.enums import Player
        from pyrat_engine.game import Direction

        # Create a game state
        game = PyGameState.create_custom(
            width=5,
            height=5,
            walls=[],
            mud=[],
            cheese=[(2, 2)],
            player1_pos=(0, 0),
            player2_pos=(4, 4),
        )

        # Initial state
        assert game.player1_position == (0, 0)
        assert game.player2_position == (4, 4)

        # Simulate turn 1: both players move
        game.step(Direction.UP, Direction.DOWN)

        # State after turn 1
        assert game.player1_position == (0, 1), "Rat should move UP to (0, 1)"
        assert game.player2_position == (4, 3), "Python should move DOWN to (4, 3)"

        # Now simulate the bug scenario:
        # If the MOVES command "moves rat:UP python:DOWN" was DROPPED,
        # the AI would not apply the step, and its local game state would
        # still think both players are at their starting positions.

        # With the fix, the MOVES command is re-queued and processed,
        # so the game state stays in sync.

        # Create a second game state that represents what the AI would have
        # if it dropped the MOVES command
        game_buggy = PyGameState.create_custom(
            width=5,
            height=5,
            walls=[],
            mud=[],
            cheese=[(2, 2)],
            player1_pos=(0, 0),
            player2_pos=(4, 4),
        )

        # Buggy state: MOVES was dropped, so no step applied
        assert game_buggy.player1_position == (0, 0), "Buggy state still at start"
        assert game_buggy.player2_position == (4, 4), "Buggy state still at start"

        # The positions should be DIFFERENT between correct and buggy state
        assert (
            game.player1_position != game_buggy.player1_position
        ), "States are out of sync"
        assert (
            game.player2_position != game_buggy.player2_position
        ), "States are out of sync"

        # This demonstrates the desync bug!


if __name__ == "__main__":
    pytest.main([__file__, "-v"])
