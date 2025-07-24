#!/usr/bin/env python3
"""Test for moves_history protocol bug fix."""

from pyrat_base.protocol import CommandType, Protocol


class TestMovesHistoryBug:
    """Test that moves_history parsing and handling work correctly."""

    def test_parse_moves_history_command(self):
        """Test parsing of moves_history command returns correct format."""
        protocol = Protocol()

        # Test with a sequence of moves
        cmd = protocol.parse_command("moves_history UP DOWN LEFT RIGHT STAY UP")

        assert cmd is not None
        assert cmd.type == CommandType.MOVES_HISTORY
        assert "history" in cmd.data
        assert cmd.data["history"] == ["UP", "DOWN", "LEFT", "RIGHT", "STAY", "UP"]

    def test_parse_moves_history_empty(self):
        """Test parsing moves_history with no moves."""
        protocol = Protocol()

        cmd = protocol.parse_command("moves_history")

        assert cmd is not None
        assert cmd.type == CommandType.MOVES_HISTORY
        assert cmd.data["history"] == []

    def test_parse_moves_history_invalid_move(self):
        """Test parsing moves_history with invalid move."""
        protocol = Protocol()

        # Invalid move should cause parse failure
        cmd = protocol.parse_command("moves_history UP INVALID DOWN")

        assert cmd is None

    def test_moves_history_pairing(self):
        """Test that moves can be paired up for rat/python."""
        protocol = Protocol()

        cmd = protocol.parse_command("moves_history UP DOWN LEFT RIGHT")
        assert cmd is not None

        # Show how to pair moves for game replay
        moves = cmd.data["history"]
        paired_moves = [(moves[i], moves[i + 1]) for i in range(0, len(moves), 2)]

        assert paired_moves == [("UP", "DOWN"), ("LEFT", "RIGHT")]

    def test_moves_history_odd_number(self):
        """Test moves_history with odd number of moves (incomplete turn)."""
        protocol = Protocol()

        cmd = protocol.parse_command("moves_history UP DOWN LEFT")
        assert cmd is not None

        moves = cmd.data["history"]
        assert len(moves) == 3

        # When pairing, the last move would be incomplete
        paired_moves = []
        for i in range(0, len(moves) - 1, 2):
            paired_moves.append((moves[i], moves[i + 1]))

        # Check if there's an unpaired move
        if len(moves) % 2 == 1:
            last_move = moves[-1]
            assert last_move == "LEFT"

        assert paired_moves == [("UP", "DOWN")]
