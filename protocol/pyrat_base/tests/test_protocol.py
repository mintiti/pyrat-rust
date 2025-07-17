"""Unit tests for the protocol module."""

import pytest
from pyrat_engine.game import Direction

from pyrat_base import (
    CommandType,
    GameResult,
    Player,
    Protocol,
    ResponseType,
)


class TestCommandParsing:
    """Test parsing of protocol commands."""

    def test_parse_empty_line(self):
        """Test parsing empty or whitespace-only lines."""
        assert Protocol.parse_command("") is None
        assert Protocol.parse_command("   ") is None
        assert Protocol.parse_command("\n") is None
        assert Protocol.parse_command("\t") is None

    def test_parse_unknown_command(self):
        """Test parsing unknown commands returns None."""
        assert Protocol.parse_command("invalid") is None
        assert Protocol.parse_command("notacommand") is None
        assert Protocol.parse_command("123") is None

    def test_parse_pyrat(self):
        """Test parsing pyrat handshake command."""
        cmd = Protocol.parse_command("pyrat")
        assert cmd is not None
        assert cmd.type == CommandType.PYRAT
        assert cmd.data == {}

        # Test case insensitive
        cmd = Protocol.parse_command("PYRAT")
        assert cmd is not None
        assert cmd.type == CommandType.PYRAT

    def test_parse_isready(self):
        """Test parsing isready command."""
        cmd = Protocol.parse_command("isready")
        assert cmd is not None
        assert cmd.type == CommandType.ISREADY
        assert cmd.data == {}

    def test_parse_setoption(self):
        """Test parsing setoption command."""
        # Simple option
        cmd = Protocol.parse_command("setoption name SearchDepth value 5")
        assert cmd is not None
        assert cmd.type == CommandType.SETOPTION
        assert cmd.data == {"name": "SearchDepth", "value": "5"}

        # Option with spaces in name
        cmd = Protocol.parse_command("setoption name Search Depth value 5")
        assert cmd is not None
        assert cmd.data == {"name": "Search Depth", "value": "5"}

        # Option with spaces in value
        cmd = Protocol.parse_command("setoption name Strategy value Aggressive Play")
        assert cmd is not None
        assert cmd.data == {"name": "Strategy", "value": "Aggressive Play"}

        # Invalid formats
        assert Protocol.parse_command("setoption") is None
        assert Protocol.parse_command("setoption name") is None
        assert Protocol.parse_command("setoption name Test") is None
        assert Protocol.parse_command("setoption name Test val 5") is None

    def test_parse_debug(self):
        """Test parsing debug command."""
        cmd = Protocol.parse_command("debug on")
        assert cmd is not None
        assert cmd.type == CommandType.DEBUG
        assert cmd.data == {"enabled": True}

        cmd = Protocol.parse_command("debug off")
        assert cmd is not None
        assert cmd.data == {"enabled": False}

        # Invalid formats
        assert Protocol.parse_command("debug") is None
        assert Protocol.parse_command("debug yes") is None
        assert Protocol.parse_command("debug on off") is None

    def test_parse_newgame(self):
        """Test parsing newgame command."""
        cmd = Protocol.parse_command("newgame")
        assert cmd is not None
        assert cmd.type == CommandType.NEWGAME
        assert cmd.data == {}

    def test_parse_maze(self):
        """Test parsing maze command."""
        cmd = Protocol.parse_command("maze height:10 width:15")
        assert cmd is not None
        assert cmd.type == CommandType.MAZE
        assert cmd.data == {"height": 10, "width": 15}

        # Different order
        cmd = Protocol.parse_command("maze width:20 height:5")
        assert cmd is not None
        assert cmd.data == {"height": 5, "width": 20}

        # Invalid formats
        assert Protocol.parse_command("maze") is None
        assert Protocol.parse_command("maze height:10") is None
        assert Protocol.parse_command("maze width:15") is None
        assert Protocol.parse_command("maze height:abc width:10") is None
        assert Protocol.parse_command("maze h:10 w:15") is None

    def test_parse_walls(self):
        """Test parsing walls command."""
        # Single wall
        cmd = Protocol.parse_command("walls (0,0)-(0,1)")
        assert cmd is not None
        assert cmd.type == CommandType.WALLS
        assert cmd.data == {"walls": [((0, 0), (0, 1))]}

        # Multiple walls
        cmd = Protocol.parse_command("walls (0,0)-(0,1) (1,1)-(2,1) (3,3)-(3,4)")
        assert cmd is not None
        assert cmd.data == {
            "walls": [((0, 0), (0, 1)), ((1, 1), (2, 1)), ((3, 3), (3, 4))]
        }

        # Empty walls list
        cmd = Protocol.parse_command("walls")
        assert cmd is not None
        assert cmd.data == {"walls": []}

        # Invalid formats
        assert Protocol.parse_command("walls (0,0)") is None
        assert Protocol.parse_command("walls (0,0)-(0,1") is None
        assert Protocol.parse_command("walls (0,0)-(a,b)") is None
        assert Protocol.parse_command("walls (0,0,0)-(1,1)") is None

    def test_parse_mud(self):
        """Test parsing mud command."""
        # Single mud
        cmd = Protocol.parse_command("mud (5,5)-(5,6):3")
        assert cmd is not None
        assert cmd.type == CommandType.MUD
        assert cmd.data == {"mud": [((5, 5), (5, 6), 3)]}

        # Multiple mud
        cmd = Protocol.parse_command("mud (1,1)-(1,2):2 (3,3)-(4,3):5")
        assert cmd is not None
        assert cmd.data == {"mud": [((1, 1), (1, 2), 2), ((3, 3), (4, 3), 5)]}

        # Empty mud list
        cmd = Protocol.parse_command("mud")
        assert cmd is not None
        assert cmd.data == {"mud": []}

        # Invalid formats
        assert Protocol.parse_command("mud (5,5)-(5,6)") is None
        assert Protocol.parse_command("mud (5,5)-(5,6):") is None
        assert Protocol.parse_command("mud (5,5)-(5,6):abc") is None

    def test_parse_cheese(self):
        """Test parsing cheese command."""
        # Single cheese
        cmd = Protocol.parse_command("cheese (2,3)")
        assert cmd is not None
        assert cmd.type == CommandType.CHEESE
        assert cmd.data == {"cheese": [(2, 3)]}

        # Multiple cheese
        cmd = Protocol.parse_command("cheese (2,2) (7,8) (4,5)")
        assert cmd is not None
        assert cmd.data == {"cheese": [(2, 2), (7, 8), (4, 5)]}

        # Empty cheese list
        cmd = Protocol.parse_command("cheese")
        assert cmd is not None
        assert cmd.data == {"cheese": []}

        # Invalid formats
        assert Protocol.parse_command("cheese 2,3") is None
        assert Protocol.parse_command("cheese (2,3,4)") is None
        assert Protocol.parse_command("cheese (a,b)") is None

    def test_parse_player1(self):
        """Test parsing player1 command."""
        cmd = Protocol.parse_command("player1 rat (9,9)")
        assert cmd is not None
        assert cmd.type == CommandType.PLAYER1
        assert cmd.data == {"position": (9, 9)}

        # Invalid formats
        assert Protocol.parse_command("player1 python (9,9)") is None
        assert Protocol.parse_command("player1 rat") is None
        assert Protocol.parse_command("player1 (9,9)") is None
        assert Protocol.parse_command("player1 rat 9,9") is None

    def test_parse_player2(self):
        """Test parsing player2 command."""
        cmd = Protocol.parse_command("player2 python (0,0)")
        assert cmd is not None
        assert cmd.type == CommandType.PLAYER2
        assert cmd.data == {"position": (0, 0)}

        # Invalid formats
        assert Protocol.parse_command("player2 rat (0,0)") is None
        assert Protocol.parse_command("player2 python") is None
        assert Protocol.parse_command("player2 (0,0)") is None

    def test_parse_youare(self):
        """Test parsing youare command."""
        cmd = Protocol.parse_command("youare rat")
        assert cmd is not None
        assert cmd.type == CommandType.YOUARE
        assert cmd.data == {"player": Player.RAT}

        cmd = Protocol.parse_command("youare python")
        assert cmd is not None
        assert cmd.data == {"player": Player.PYTHON}

        # Case insensitive
        cmd = Protocol.parse_command("youare PYTHON")
        assert cmd is not None
        assert cmd.data == {"player": Player.PYTHON}

        # Invalid formats
        assert Protocol.parse_command("youare") is None
        assert Protocol.parse_command("youare snake") is None
        assert Protocol.parse_command("youare rat python") is None

    def test_parse_timecontrol(self):
        """Test parsing timecontrol command."""
        # Full timecontrol
        cmd = Protocol.parse_command(
            "timecontrol move:100 preprocessing:3000 postprocessing:1000"
        )
        assert cmd is not None
        assert cmd.type == CommandType.TIMECONTROL
        assert cmd.data == {"move": 100, "preprocessing": 3000, "postprocessing": 1000}

        # Partial timecontrol
        cmd = Protocol.parse_command("timecontrol move:50")
        assert cmd is not None
        assert cmd.data == {"move": 50}

        # Different order
        cmd = Protocol.parse_command("timecontrol postprocessing:500 move:200")
        assert cmd is not None
        assert cmd.data == {"postprocessing": 500, "move": 200}

        # Invalid formats
        assert Protocol.parse_command("timecontrol") is None
        assert Protocol.parse_command("timecontrol move:abc") is None
        assert Protocol.parse_command("timecontrol invalid:100") is None

    def test_parse_startpreprocessing(self):
        """Test parsing startpreprocessing command."""
        cmd = Protocol.parse_command("startpreprocessing")
        assert cmd is not None
        assert cmd.type == CommandType.STARTPREPROCESSING
        assert cmd.data == {}

    def test_parse_moves(self):
        """Test parsing moves command."""
        cmd = Protocol.parse_command("moves rat:UP python:DOWN")
        assert cmd is not None
        assert cmd.type == CommandType.MOVES
        assert cmd.data == {"moves": {Player.RAT: "UP", Player.PYTHON: "DOWN"}}

        # Different moves
        cmd = Protocol.parse_command("moves rat:STAY python:LEFT")
        assert cmd is not None
        assert cmd.data == {"moves": {Player.RAT: "STAY", Player.PYTHON: "LEFT"}}

        # Case insensitive moves
        cmd = Protocol.parse_command("moves rat:up python:right")
        assert cmd is not None
        assert cmd.data == {"moves": {Player.RAT: "UP", Player.PYTHON: "RIGHT"}}

        # Invalid formats
        assert Protocol.parse_command("moves") is None
        assert Protocol.parse_command("moves rat:UP") is None
        assert Protocol.parse_command("moves python:DOWN") is None
        assert Protocol.parse_command("moves rat:INVALID python:DOWN") is None
        assert Protocol.parse_command("moves rat:UP snake:DOWN") is None

    def test_parse_go(self):
        """Test parsing go command."""
        cmd = Protocol.parse_command("go")
        assert cmd is not None
        assert cmd.type == CommandType.GO
        assert cmd.data == {}

    def test_parse_stop(self):
        """Test parsing stop command."""
        cmd = Protocol.parse_command("stop")
        assert cmd is not None
        assert cmd.type == CommandType.STOP
        assert cmd.data == {}

    def test_parse_timeout(self):
        """Test parsing timeout command."""
        # Timeout with move
        cmd = Protocol.parse_command("timeout move:STAY")
        assert cmd is not None
        assert cmd.type == CommandType.TIMEOUT
        assert cmd.data == {"move": "STAY"}

        # Timeout preprocessing
        cmd = Protocol.parse_command("timeout preprocessing")
        assert cmd is not None
        assert cmd.data == {"phase": "preprocessing"}

        # Timeout postprocessing
        cmd = Protocol.parse_command("timeout postprocessing")
        assert cmd is not None
        assert cmd.data == {"phase": "postprocessing"}

        # Invalid formats
        assert Protocol.parse_command("timeout") is None
        assert Protocol.parse_command("timeout move:INVALID") is None
        assert Protocol.parse_command("timeout invalid") is None

    def test_parse_ready(self):
        """Test parsing ready? command."""
        cmd = Protocol.parse_command("ready?")
        assert cmd is not None
        assert cmd.type == CommandType.READY
        assert cmd.data == {}

    def test_parse_gameover(self):
        """Test parsing gameover command."""
        cmd = Protocol.parse_command("gameover winner:rat score:3-2")
        assert cmd is not None
        assert cmd.type == CommandType.GAMEOVER
        assert cmd.data == {"winner": GameResult.RAT, "score": (3.0, 2.0)}

        # Draw game
        cmd = Protocol.parse_command("gameover winner:draw score:2.5-2.5")
        assert cmd is not None
        assert cmd.data == {"winner": GameResult.DRAW, "score": (2.5, 2.5)}

        # Invalid formats
        assert Protocol.parse_command("gameover") is None
        assert Protocol.parse_command("gameover winner:rat") is None
        assert Protocol.parse_command("gameover score:3-2") is None
        assert Protocol.parse_command("gameover winner:invalid score:3-2") is None
        assert Protocol.parse_command("gameover winner:rat score:3") is None

    def test_parse_startpostprocessing(self):
        """Test parsing startpostprocessing command."""
        cmd = Protocol.parse_command("startpostprocessing")
        assert cmd is not None
        assert cmd.type == CommandType.STARTPOSTPROCESSING
        assert cmd.data == {}

    def test_parse_recover(self):
        """Test parsing recover command."""
        cmd = Protocol.parse_command("recover")
        assert cmd is not None
        assert cmd.type == CommandType.RECOVER
        assert cmd.data == {}

    def test_parse_moves_history(self):
        """Test parsing moves_history command."""
        # Single move
        cmd = Protocol.parse_command("moves_history UP")
        assert cmd is not None
        assert cmd.type == CommandType.MOVES_HISTORY
        assert cmd.data == {"history": ["UP"]}

        # Multiple moves
        cmd = Protocol.parse_command("moves_history UP DOWN LEFT RIGHT STAY")
        assert cmd is not None
        assert cmd.data == {"history": ["UP", "DOWN", "LEFT", "RIGHT", "STAY"]}

        # Empty history
        cmd = Protocol.parse_command("moves_history")
        assert cmd is not None
        assert cmd.data == {"history": []}

        # Invalid moves
        assert Protocol.parse_command("moves_history UP INVALID DOWN") is None

    def test_parse_current_position(self):
        """Test parsing current_position command."""
        cmd = Protocol.parse_command("current_position rat:(5,3) python:(2,7)")
        assert cmd is not None
        assert cmd.type == CommandType.CURRENT_POSITION
        assert cmd.data == {"positions": {Player.RAT: (5, 3), Player.PYTHON: (2, 7)}}

        # Invalid formats
        assert Protocol.parse_command("current_position") is None
        assert Protocol.parse_command("current_position rat:(5,3)") is None
        assert Protocol.parse_command("current_position rat:5,3 python:(2,7)") is None

    def test_parse_score(self):
        """Test parsing score command."""
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

    def test_parse_extra_whitespace(self):
        """Test parsing commands with extra whitespace."""
        cmd = Protocol.parse_command("  pyrat  ")
        assert cmd is not None
        assert cmd.type == CommandType.PYRAT

        cmd = Protocol.parse_command("maze   height:10   width:15")
        assert cmd is not None
        assert cmd.data == {"height": 10, "width": 15}

        cmd = Protocol.parse_command("   walls   (0,0)-(0,1)   (1,1)-(2,1)   ")
        assert cmd is not None
        expected_wall_count = 2
        assert len(cmd.data["walls"]) == expected_wall_count

    def test_parse_numeric_conversion_errors(self):
        """Test commands that fail numeric conversion."""
        # Non-numeric maze dimensions
        assert Protocol.parse_command("maze height:ten width:15") is None
        assert Protocol.parse_command("maze height:10 width:abc") is None

        # Non-numeric timecontrol values
        assert Protocol.parse_command("timecontrol move:fast") is None

        # Non-numeric score values
        assert Protocol.parse_command("score rat:high python:2") is None

        # Non-numeric gameover score
        assert Protocol.parse_command("gameover winner:rat score:abc-def") is None
        assert Protocol.parse_command("gameover winner:rat score:3-abc") is None


class TestResponseFormatting:
    """Test formatting of protocol responses."""

    def test_format_id_responses(self):
        """Test formatting ID responses."""
        # Name ID
        response = Protocol.format_response(ResponseType.ID, {"name": "MyBot v1.0"})
        assert response == "id name MyBot v1.0"

        # Author ID
        response = Protocol.format_response(ResponseType.ID, {"author": "John Doe"})
        assert response == "id author John Doe"

        # Missing data
        with pytest.raises(ValueError):
            Protocol.format_response(ResponseType.ID, {})

        with pytest.raises(ValueError):
            Protocol.format_response(ResponseType.ID, {"invalid": "data"})

    def test_format_option_responses(self):
        """Test formatting option responses."""
        # Check option
        response = Protocol.format_response(
            ResponseType.OPTION, {"name": "Debug", "type": "check", "default": "false"}
        )
        assert response == "option name Debug type check default false"

        # Spin option
        response = Protocol.format_response(
            ResponseType.OPTION,
            {
                "name": "SearchDepth",
                "type": "spin",
                "default": "3",
                "min": "1",
                "max": "10",
            },
        )
        assert response == "option name SearchDepth type spin default 3 min 1 max 10"

        # Combo option
        response = Protocol.format_response(
            ResponseType.OPTION,
            {
                "name": "Strategy",
                "type": "combo",
                "default": "Balanced",
                "values": ["Aggressive", "Balanced", "Defensive"],
            },
        )
        assert (
            response
            == "option name Strategy type combo default Balanced var Aggressive var Balanced var Defensive"
        )

        # String option
        response = Protocol.format_response(
            ResponseType.OPTION,
            {"name": "LogFile", "type": "string", "default": "game.log"},
        )
        assert response == "option name LogFile type string default game.log"

        # Minimal option (no default)
        response = Protocol.format_response(
            ResponseType.OPTION, {"name": "Reset", "type": "button"}
        )
        assert response == "option name Reset type button"

        # Missing required fields
        with pytest.raises(ValueError):
            Protocol.format_response(ResponseType.OPTION, {"name": "Test"})

        with pytest.raises(ValueError):
            Protocol.format_response(ResponseType.OPTION, {"type": "check"})

    def test_format_simple_responses(self):
        """Test formatting simple responses without data."""
        assert Protocol.format_response(ResponseType.PYRATREADY) == "pyratready"
        assert Protocol.format_response(ResponseType.READYOK) == "readyok"
        assert (
            Protocol.format_response(ResponseType.PREPROCESSINGDONE)
            == "preprocessingdone"
        )
        assert (
            Protocol.format_response(ResponseType.POSTPROCESSINGDONE)
            == "postprocessingdone"
        )
        assert Protocol.format_response(ResponseType.READY) == "ready"

    def test_format_move_response(self):
        """Test formatting move responses."""
        # String move
        response = Protocol.format_response(ResponseType.MOVE, {"move": "UP"})
        assert response == "move UP"

        # Direction enum move
        response = Protocol.format_response(ResponseType.MOVE, {"move": Direction.DOWN})
        assert response == "move DOWN"

        # Missing move
        with pytest.raises(ValueError):
            Protocol.format_response(ResponseType.MOVE, {})

    def test_format_info_responses(self):
        """Test formatting info responses."""
        # Simple info
        response = Protocol.format_response(
            ResponseType.INFO, {"nodes": 12345, "depth": 3}
        )
        assert response == "info nodes 12345 depth 3"

        # Info with currmove
        response = Protocol.format_response(
            ResponseType.INFO, {"depth": 3, "currmove": "UP"}
        )
        assert response == "info depth 3 currmove UP"

        # Info with pv (principal variation)
        response = Protocol.format_response(
            ResponseType.INFO, {"score": 25, "pv": ["UP", "RIGHT", "RIGHT"]}
        )
        assert response == "info score 25 pv UP RIGHT RIGHT"

        # Info with target position
        response = Protocol.format_response(
            ResponseType.INFO, {"nodes": 5000, "target": (5, 3)}
        )
        assert response == "info nodes 5000 target (5,3)"

        # Info with string message
        response = Protocol.format_response(
            ResponseType.INFO, {"depth": 4, "string": "Switching to defensive strategy"}
        )
        assert response == "info depth 4 string Switching to defensive strategy"

        # Complex info
        response = Protocol.format_response(
            ResponseType.INFO,
            {
                "nodes": 50000,
                "depth": 4,
                "time": 150,
                "score": 30,
                "pv": ["UP", "UP", "LEFT"],
                "string": "Found winning line",
            },
        )
        # Note: string should be at the end
        assert (
            response
            == "info nodes 50000 depth 4 time 150 score 30 pv UP UP LEFT string Found winning line"
        )

        # Empty info
        response = Protocol.format_response(ResponseType.INFO, {})
        assert response == "info"

    def test_format_unknown_response(self):
        """Test formatting unknown response type raises error."""
        # Create a mock response type that's not in the format_response method
        with pytest.raises(ValueError, match="Unknown response type"):
            Protocol.format_response(None, {})  # type: ignore


class TestEdgeCases:
    """Test edge cases and error handling."""

    def test_parse_position_edge_cases(self):
        """Test position parsing edge cases."""
        from pyrat_base.protocol import _parse_position

        # Valid positions
        assert _parse_position("(0,0)") == (0, 0)
        assert _parse_position("(123,456)") == (123, 456)
        assert _parse_position("( 1 , 2 )") == (1, 2)  # With spaces

        # Invalid positions
        assert _parse_position("0,0") is None  # No parentheses
        assert _parse_position("(0,0") is None  # Missing closing
        assert _parse_position("0,0)") is None  # Missing opening
        assert _parse_position("(0)") is None  # Only one coordinate
        assert _parse_position("(0,0,0)") is None  # Too many coordinates
        assert _parse_position("(a,b)") is None  # Non-numeric
        assert _parse_position("") is None

    def test_parse_wall_edge_cases(self):
        """Test wall parsing edge cases."""
        from pyrat_base.protocol import _parse_wall

        # Valid walls
        assert _parse_wall("(0,0)-(1,1)") == ((0, 0), (1, 1))
        assert _parse_wall("(10,20)-(30,40)") == ((10, 20), (30, 40))

        # Invalid walls
        assert _parse_wall("(0,0)(1,1)") is None  # No dash
        assert _parse_wall("(0,0)-") is None  # Missing second position
        assert _parse_wall("-(1,1)") is None  # Missing first position
        assert _parse_wall("(0,0)-(1,1)-(2,2)") is None  # Too many positions

    def test_parse_mud_edge_cases(self):
        """Test mud parsing edge cases."""
        from pyrat_base.protocol import _parse_mud

        # Valid mud
        assert _parse_mud("(0,0)-(1,1):3") == ((0, 0), (1, 1), 3)
        assert _parse_mud("(5,5)-(5,6):10") == ((5, 5), (5, 6), 10)

        # Invalid mud
        assert _parse_mud("(0,0)-(1,1)") is None  # No cost
        assert _parse_mud("(0,0)-(1,1):") is None  # Empty cost
        assert _parse_mud("(0,0)-(1,1):abc") is None  # Non-numeric cost
        assert _parse_mud("(0,0)-(1,1):3:4") is None  # Multiple colons

    def test_parse_move_edge_cases(self):
        """Test move parsing edge cases."""
        from pyrat_base.protocol import _parse_move

        # Valid moves
        assert _parse_move("UP") == "UP"
        assert _parse_move("up") == "UP"  # Case insensitive
        assert _parse_move("Down") == "DOWN"
        assert _parse_move("STAY") == "STAY"

        # Invalid moves
        assert _parse_move("INVALID") is None
        assert _parse_move("") is None
        assert _parse_move("UPDOWN") is None

    def test_none_data_handling(self):
        """Test that None data is handled properly in format_response."""
        # These should work with None data
        assert Protocol.format_response(ResponseType.PYRATREADY, None) == "pyratready"
        assert Protocol.format_response(ResponseType.READYOK, None) == "readyok"

        # These should work with empty dict (same as None)
        assert Protocol.format_response(ResponseType.INFO, None) == "info"
        assert Protocol.format_response(ResponseType.INFO, {}) == "info"


class TestDefensiveCodeCoverage:
    """Tests specifically to reach defensive code paths for 100% coverage.

    These tests cover edge cases that are logically difficult to reach but
    represent good defensive programming practices. They ensure robustness
    against malformed input that might occur due to future changes or
    unexpected protocol violations.
    """

    def test_empty_split_result(self):
        """Test the unreachable empty parts check after split.

        This covers line 68 which checks if parts is empty after split.
        In practice, this is unreachable because strip() + split() on a
        non-empty string always produces at least one element.
        """
        # We can't actually reach this case naturally, but if we could,
        # it would be something like a string that strips to empty
        # but somehow passes the earlier empty check

        # Instead test with only whitespace variations
        assert Protocol.parse_command("\t \n") is None
        assert Protocol.parse_command("   \r\n   ") is None

        # Let's also test with Unicode zero-width spaces that might behave oddly
        assert Protocol.parse_command("\u200b") is None  # Zero-width space
        assert Protocol.parse_command("\ufeff") is None  # Zero-width no-break space

    def test_maze_missing_colon_in_dimension(self):
        """Test maze command with missing colon in dimension spec.

        Covers line 109: if ":" not in part
        """
        assert Protocol.parse_command("maze height10 width:15") is None
        assert Protocol.parse_command("maze height: width:15") is None
        assert Protocol.parse_command("maze height width:15") is None

    def test_maze_missing_required_dimension(self):
        """Test maze command that somehow misses a required dimension.

        Covers line 115: if "height" not in maze_data or "width" not in maze_data
        This happens when a key is not height/width.
        """
        assert Protocol.parse_command("maze height:10 depth:15") is None
        assert Protocol.parse_command("maze size:10 width:15") is None
        # Also test duplicate keys which would overwrite
        assert Protocol.parse_command("maze width:10 width:15") is None  # No height
        assert Protocol.parse_command("maze height:10 height:15") is None  # No width

    def test_player2_invalid_position_format(self):
        """Test player2 with position that fails parsing.

        Covers line 163: position parsing failure for player2
        """
        assert (
            Protocol.parse_command("player2 python 0,0") is None
        )  # Missing parentheses
        assert (
            Protocol.parse_command("player2 python (0,0") is None
        )  # Missing closing paren
        assert Protocol.parse_command("player2 python (a,b)") is None  # Non-numeric

    def test_timecontrol_missing_colon(self):
        """Test timecontrol with missing colon in time spec.

        Covers line 182: if ":" not in part
        """
        assert Protocol.parse_command("timecontrol move100") is None
        assert Protocol.parse_command("timecontrol move:100 preprocessing3000") is None

    def test_moves_missing_colon(self):
        """Test moves command with missing colon in move spec.

        Covers line 199: if ":" not in part
        """
        assert Protocol.parse_command("moves ratUP python:DOWN") is None
        assert Protocol.parse_command("moves rat:UP pythonDOWN") is None

    def test_moves_missing_player(self):
        """Test moves command that somehow misses a required player.

        Covers line 209: if Player.RAT not in moves or Player.PYTHON not in moves
        This could happen if the same player is specified twice.
        """
        # This is tricky - we need valid player:move pairs but duplicate players
        # Since we iterate through parts and assign to dict, duplicates overwrite
        # So we need to construct a case where we have two parts but one is invalid
        assert Protocol.parse_command("moves cat:UP python:DOWN") is None
        assert Protocol.parse_command("moves rat:UP dog:DOWN") is None
        # Test duplicate players (second overwrites first, leaving one missing)
        assert Protocol.parse_command("moves rat:UP rat:DOWN") is None  # No python
        assert Protocol.parse_command("moves python:UP python:DOWN") is None  # No rat

    def test_gameover_missing_colon(self):
        """Test gameover with missing colon in spec.

        Covers line 244: if ":" not in part
        """
        assert Protocol.parse_command("gameover winnerrat score:3-2") is None
        assert Protocol.parse_command("gameover winner:rat score3-2") is None

    def test_gameover_score_multiple_dashes(self):
        """Test gameover with score containing multiple dashes.

        Covers line 256: if len(score_parts) != 2
        """
        assert Protocol.parse_command("gameover winner:rat score:3-2-1") is None
        assert Protocol.parse_command("gameover winner:rat score:3-2-1-0") is None

    def test_gameover_missing_required_field(self):
        """Test gameover that somehow misses a required field.

        Covers line 262: if "winner" not in gameover_data or "score" not in gameover_data
        This happens when an unrecognized key is provided.
        """
        assert (
            Protocol.parse_command("gameover winner:rat result:3-2") is None
        )  # 'result' instead of 'score'
        assert (
            Protocol.parse_command("gameover champion:rat score:3-2") is None
        )  # 'champion' instead of 'winner'

    def test_current_position_missing_colon(self):
        """Test current_position with missing colon.

        Covers line 288: if ":" not in part
        """
        assert Protocol.parse_command("current_position rat(5,3) python:(2,7)") is None
        assert Protocol.parse_command("current_position rat:(5,3) python(2,7)") is None

    def test_current_position_invalid_position(self):
        """Test current_position with position that fails parsing.

        Covers line 292: position parsing failure in current_position
        """
        assert Protocol.parse_command("current_position rat:5,3 python:(2,7)") is None
        assert Protocol.parse_command("current_position rat:(5,3) python:2,7") is None

    def test_current_position_missing_player(self):
        """Test current_position that somehow misses a required player.

        Covers line 298: if Player.RAT not in positions or Player.PYTHON not in positions
        """
        assert Protocol.parse_command("current_position cat:(5,3) python:(2,7)") is None
        assert Protocol.parse_command("current_position rat:(5,3) snake:(2,7)") is None
        # Test duplicate players
        assert (
            Protocol.parse_command("current_position rat:(5,3) rat:(2,7)") is None
        )  # No python
        assert (
            Protocol.parse_command("current_position python:(5,3) python:(2,7)") is None
        )  # No rat

    def test_score_missing_colon(self):
        """Test score command with missing colon.

        Covers line 308: if ":" not in part
        """
        assert Protocol.parse_command("score rat3 python:2") is None
        assert Protocol.parse_command("score rat:3 python2") is None

    def test_score_invalid_player(self):
        """Test score with invalid player that fails parsing.

        Covers line 312: player parsing failure in score command
        """
        assert Protocol.parse_command("score cat:3 python:2") is None
        assert Protocol.parse_command("score rat:3 dog:2") is None

    def test_score_missing_player(self):
        """Test score that somehow misses a required player.

        Covers line 315: if Player.RAT not in scores or Player.PYTHON not in scores
        """
        # Similar to moves/current_position - need invalid player to not add to dict
        assert Protocol.parse_command("score mouse:3 python:2") is None
        assert Protocol.parse_command("score rat:3 snake:2") is None
        # Test duplicate players
        assert Protocol.parse_command("score rat:3 rat:2") is None  # No python
        assert Protocol.parse_command("score python:3 python:2") is None  # No rat
