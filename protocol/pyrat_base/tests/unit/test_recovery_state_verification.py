#!/usr/bin/env python3
"""Test state verification during recovery protocol."""

from pyrat_base.enums import Player
from pyrat_base.protocol import CommandType, Protocol


class TestRecoveryStateVerification:
    """Test that recovery commands verify state correctly."""

    def test_parse_current_position_command(self):
        """Test parsing of current_position command."""
        protocol = Protocol()

        cmd = protocol.parse_command("current_position rat:(2,3) python:(7,8)")

        assert cmd is not None
        assert cmd.type == CommandType.CURRENT_POSITION
        assert "positions" in cmd.data
        assert cmd.data["positions"][Player.RAT] == (2, 3)
        assert cmd.data["positions"][Player.PYTHON] == (7, 8)

    def test_parse_score_command(self):
        """Test parsing of score command."""
        protocol = Protocol()

        cmd = protocol.parse_command("score rat:3 python:2")

        assert cmd is not None
        assert cmd.type == CommandType.SCORE
        assert "scores" in cmd.data
        assert cmd.data["scores"][Player.RAT] == 3
        assert cmd.data["scores"][Player.PYTHON] == 2

    def test_parse_score_command_with_half_points(self):
        """Test parsing score with half points from simultaneous collection."""
        protocol = Protocol()

        cmd = protocol.parse_command("score rat:2.5 python:1.5")

        assert cmd is not None
        assert cmd.type == CommandType.SCORE
        assert "scores" in cmd.data
        assert cmd.data["scores"][Player.RAT] == 2.5
        assert cmd.data["scores"][Player.PYTHON] == 1.5

    def test_current_position_invalid_format(self):
        """Test parsing fails with invalid position format."""
        protocol = Protocol()

        # Missing colon after rat
        cmd = protocol.parse_command("current_position rat(2,3) python:(7,8)")
        assert cmd is None

        # Missing parentheses
        cmd = protocol.parse_command("current_position rat:2,3 python:7,8")
        assert cmd is None

    def test_score_invalid_format(self):
        """Test parsing fails with invalid score format."""
        protocol = Protocol()

        # Non-numeric score
        cmd = protocol.parse_command("score rat:three python:2")
        assert cmd is None

        # Missing colon
        cmd = protocol.parse_command("score rat 3 python 2")
        assert cmd is None

    def test_recovery_command_sequence(self):
        """Test a typical recovery command sequence parses correctly."""
        protocol = Protocol()

        # Typical recovery sequence after restart
        commands = [
            "recover",
            "maze height:15 width:21",
            "walls (0,0)-(0,1) (1,1)-(2,1)",
            "mud (5,5)-(5,6):3",
            "cheese (2,2) (7,8)",  # Only remaining cheese
            "moves_history UP DOWN LEFT RIGHT STAY STAY",
            "current_position rat:(1,1) python:(19,13)",
            "score rat:1 python:0",
        ]

        parsed_commands = []
        for cmd_str in commands:
            cmd = protocol.parse_command(cmd_str)
            assert cmd is not None, f"Failed to parse: {cmd_str}"
            parsed_commands.append(cmd)

        # Verify the sequence
        assert parsed_commands[0].type == CommandType.RECOVER
        assert parsed_commands[1].type == CommandType.MAZE
        assert parsed_commands[2].type == CommandType.WALLS
        assert parsed_commands[3].type == CommandType.MUD
        assert parsed_commands[4].type == CommandType.CHEESE
        assert parsed_commands[5].type == CommandType.MOVES_HISTORY
        assert parsed_commands[6].type == CommandType.CURRENT_POSITION
        assert parsed_commands[7].type == CommandType.SCORE

        # Verify specific data
        assert parsed_commands[5].data["history"] == [
            "UP",
            "DOWN",
            "LEFT",
            "RIGHT",
            "STAY",
            "STAY",
        ]
        assert parsed_commands[6].data["positions"][Player.RAT] == (1, 1)
        assert parsed_commands[6].data["positions"][Player.PYTHON] == (19, 13)
        assert parsed_commands[7].data["scores"][Player.RAT] == 1
        assert parsed_commands[7].data["scores"][Player.PYTHON] == 0
