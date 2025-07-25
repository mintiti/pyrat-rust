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
