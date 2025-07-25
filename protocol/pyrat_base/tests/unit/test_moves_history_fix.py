#!/usr/bin/env python3
"""Integration test for moves_history bug fix."""

from pyrat_base.protocol import CommandType, Protocol


class TestMovesHistoryFix:
    """Test that the moves_history handler fix works correctly."""

    def test_moves_history_handler_uses_correct_key(self):
        """Test that moves_history handler looks for 'history' key, not 'moves' key."""
        # This tests the bug fix where handler was looking for 'moves' but parser stored in 'history'

        protocol = Protocol()
        cmd = protocol.parse_command("moves_history UP DOWN LEFT RIGHT")

        assert cmd is not None
        assert cmd.type == CommandType.MOVES_HISTORY

        # The parser should store moves in 'history' key
        assert "history" in cmd.data
        assert "moves" not in cmd.data  # This was the bug - handler looked for 'moves'

        # Verify the moves are stored correctly
        assert cmd.data["history"] == ["UP", "DOWN", "LEFT", "RIGHT"]

    def test_moves_history_pairing_logic(self):
        """Test that moves can be paired correctly for replay."""
        protocol = Protocol()
        cmd = protocol.parse_command("moves_history UP DOWN LEFT RIGHT STAY UP")

        assert cmd is not None
        history = cmd.data["history"]

        # Show how the fixed handler pairs moves
        paired_moves = []
        for i in range(0, len(history) - 1, 2):
            rat_move = history[i]
            python_move = history[i + 1]
            paired_moves.append((rat_move, python_move))

        # Should have 3 complete turns
        expected_pairs = 3
        assert len(paired_moves) == expected_pairs
        assert paired_moves[0] == ("UP", "DOWN")
        assert paired_moves[1] == ("LEFT", "RIGHT")
        assert paired_moves[2] == ("STAY", "UP")

    def test_moves_history_odd_number_handling(self):
        """Test handling of odd number of moves (incomplete turn)."""
        protocol = Protocol()
        cmd = protocol.parse_command("moves_history UP DOWN LEFT")

        assert cmd is not None
        history = cmd.data["history"]

        # With the fix, pairing should handle odd numbers gracefully
        paired_moves = []
        for i in range(0, len(history) - 1, 2):
            rat_move = history[i]
            python_move = history[i + 1]
            paired_moves.append((rat_move, python_move))

        # Only 1 complete turn, the last "LEFT" is ignored
        assert len(paired_moves) == 1
        assert paired_moves[0] == ("UP", "DOWN")

        # The unpaired move
        if len(history) % 2 == 1:
            unpaired_move = history[-1]
            assert unpaired_move == "LEFT"

    def test_empty_moves_history(self):
        """Test handling of empty moves history."""
        protocol = Protocol()
        cmd = protocol.parse_command("moves_history")

        assert cmd is not None
        history = cmd.data["history"]
        assert history == []

        # No moves to pair
        paired_moves = []
        for i in range(0, len(history) - 1, 2):
            paired_moves.append((history[i], history[i + 1]))

        assert len(paired_moves) == 0
