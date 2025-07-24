"""Tests for parsing protocol commands.

These tests verify that text commands from the engine are correctly parsed
into structured command objects that the AI can process.
"""

import pytest

from pyrat_base import CommandType, Player, Protocol


class TestHandshakeCommands:
    """Tests for parsing handshake phase commands.

    Protocol spec: Handshake occurs at AI startup to identify capabilities.
    """

    def test_parse_pyrat(self):
        """PYRAT command initiates the handshake sequence."""
        cmd = Protocol.parse_command("pyrat")
        assert cmd is not None
        assert cmd.type == CommandType.PYRAT
        assert cmd.data == {}

    def test_parse_pyrat_case_insensitive(self):
        """Protocol commands are case-insensitive for robustness."""
        cmd = Protocol.parse_command("PYRAT")
        assert cmd is not None
        assert cmd.type == CommandType.PYRAT

    def test_parse_isready(self):
        """ISREADY checks if AI is responsive."""
        cmd = Protocol.parse_command("isready")
        assert cmd is not None
        assert cmd.type == CommandType.ISREADY
        assert cmd.data == {}

    @pytest.mark.parametrize(
        "command,expected_data",
        [
            (
                "setoption name SearchDepth value 5",
                {"name": "SearchDepth", "value": "5"},
            ),
            (
                "setoption name Search Depth value 5",
                {"name": "Search Depth", "value": "5"},
            ),
            (
                "setoption name Strategy value Aggressive Play",
                {"name": "Strategy", "value": "Aggressive Play"},
            ),
        ],
        ids=["simple_option", "name_with_space", "value_with_space"],
    )
    def test_parse_setoption_valid(self, command, expected_data):
        """SETOPTION configures AI parameters before games."""
        cmd = Protocol.parse_command(command)
        assert cmd is not None
        assert cmd.type == CommandType.SETOPTION
        assert cmd.data == expected_data

    @pytest.mark.parametrize(
        "command",
        [
            "setoption",
            "setoption name",
            "setoption name Test",
            "setoption name Test val 5",  # Should be 'value' not 'val'
        ],
    )
    def test_parse_setoption_invalid(self, command):
        """SETOPTION requires both name and value keywords."""
        assert Protocol.parse_command(command) is None

    @pytest.mark.parametrize(
        "command,enabled",
        [
            ("debug on", True),
            ("debug off", False),
        ],
    )
    def test_parse_debug(self, command, enabled):
        """DEBUG toggles verbose output mode."""
        cmd = Protocol.parse_command(command)
        assert cmd is not None
        assert cmd.type == CommandType.DEBUG
        assert cmd.data == {"enabled": enabled}

    @pytest.mark.parametrize(
        "command",
        [
            "debug",
            "debug yes",
            "debug on off",
        ],
    )
    def test_parse_debug_invalid(self, command):
        """DEBUG requires exactly 'on' or 'off'."""
        assert Protocol.parse_command(command) is None


class TestGameInitCommands:
    """Tests for parsing game initialization commands.

    Protocol spec: After handshake, engine sends game setup information.
    """

    def test_parse_newgame(self):
        """NEWGAME signals start of a new game setup."""
        cmd = Protocol.parse_command("newgame")
        assert cmd is not None
        assert cmd.type == CommandType.NEWGAME
        assert cmd.data == {}

    @pytest.mark.parametrize(
        "command,expected_data",
        [
            ("maze height:10 width:15", {"height": 10, "width": 15}),
            ("maze width:20 height:5", {"height": 5, "width": 20}),
        ],
        ids=["height_first", "width_first"],
    )
    def test_parse_maze_valid(self, command, expected_data):
        """MAZE defines the game board dimensions."""
        cmd = Protocol.parse_command(command)
        assert cmd is not None
        assert cmd.type == CommandType.MAZE
        assert cmd.data == expected_data

    @pytest.mark.parametrize(
        "command,reason",
        [
            ("maze", "no_dimensions"),
            ("maze height:10", "missing_width"),
            ("maze width:15", "missing_height"),
            ("maze height:abc width:10", "non_numeric_height"),
            ("maze h:10 w:15", "wrong_keywords"),
        ],
    )
    def test_parse_maze_invalid(self, command, reason):
        """MAZE requires both numeric width and height."""
        assert Protocol.parse_command(command) is None

    @pytest.mark.parametrize(
        "command,expected_walls",
        [
            ("walls", []),
            ("walls (0,0)-(0,1)", [((0, 0), (0, 1))]),
            (
                "walls (0,0)-(0,1) (1,1)-(2,1) (3,3)-(3,4)",
                [((0, 0), (0, 1)), ((1, 1), (2, 1)), ((3, 3), (3, 4))],
            ),
        ],
        ids=["empty", "single", "multiple"],
    )
    def test_parse_walls_valid(self, command, expected_walls):
        """WALLS defines impassable barriers between cells."""
        cmd = Protocol.parse_command(command)
        assert cmd is not None
        assert cmd.type == CommandType.WALLS
        assert cmd.data == {"walls": expected_walls}

    @pytest.mark.parametrize(
        "command",
        [
            "walls (0,0)",
            "walls (0,0)-(0,1",
            "walls (0,0)-(a,b)",
            "walls (0,0,0)-(1,1)",
        ],
    )
    def test_parse_walls_invalid(self, command):
        """WALLS requires valid coordinate pairs."""
        assert Protocol.parse_command(command) is None

    @pytest.mark.parametrize(
        "command,expected_mud",
        [
            ("mud", []),
            ("mud (5,5)-(5,6):3", [((5, 5), (5, 6), 3)]),
            (
                "mud (1,1)-(1,2):2 (3,3)-(4,3):5",
                [((1, 1), (1, 2), 2), ((3, 3), (4, 3), 5)],
            ),
        ],
        ids=["empty", "single", "multiple"],
    )
    def test_parse_mud_valid(self, command, expected_mud):
        """MUD defines passages that take multiple turns to traverse."""
        cmd = Protocol.parse_command(command)
        assert cmd is not None
        assert cmd.type == CommandType.MUD
        assert cmd.data == {"mud": expected_mud}

    @pytest.mark.parametrize(
        "command",
        [
            "mud (5,5)-(5,6)",  # Missing cost
            "mud (5,5)-(5,6):",  # Empty cost
            "mud (5,5)-(5,6):abc",  # Non-numeric cost
        ],
    )
    def test_parse_mud_invalid(self, command):
        """MUD requires numeric traversal cost."""
        assert Protocol.parse_command(command) is None

    @pytest.mark.parametrize(
        "command,expected_cheese",
        [
            ("cheese", []),
            ("cheese (2,3)", [(2, 3)]),
            ("cheese (2,2) (7,8) (4,5)", [(2, 2), (7, 8), (4, 5)]),
        ],
        ids=["empty", "single", "multiple"],
    )
    def test_parse_cheese_valid(self, command, expected_cheese):
        """CHEESE defines collectible scoring items."""
        cmd = Protocol.parse_command(command)
        assert cmd is not None
        assert cmd.type == CommandType.CHEESE
        assert cmd.data == {"cheese": expected_cheese}

    def test_parse_player1(self):
        """PLAYER1 sets rat's starting position."""
        cmd = Protocol.parse_command("player1 rat (9,9)")
        assert cmd is not None
        assert cmd.type == CommandType.PLAYER1
        assert cmd.data == {"position": (9, 9)}

    def test_parse_player2(self):
        """PLAYER2 sets python's starting position."""
        cmd = Protocol.parse_command("player2 python (0,0)")
        assert cmd is not None
        assert cmd.type == CommandType.PLAYER2
        assert cmd.data == {"position": (0, 0)}

    @pytest.mark.parametrize(
        "command",
        [
            "player1 python (9,9)",  # Wrong player
            "player2 rat (0,0)",  # Wrong player
        ],
    )
    def test_parse_player_wrong_type(self, command):
        """Player commands must match expected player type."""
        assert Protocol.parse_command(command) is None

    @pytest.mark.parametrize(
        "command,expected_player",
        [
            ("youare rat", Player.RAT),
            ("youare python", Player.PYTHON),
            ("youare PYTHON", Player.PYTHON),  # Case insensitive
        ],
    )
    def test_parse_youare_valid(self, command, expected_player):
        """YOUARE tells AI which player it controls."""
        cmd = Protocol.parse_command(command)
        assert cmd is not None
        assert cmd.type == CommandType.YOUARE
        assert cmd.data == {"player": expected_player}

    @pytest.mark.parametrize(
        "command",
        [
            "youare",
            "youare snake",
            "youare rat python",
        ],
    )
    def test_parse_youare_invalid(self, command):
        """YOUARE requires valid player name (rat or python)."""
        assert Protocol.parse_command(command) is None

    @pytest.mark.parametrize(
        "command,expected_data",
        [
            (
                "timecontrol move:100 preprocessing:3000 postprocessing:1000",
                {"move": 100, "preprocessing": 3000, "postprocessing": 1000},
            ),
            ("timecontrol move:50", {"move": 50}),
            (
                "timecontrol preprocessing:5000 move:200",
                {"move": 200, "preprocessing": 5000},
            ),
        ],
        ids=["full", "partial", "different_order"],
    )
    def test_parse_timecontrol_valid(self, command, expected_data):
        """TIMECONTROL sets time limits for different phases."""
        cmd = Protocol.parse_command(command)
        assert cmd is not None
        assert cmd.type == CommandType.TIMECONTROL
        assert cmd.data == expected_data


class TestGameplayCommands:
    """Tests for parsing in-game commands.

    Protocol spec: Commands sent during active gameplay.
    """

    def test_parse_startpreprocessing(self):
        """STARTPREPROCESSING begins maze analysis phase."""
        cmd = Protocol.parse_command("startpreprocessing")
        assert cmd is not None
        assert cmd.type == CommandType.STARTPREPROCESSING
        assert cmd.data == {}

    @pytest.mark.parametrize(
        "command,expected_moves",
        [
            ("moves rat:UP python:DOWN", {Player.RAT: "UP", Player.PYTHON: "DOWN"}),
            (
                "moves python:LEFT rat:RIGHT",
                {Player.RAT: "RIGHT", Player.PYTHON: "LEFT"},
            ),
            ("moves rat:STAY python:STAY", {Player.RAT: "STAY", Player.PYTHON: "STAY"}),
        ],
        ids=["rat_first", "python_first", "both_stay"],
    )
    def test_parse_moves_valid(self, command, expected_moves):
        """MOVES reports what both players did last turn."""
        cmd = Protocol.parse_command(command)
        assert cmd is not None
        assert cmd.type == CommandType.MOVES
        assert cmd.data == {"moves": expected_moves}

    @pytest.mark.parametrize(
        "command",
        [
            "moves ratUP python:DOWN",  # Missing colon
            "moves rat:UP pythonDOWN",  # Missing colon
            "moves cat:UP python:DOWN",  # Invalid player
            "moves rat:UP dog:DOWN",  # Invalid player
            "moves rat:UP rat:DOWN",  # Duplicate player
            "moves python:UP python:DOWN",  # Duplicate player
        ],
    )
    def test_parse_moves_invalid(self, command):
        """MOVES requires both players with valid moves."""
        assert Protocol.parse_command(command) is None

    def test_parse_go(self):
        """GO requests AI to calculate and return a move."""
        cmd = Protocol.parse_command("go")
        assert cmd is not None
        assert cmd.type == CommandType.GO
        assert cmd.data == {}

    def test_parse_stop(self):
        """STOP interrupts AI calculation."""
        cmd = Protocol.parse_command("stop")
        assert cmd is not None
        assert cmd.type == CommandType.STOP
        assert cmd.data == {}

    @pytest.mark.parametrize(
        "command,expected_data",
        [
            ("timeout move:STAY", {"move": "STAY"}),
            ("timeout move:UP", {"move": "UP"}),
            ("timeout preprocessing", {"phase": "preprocessing"}),
            ("timeout postprocessing", {"phase": "postprocessing"}),
        ],
        ids=["with_stay", "with_move", "preprocessing", "postprocessing"],
    )
    def test_parse_timeout_valid(self, command, expected_data):
        """TIMEOUT indicates AI failed to respond in time."""
        cmd = Protocol.parse_command(command)
        assert cmd is not None
        assert cmd.type == CommandType.TIMEOUT
        assert cmd.data == expected_data

    @pytest.mark.parametrize(
        "command",
        [
            "timeout",
            "timeout move:INVALID",
            "timeout invalid",
        ],
    )
    def test_parse_timeout_invalid(self, command):
        """TIMEOUT requires valid phase or move."""
        assert Protocol.parse_command(command) is None

    def test_parse_ready(self):
        """READY checks AI responsiveness after timeout."""
        cmd = Protocol.parse_command("ready?")
        assert cmd is not None
        assert cmd.type == CommandType.READY
        assert cmd.data == {}

    def test_parse_current_position(self):
        """CURRENT_POSITION used during recovery to report player locations."""
        cmd = Protocol.parse_command("current_position rat:(5,3) python:(2,7)")
        assert cmd is not None
        assert cmd.type == CommandType.CURRENT_POSITION
        assert cmd.data == {"positions": {Player.RAT: (5, 3), Player.PYTHON: (2, 7)}}

        # Invalid formats
        assert Protocol.parse_command("current_position") is None
        assert Protocol.parse_command("current_position rat:(5,3)") is None
        assert Protocol.parse_command("current_position rat:5,3 python:(2,7)") is None

    def test_parse_score(self):
        """SCORE used during recovery to report current scores."""
        cmd = Protocol.parse_command("score rat:3 python:2")
        assert cmd is not None
        assert cmd.type == CommandType.SCORE
        assert cmd.data == {"scores": {Player.RAT: 3.0, Player.PYTHON: 2.0}}

        # Decimal scores
        cmd = Protocol.parse_command("score rat:2.5 python:1.5")
        assert cmd is not None
        assert cmd.data == {"scores": {Player.RAT: 2.5, Player.PYTHON: 1.5}}

        # Invalid formats
        assert Protocol.parse_command("score") is None
        assert Protocol.parse_command("score rat:3") is None
        assert Protocol.parse_command("score rat:abc python:2") is None


class TestGameEndCommands:
    """Tests for parsing game termination commands.

    Protocol spec: Commands sent when game concludes.
    """

    def test_parse_gameover(self):
        """GAMEOVER announces final game result per spec format."""
        # According to original test_protocol.py, the parser converts to GameResult enum and tuple
        from pyrat_base import GameResult

        cmd = Protocol.parse_command("gameover winner:rat score:3-2")
        assert cmd is not None
        assert cmd.type == CommandType.GAMEOVER
        assert cmd.data == {"winner": GameResult.RAT, "score": (3.0, 2.0)}

        cmd = Protocol.parse_command("gameover winner:python score:1-0")
        assert cmd is not None
        assert cmd.data == {"winner": GameResult.PYTHON, "score": (1.0, 0.0)}

        cmd = Protocol.parse_command("gameover winner:draw score:2.5-2.5")
        assert cmd is not None
        assert cmd.data == {"winner": GameResult.DRAW, "score": (2.5, 2.5)}

    @pytest.mark.parametrize(
        "command",
        [
            "gameover",
            "gameover winner:rat",
            "gameover score:3-2",
            "gameover winner:invalid score:3-2",
            "gameover winner:rat score:3",
        ],
    )
    def test_parse_gameover_invalid(self, command):
        """GAMEOVER requires both winner and score in correct format."""
        assert Protocol.parse_command(command) is None

    def test_parse_startpostprocessing(self):
        """STARTPOSTPROCESSING begins learning phase."""
        cmd = Protocol.parse_command("startpostprocessing")
        assert cmd is not None
        assert cmd.type == CommandType.STARTPOSTPROCESSING
        assert cmd.data == {}

    def test_parse_recover(self):
        """RECOVER requests game state after crash/restart."""
        cmd = Protocol.parse_command("recover")
        assert cmd is not None
        assert cmd.type == CommandType.RECOVER
        assert cmd.data == {}

    def test_parse_moves_history(self):
        """MOVES_HISTORY provides all moves for recovery per spec."""
        # Check original implementation to understand format
        # For now, let's mark this as needs investigation
        # The spec says "moves_history [list of all moves]" but doesn't specify format
        pass  # TODO: Investigate actual parser implementation for moves_history


class TestParsingEdgeCases:
    """Tests for edge cases and robustness in command parsing."""

    @pytest.mark.parametrize(
        "command",
        [
            "",
            "   ",
            "\n",
            "\t",
            "\r\n",
        ],
    )
    def test_parse_empty_or_whitespace(self, command):
        """Empty and whitespace-only lines are ignored."""
        assert Protocol.parse_command(command) is None

    @pytest.mark.parametrize(
        "command",
        [
            "invalid",
            "notacommand",
            "123",
            "!@#$",
        ],
    )
    def test_parse_unknown_command(self, command):
        """Unknown commands return None without crashing."""
        assert Protocol.parse_command(command) is None

    def test_parse_extra_whitespace(self):
        """Commands should handle extra whitespace gracefully."""
        cmd = Protocol.parse_command("  pyrat  ")
        assert cmd is not None
        assert cmd.type == CommandType.PYRAT

        cmd = Protocol.parse_command("maze   height:10   width:15")
        assert cmd is not None
        assert cmd.data == {"height": 10, "width": 15}

    @pytest.mark.parametrize(
        "command,expected_type",
        [
            ("PYRAT", CommandType.PYRAT),
            ("NEWGAME", CommandType.NEWGAME),
            ("ISREADY", CommandType.ISREADY),
        ],
    )
    def test_parse_case_insensitive_commands(self, command, expected_type):
        """Most commands are case-insensitive for robustness."""
        cmd = Protocol.parse_command(command)
        assert cmd is not None
        assert cmd.type == expected_type

    def test_parse_numeric_conversion_errors(self):
        """Non-numeric values in numeric fields return None."""
        assert Protocol.parse_command("maze height:abc width:10") is None
        assert Protocol.parse_command("timecontrol move:100ms") is None
        assert Protocol.parse_command("mud (0,0)-(0,1):three") is None

    @pytest.mark.parametrize(
        "pos_str,expected",
        [
            ("(0,0)", (0, 0)),
            ("(10,15)", (10, 15)),
            ("(999,999)", (999, 999)),
        ],
    )
    def test_parse_position_valid(self, pos_str, expected):
        """Position parsing handles various coordinate values."""
        # Test via cheese command which uses position parsing
        cmd = Protocol.parse_command(f"cheese {pos_str}")
        assert cmd is not None
        assert cmd.data["cheese"][0] == expected

    @pytest.mark.parametrize(
        "wall_str",
        [
            "(0,0)-(1,0)",
            "(5,5)-(5,6)",
            "(99,99)-(100,99)",
        ],
    )
    def test_parse_wall_valid(self, wall_str):
        """Wall parsing handles adjacent cell pairs."""
        cmd = Protocol.parse_command(f"walls {wall_str}")
        assert cmd is not None
        assert len(cmd.data["walls"]) == 1
