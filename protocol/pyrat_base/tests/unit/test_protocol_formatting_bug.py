"""Tests for protocol formatting bugs that were lost in git manipulation.

These tests ensure the critical bugs don't resurface:
1. Handshake response format mismatch
2. Move broadcast parsing error
"""

import pytest

from pyrat_base import Protocol
from pyrat_base.enums import ResponseType


class TestHandshakeFormatting:
    """Test correct handshake response formatting."""

    def test_id_response_format(self):
        """ID response must use correct format: {"name": value} not {"type": "name", "value": value}."""
        protocol = Protocol()

        # Correct format - current implementation uses "id name" format
        response = protocol.format_response(ResponseType.ID, {"name": "TestBot"})
        assert response == "id name TestBot"

        # The bug was passing wrong format - this should handle gracefully
        # Current implementation raises ValueError for missing keys
        with pytest.raises(ValueError) as exc_info:
            # This was the buggy format that caused crashes
            protocol.format_response(
                ResponseType.ID, {"type": "name", "value": "TestBot"}
            )
        assert "requires 'name' or 'author'" in str(exc_info.value)

    def test_id_response_with_author(self):
        """Test formatting ID response with author information."""
        protocol = Protocol()

        # Standard author format - using ID response type with author key
        response = protocol.format_response(ResponseType.ID, {"author": "Test Author"})
        assert response == "id author Test Author"

    def test_complete_handshake_sequence(self):
        """Test complete handshake response sequence."""
        protocol = Protocol()

        # Parse handshake command
        cmd = protocol.parse_command("pyrat")
        assert cmd.type.name == "PYRAT"

        # Format responses in correct order
        responses = []

        # 1. id name response with correct format
        responses.append(
            protocol.format_response(ResponseType.ID, {"name": "TestBot v1.0"})
        )

        # 2. id author (optional)
        responses.append(
            protocol.format_response(ResponseType.ID, {"author": "Tester"})
        )

        # 3. pyratready - this is sent directly as a string
        responses.append("pyratready")

        assert responses == ["id name TestBot v1.0", "id author Tester", "pyratready"]


class TestMoveBroadcastParsing:
    """Test correct move broadcast parsing."""

    def test_moves_command_parsing(self):
        """Moves command creates nested data structure with Player enum keys."""
        protocol = Protocol()

        # Parse moves broadcast
        cmd = protocol.parse_command("moves rat:UP python:DOWN")

        # The data structure should have a 'moves' key
        assert "moves" in cmd.data

        # Moves should be accessible by Player enum
        moves = cmd.data["moves"]
        from pyrat_base.enums import Player

        assert Player.RAT in moves
        assert Player.PYTHON in moves
        assert moves[Player.RAT] == "UP"
        assert moves[Player.PYTHON] == "DOWN"

    def test_moves_command_various_directions(self):
        """Test parsing moves with all possible directions."""
        protocol = Protocol()

        from pyrat_base.enums import Player

        test_cases = [
            ("moves rat:STAY python:STAY", {Player.RAT: "STAY", Player.PYTHON: "STAY"}),
            ("moves rat:UP python:DOWN", {Player.RAT: "UP", Player.PYTHON: "DOWN"}),
            (
                "moves rat:LEFT python:RIGHT",
                {Player.RAT: "LEFT", Player.PYTHON: "RIGHT"},
            ),
            (
                "moves rat:RIGHT python:LEFT",
                {Player.RAT: "RIGHT", Player.PYTHON: "LEFT"},
            ),
            ("moves rat:DOWN python:UP", {Player.RAT: "DOWN", Player.PYTHON: "UP"}),
        ]

        for command, expected_moves in test_cases:
            cmd = protocol.parse_command(command)
            assert cmd is not None
            assert "moves" in cmd.data
            assert cmd.data["moves"] == expected_moves

    def test_accessing_moves_safely(self):
        """Test safe access patterns for move data."""
        protocol = Protocol()
        cmd = protocol.parse_command("moves rat:UP python:DOWN")

        from pyrat_base.enums import Player

        # Safe access pattern that should work
        moves = cmd.data.get("moves", {})
        # Access with Player enum keys
        rat_move = moves.get(Player.RAT, "STAY")
        python_move = moves.get(Player.PYTHON, "STAY")

        assert rat_move == "UP"
        assert python_move == "DOWN"

        # The bug was trying to access cmd.data["rat"] directly
        # This should fail
        with pytest.raises(KeyError):
            _ = cmd.data["rat"]

        with pytest.raises(KeyError):
            _ = cmd.data["python"]


class TestProtocolRobustness:
    """Test that protocol handles edge cases without crashing."""

    def test_malformed_handshake_data(self):
        """Protocol should handle malformed handshake data gracefully."""
        protocol = Protocol()

        # Various malformed data that might be passed
        test_cases = [
            {},  # Empty dict
            {"wrong_key": "value"},  # Wrong key
            {"name": None},  # None value
            {"name": ""},  # Empty string
            {"name": 123},  # Wrong type
        ]

        for data in test_cases:
            # Should either work or raise a clear error, not crash mysteriously
            try:
                response = protocol.format_response(ResponseType.ID, data)
                # If it succeeds, check it's reasonable
                assert response.startswith("id name") or response.startswith(
                    "id author"
                )
            except (KeyError, TypeError, ValueError) as e:
                # These are acceptable errors that indicate the issue
                assert str(e)  # Error should have a message

    def test_move_parsing_edge_cases(self):
        """Test move parsing handles edge cases."""
        protocol = Protocol()

        # Valid format
        cmd = protocol.parse_command("moves rat:UP python:DOWN")
        assert cmd is not None

        # Invalid formats should return None or have clear errors
        invalid_commands = [
            "moves",  # No moves specified
            "moves rat:UP",  # Missing python move
            "moves python:DOWN",  # Missing rat move
            "moves rat: python:",  # Empty moves
            "moves rat:INVALID python:DOWN",  # Invalid direction
        ]

        for invalid in invalid_commands:
            cmd = protocol.parse_command(invalid)
            # Should either return None or parse partially
            # but not crash with cryptic errors
