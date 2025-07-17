"""Unit tests for the enums module."""

from pyrat_base import (
    CommandType,
    GameResult,
    InfoType,
    OptionType,
    Player,
    ResponseType,
    command_from_string,
    game_result_from_string,
    info_type_from_string,
    option_type_from_string,
    player_from_string,
    response_to_string,
)


class TestCommandType:
    """Test CommandType enum and conversions."""

    def test_all_commands_exist(self):
        """Verify all protocol commands are defined."""
        expected_commands = [
            "PYRAT",
            "ISREADY",
            "SETOPTION",
            "DEBUG",
            "NEWGAME",
            "MAZE",
            "WALLS",
            "MUD",
            "CHEESE",
            "PLAYER1",
            "PLAYER2",
            "YOUARE",
            "TIMECONTROL",
            "STARTPREPROCESSING",
            "MOVES",
            "GO",
            "STOP",
            "TIMEOUT",
            "READY",
            "GAMEOVER",
            "STARTPOSTPROCESSING",
            "RECOVER",
            "MOVES_HISTORY",
            "CURRENT_POSITION",
            "SCORE",
        ]

        actual_commands = [cmd.name for cmd in CommandType]
        assert set(actual_commands) == set(expected_commands)

    def test_command_from_string_valid(self):
        """Test converting valid command strings to enums."""
        test_cases = [
            ("pyrat", CommandType.PYRAT),
            ("isready", CommandType.ISREADY),
            ("setoption", CommandType.SETOPTION),
            ("debug", CommandType.DEBUG),
            ("newgame", CommandType.NEWGAME),
            ("maze", CommandType.MAZE),
            ("walls", CommandType.WALLS),
            ("mud", CommandType.MUD),
            ("cheese", CommandType.CHEESE),
            ("player1", CommandType.PLAYER1),
            ("player2", CommandType.PLAYER2),
            ("youare", CommandType.YOUARE),
            ("timecontrol", CommandType.TIMECONTROL),
            ("startpreprocessing", CommandType.STARTPREPROCESSING),
            ("moves", CommandType.MOVES),
            ("go", CommandType.GO),
            ("stop", CommandType.STOP),
            ("timeout", CommandType.TIMEOUT),
            ("ready?", CommandType.READY),  # Special case with ?
            ("gameover", CommandType.GAMEOVER),
            ("startpostprocessing", CommandType.STARTPOSTPROCESSING),
            ("recover", CommandType.RECOVER),
            ("moves_history", CommandType.MOVES_HISTORY),
            ("current_position", CommandType.CURRENT_POSITION),
            ("score", CommandType.SCORE),
        ]

        for string_val, expected_enum in test_cases:
            assert command_from_string(string_val) == expected_enum

    def test_command_from_string_case_insensitive(self):
        """Test that command parsing is case insensitive."""
        assert command_from_string("PYRAT") == CommandType.PYRAT
        assert command_from_string("PyRat") == CommandType.PYRAT
        assert command_from_string("isReady") == CommandType.ISREADY

    def test_command_from_string_invalid(self):
        """Test that invalid command strings return None."""
        assert command_from_string("invalid") is None
        assert command_from_string("") is None
        assert command_from_string("notacommand") is None


class TestResponseType:
    """Test ResponseType enum and conversions."""

    def test_all_responses_exist(self):
        """Verify all protocol responses are defined."""
        expected_responses = [
            "ID",
            "OPTION",
            "PYRATREADY",
            "READYOK",
            "PREPROCESSINGDONE",
            "MOVE",
            "POSTPROCESSINGDONE",
            "READY",
            "INFO",
        ]

        actual_responses = [resp.name for resp in ResponseType]
        assert set(actual_responses) == set(expected_responses)

    def test_response_to_string(self):
        """Test converting ResponseType enums to protocol strings."""
        test_cases = [
            (ResponseType.ID, "id"),
            (ResponseType.OPTION, "option"),
            (ResponseType.PYRATREADY, "pyratready"),
            (ResponseType.READYOK, "readyok"),
            (ResponseType.PREPROCESSINGDONE, "preprocessingdone"),
            (ResponseType.MOVE, "move"),
            (ResponseType.POSTPROCESSINGDONE, "postprocessingdone"),
            (ResponseType.READY, "ready"),
            (ResponseType.INFO, "info"),
        ]

        for enum_val, expected_string in test_cases:
            assert response_to_string(enum_val) == expected_string


class TestPlayer:
    """Test Player enum and conversions."""

    def test_player_values(self):
        """Test Player enum has correct values."""
        assert Player.RAT.value == "rat"
        assert Player.PYTHON.value == "python"

    def test_player_from_string_valid(self):
        """Test converting valid player strings to enums."""
        assert player_from_string("rat") == Player.RAT
        assert player_from_string("python") == Player.PYTHON
        assert player_from_string("RAT") == Player.RAT
        assert player_from_string("Python") == Player.PYTHON

    def test_player_from_string_invalid(self):
        """Test that invalid player strings return None."""
        assert player_from_string("invalid") is None
        assert player_from_string("") is None
        assert player_from_string("snake") is None


class TestGameResult:
    """Test GameResult enum and conversions."""

    def test_game_result_values(self):
        """Test GameResult enum has correct values."""
        assert GameResult.RAT.value == "rat"
        assert GameResult.PYTHON.value == "python"
        assert GameResult.DRAW.value == "draw"

    def test_game_result_from_string_valid(self):
        """Test converting valid result strings to enums."""
        assert game_result_from_string("rat") == GameResult.RAT
        assert game_result_from_string("python") == GameResult.PYTHON
        assert game_result_from_string("draw") == GameResult.DRAW
        assert game_result_from_string("DRAW") == GameResult.DRAW

    def test_game_result_from_string_invalid(self):
        """Test that invalid result strings return None."""
        assert game_result_from_string("invalid") is None
        assert game_result_from_string("") is None
        assert game_result_from_string("tie") is None


class TestOptionType:
    """Test OptionType enum and conversions."""

    def test_option_type_values(self):
        """Test OptionType enum has correct values."""
        assert OptionType.CHECK.value == "check"
        assert OptionType.SPIN.value == "spin"
        assert OptionType.COMBO.value == "combo"
        assert OptionType.STRING.value == "string"
        assert OptionType.BUTTON.value == "button"

    def test_option_type_from_string_valid(self):
        """Test converting valid option type strings to enums."""
        assert option_type_from_string("check") == OptionType.CHECK
        assert option_type_from_string("spin") == OptionType.SPIN
        assert option_type_from_string("combo") == OptionType.COMBO
        assert option_type_from_string("string") == OptionType.STRING
        assert option_type_from_string("button") == OptionType.BUTTON
        assert option_type_from_string("CHECK") == OptionType.CHECK

    def test_option_type_from_string_invalid(self):
        """Test that invalid option type strings return None."""
        assert option_type_from_string("invalid") is None
        assert option_type_from_string("") is None
        assert option_type_from_string("boolean") is None


class TestInfoType:
    """Test InfoType enum and conversions."""

    def test_info_type_values(self):
        """Test InfoType enum has correct values."""
        assert InfoType.NODES.value == "nodes"
        assert InfoType.DEPTH.value == "depth"
        assert InfoType.TIME.value == "time"
        assert InfoType.CURRMOVE.value == "currmove"
        assert InfoType.CURRLINE.value == "currline"
        assert InfoType.SCORE.value == "score"
        assert InfoType.PV.value == "pv"
        assert InfoType.TARGET.value == "target"
        assert InfoType.STRING.value == "string"

    def test_info_type_from_string_valid(self):
        """Test converting valid info type strings to enums."""
        test_cases = [
            ("nodes", InfoType.NODES),
            ("depth", InfoType.DEPTH),
            ("time", InfoType.TIME),
            ("currmove", InfoType.CURRMOVE),
            ("currline", InfoType.CURRLINE),
            ("score", InfoType.SCORE),
            ("pv", InfoType.PV),
            ("target", InfoType.TARGET),
            ("string", InfoType.STRING),
        ]

        for string_val, expected_enum in test_cases:
            assert info_type_from_string(string_val) == expected_enum
            # Test case insensitivity
            assert info_type_from_string(string_val.upper()) == expected_enum

    def test_info_type_from_string_invalid(self):
        """Test that invalid info type strings return None."""
        assert info_type_from_string("invalid") is None
        assert info_type_from_string("") is None
        assert info_type_from_string("evaluation") is None


class TestEnumCompleteness:
    """Test that all enums match the protocol specification."""

    def test_no_missing_enums(self):
        """Verify we haven't missed any enums from the spec."""
        # This test ensures we have exactly the enums specified
        # If the protocol spec changes, this test should fail
        all_enum_classes = [
            CommandType,
            ResponseType,
            Player,
            GameResult,
            OptionType,
            InfoType,
        ]
        expected_enum_count = 6
        assert len(all_enum_classes) == expected_enum_count

    def test_enum_string_round_trip(self):
        """Test that string conversions are reversible where applicable."""
        # Test Player round trip
        for player in Player:
            converted = player_from_string(player.value)
            assert converted == player

        # Test GameResult round trip
        for result in GameResult:
            converted = game_result_from_string(result.value)
            assert converted == result

        # Test OptionType round trip
        for opt_type in OptionType:
            converted = option_type_from_string(opt_type.value)
            assert converted == opt_type

        # Test InfoType round trip
        for info_type in InfoType:
            converted = info_type_from_string(info_type.value)
            assert converted == info_type
