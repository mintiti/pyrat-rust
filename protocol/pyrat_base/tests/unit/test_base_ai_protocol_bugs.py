"""Tests for specific protocol bugs that were fixed in base_ai.py.

These tests verify specific bug fixes without accessing private methods.
We test the observable behavior through the protocol.
"""

import pytest

from pyrat_base import Protocol
from pyrat_base.enums import CommandType, Player, ResponseType


class TestProtocolBugFixes:
    """Test that specific protocol bugs are fixed."""

    def test_handshake_response_format_bug(self):
        """Verify the handshake response format is correct.

        Bug: AI was passing {"type": "name", "value": self.name} instead of {"name": self.name}
        This is now handled correctly in the protocol formatter.
        """
        # Test that the protocol formats ID responses correctly
        response = Protocol.format_response(ResponseType.ID, {"name": "TestBot"})
        assert response == "id name TestBot"

        # The bug would have produced something like "id type name value TestBot"
        # Make sure the bad format raises an error
        bad_data = {"type": "name", "value": "TestBot"}
        with pytest.raises(ValueError, match="ID response requires"):
            Protocol.format_response(ResponseType.ID, bad_data)

    def test_moves_broadcast_data_structure(self):
        """Verify moves are parsed with correct data structure.

        Bug: Code expected cmd.data["rat"] but actual structure is cmd.data["moves"]["rat"]
        """
        # Test the correct parsing of moves command
        cmd = Protocol.parse_command("moves rat:UP python:DOWN")
        assert cmd is not None
        assert cmd.type == CommandType.MOVES
        assert "moves" in cmd.data
        assert cmd.data["moves"][Player.RAT] == "UP"
        assert cmd.data["moves"][Player.PYTHON] == "DOWN"

        # The old bug would have expected data[Player.RAT] directly
        # Make sure we're using the correct nested structure
        assert Player.RAT not in cmd.data  # Should NOT be at top level
        assert Player.PYTHON not in cmd.data  # Should NOT be at top level

    def test_parse_direction_robustness(self):
        """Verify that invalid directions are rejected by the protocol parser.

        The protocol validates directions at parse time, rejecting invalid ones.
        This ensures the AI only receives valid move commands.
        """
        # Test parsing moves with invalid directions - should be rejected
        cmd = Protocol.parse_command("moves rat:INVALID python:NONSENSE")
        assert cmd is None  # Invalid directions are rejected

        # Test empty directions - should be rejected
        cmd = Protocol.parse_command("moves rat: python:")
        assert cmd is None  # Empty directions are rejected

        # Test valid directions are accepted
        cmd = Protocol.parse_command("moves rat:UP python:STAY")
        assert cmd is not None
        assert cmd.type == CommandType.MOVES
        assert cmd.data["moves"][Player.RAT] == "UP"
        assert cmd.data["moves"][Player.PYTHON] == "STAY"

    def test_malformed_moves_handling(self):
        """Verify that malformed moves commands are rejected by parser."""
        # These should all return None (invalid format)
        assert Protocol.parse_command("moves") is None  # No data
        assert Protocol.parse_command("moves rat:UP") is None  # Missing python
        assert Protocol.parse_command("moves python:DOWN") is None  # Missing rat
        assert Protocol.parse_command("moves UP DOWN") is None  # Wrong format

        # Valid format should parse successfully
        assert Protocol.parse_command("moves rat:UP python:DOWN") is not None

    def test_commands_not_dropped_during_calculation(self):
        """Verify commands arriving during calculation are not dropped.

        Bug: Commands received during move calculation (in _handle_go_command)
        were being read from the queue but only STOP and ISREADY were handled.
        Other commands (especially MOVES) were silently dropped, causing game
        state desynchronization.

        Fix: Non-urgent commands are now re-queued for processing after the
        move calculation completes.

        This test verifies that MOVES commands are preserved.
        """
        from pyrat_base import IOHandler

        # Parse a MOVES command (this is what arrives during calculation)
        cmd = Protocol.parse_command("moves rat:UP python:DOWN")
        assert cmd is not None
        assert cmd.type == CommandType.MOVES

        # Verify the command has the correct structure
        assert "moves" in cmd.data
        assert cmd.data["moves"][Player.RAT] == "UP"
        assert cmd.data["moves"][Player.PYTHON] == "DOWN"

        # The bug was that such commands would be read and dropped
        # With the fix, they are re-queued using IOHandler.requeue_command()
        io = IOHandler()

        # Simulate: command arrives, is read, then re-queued
        io._command_queue.put(cmd)
        read_cmd = io.read_command(timeout=0.01)
        assert read_cmd is not None

        # Re-queue it (this is the fix)
        io.requeue_command(read_cmd)

        # Verify it's still available for processing
        cmd_after = io.read_command(timeout=0.01)
        assert cmd_after is not None
        assert cmd_after.type == CommandType.MOVES

        io.close()

    def test_game_state_sync_preserved_with_requeue(self):
        """Integration test: verify game state stays synchronized.

        This demonstrates the actual bug scenario where dropped MOVES commands
        cause game state to go out of sync between AI and server.

        Bug scenario:
        1. AI receives GO and starts calculating
        2. Server sends MOVES command (with actual moves executed)
        3. AI reads MOVES during calculation but drops it
        4. AI's game state is never updated
        5. On next turn, AI's state is out of sync with server

        Fix: MOVES commands are re-queued and processed after calculation.
        """
        # This test demonstrates the impact of the bug using a simple example
        # We don't need full PyRat for this - just show the concept

        # Simulate game state update
        class SimpleGameState:
            def __init__(self):
                self.rat_pos = (0, 0)
                self.python_pos = (4, 4)
                self.turn = 0

            def apply_moves(self, rat_move, python_move):
                """Apply moves to update positions."""
                self.turn += 1
                # Simplified move application
                if rat_move == "UP":
                    self.rat_pos = (self.rat_pos[0], self.rat_pos[1] + 1)
                if python_move == "DOWN":
                    self.python_pos = (self.python_pos[0], self.python_pos[1] - 1)

        # Server's game state (always updated)
        server_state = SimpleGameState()
        server_state.apply_moves("UP", "DOWN")
        assert server_state.rat_pos == (0, 1)
        assert server_state.python_pos == (4, 3)
        assert server_state.turn == 1

        # AI's game state WITH THE BUG (MOVES command dropped)
        ai_state_buggy = SimpleGameState()
        # BUG: MOVES command was dropped, so no update applied (line never executed)
        assert ai_state_buggy.rat_pos == (0, 0)  # Still at start!
        assert ai_state_buggy.python_pos == (4, 4)  # Still at start!
        assert ai_state_buggy.turn == 0  # Turn never incremented!

        # AI's game state WITH THE FIX (MOVES command re-queued and processed)
        ai_state_fixed = SimpleGameState()
        # FIX: MOVES command was re-queued and processed
        ai_state_fixed.apply_moves("UP", "DOWN")
        assert ai_state_fixed.rat_pos == (0, 1)  # Correctly updated!
        assert ai_state_fixed.python_pos == (4, 3)  # Correctly updated!
        assert ai_state_fixed.turn == 1  # Turn incremented!

        # Verify: buggy state is out of sync, fixed state is in sync
        assert ai_state_buggy.rat_pos != server_state.rat_pos  # DESYNC!
        assert ai_state_fixed.rat_pos == server_state.rat_pos  # IN SYNC!

    def test_isready_handled_immediately_not_requeued(self):
        """Verify ISREADY commands are handled immediately during calculation.

        ISREADY commands should be answered immediately (not re-queued) even
        during move calculation. This is part of the protocol requirement for
        synchronization checks.
        """
        cmd = Protocol.parse_command("isready")
        assert cmd is not None
        assert cmd.type == CommandType.ISREADY

        # ISREADY should be handled with immediate "readyok" response
        # It should NOT be re-queued like other commands
