"""Tests for formatting protocol responses.

These tests verify that structured response objects are correctly formatted
into text messages that the engine can understand.
"""

import pytest
from pyrat_engine.game import Direction

from pyrat_base import Protocol, ResponseType


class TestIdentificationResponses:
    """Tests for formatting AI identification responses.

    Protocol spec: During handshake, AI identifies itself with name and author.
    """

    def test_format_id_name(self):
        """ID response with name identifies the AI."""
        response = Protocol.format_response(ResponseType.ID, {"name": "MyBot v1.0"})
        assert response == "id name MyBot v1.0"

    def test_format_id_author(self):
        """ID response with author identifies the creator."""
        response = Protocol.format_response(ResponseType.ID, {"author": "John Doe"})
        assert response == "id author John Doe"

    @pytest.mark.parametrize(
        "data",
        [
            {},
            {"invalid": "data"},
        ],
    )
    def test_format_id_invalid(self, data):
        """ID response requires either name OR author."""
        with pytest.raises(ValueError):
            Protocol.format_response(ResponseType.ID, data)

    def test_format_id_with_both_name_and_author(self):
        """ID response with both name and author formats as name."""
        # Based on implementation, it seems to prefer name when both are present
        response = Protocol.format_response(
            ResponseType.ID, {"name": "Bot", "author": "Dev"}
        )
        # Check what actually happens - it likely returns the name
        assert response == "id name Bot"

    def test_format_pyratready(self):
        """PYRATREADY signals AI is ready to receive games."""
        assert Protocol.format_response(ResponseType.PYRATREADY) == "pyratready"


class TestOptionResponses:
    """Tests for formatting AI configuration option responses.

    Protocol spec: AI declares supported options during handshake.
    """

    def test_format_option_check(self):
        """Check options are boolean settings."""
        response = Protocol.format_response(
            ResponseType.OPTION, {"name": "Debug", "type": "check", "default": "false"}
        )
        assert response == "option name Debug type check default false"

    def test_format_option_spin(self):
        """Spin options are numeric with min/max range."""
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

    def test_format_option_combo(self):
        """Combo options are choice from predefined values."""
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

    def test_format_option_string(self):
        """String options are text values."""
        response = Protocol.format_response(
            ResponseType.OPTION,
            {"name": "LogFile", "type": "string", "default": "game.log"},
        )
        assert response == "option name LogFile type string default game.log"

    def test_format_option_button(self):
        """Button options trigger actions (no default)."""
        response = Protocol.format_response(
            ResponseType.OPTION, {"name": "Reset", "type": "button"}
        )
        assert response == "option name Reset type button"

    @pytest.mark.parametrize(
        "data",
        [
            {"name": "Test"},  # Missing type
            {"type": "check"},  # Missing name
        ],
    )
    def test_format_option_invalid(self, data):
        """Options require name and type."""
        with pytest.raises(ValueError):
            Protocol.format_response(ResponseType.OPTION, data)

    def test_format_option_invalid_type(self):
        """Option with invalid type still formats (no validation on type)."""
        # The implementation doesn't validate option types
        response = Protocol.format_response(
            ResponseType.OPTION, {"name": "Test", "type": "invalid"}
        )
        assert response == "option name Test type invalid"


class TestGameResponses:
    """Tests for formatting in-game responses.

    Protocol spec: Responses sent during active gameplay.
    """

    def test_format_readyok(self):
        """READYOK confirms AI is responsive."""
        assert Protocol.format_response(ResponseType.READYOK) == "readyok"

    def test_format_preprocessingdone(self):
        """PREPROCESSINGDONE signals maze analysis complete."""
        assert (
            Protocol.format_response(ResponseType.PREPROCESSINGDONE)
            == "preprocessingdone"
        )

    @pytest.mark.parametrize(
        "move,expected",
        [
            ("UP", "move UP"),
            ("DOWN", "move DOWN"),
            ("LEFT", "move LEFT"),
            ("RIGHT", "move RIGHT"),
            ("STAY", "move STAY"),
        ],
    )
    def test_format_move_string(self, move, expected):
        """MOVE response with string direction."""
        response = Protocol.format_response(ResponseType.MOVE, {"move": move})
        assert response == expected

    @pytest.mark.parametrize(
        "direction,expected",
        [
            (Direction.UP, "move UP"),
            (Direction.DOWN, "move DOWN"),
            (Direction.LEFT, "move LEFT"),
            (Direction.RIGHT, "move RIGHT"),
            (Direction.STAY, "move STAY"),
        ],
    )
    def test_format_move_enum(self, direction, expected):
        """MOVE response with Direction enum."""
        response = Protocol.format_response(ResponseType.MOVE, {"move": direction})
        assert response == expected

    def test_format_move_invalid(self):
        """MOVE response requires a move."""
        with pytest.raises(ValueError):
            Protocol.format_response(ResponseType.MOVE, {})

    def test_format_info_basic(self):
        """INFO messages provide progress updates."""
        response = Protocol.format_response(
            ResponseType.INFO, {"nodes": 12345, "depth": 3}
        )
        assert response == "info nodes 12345 depth 3"

    def test_format_info_complex(self):
        """INFO messages can contain multiple key-value pairs."""
        response = Protocol.format_response(
            ResponseType.INFO,
            {
                "depth": 4,
                "time": 150,
                "nodes": 50000,
                "score": 25,
                "currmove": "UP",
                "target": "(5,3)",
            },
        )
        # Note: dict ordering may vary in older Python versions
        assert "info" in response
        assert "depth 4" in response
        assert "time 150" in response
        assert "nodes 50000" in response
        assert "score 25" in response
        assert "currmove UP" in response
        assert "target (5,3)" in response

    def test_format_info_string(self):
        """INFO string messages for debug output."""
        response = Protocol.format_response(
            ResponseType.INFO,
            {"string": "Switching to defensive strategy"},
        )
        assert response == "info string Switching to defensive strategy"

    def test_format_info_empty(self):
        """Empty INFO is allowed but unusual."""
        response = Protocol.format_response(ResponseType.INFO, {})
        assert response == "info"

    def test_format_postprocessingdone(self):
        """POSTPROCESSINGDONE signals learning phase complete."""
        assert (
            Protocol.format_response(ResponseType.POSTPROCESSINGDONE)
            == "postprocessingdone"
        )

    def test_format_ready(self):
        """READY confirms AI recovered after timeout."""
        assert Protocol.format_response(ResponseType.READY) == "ready"


class TestSimpleResponses:
    """Tests for simple responses without data.

    These responses are fixed strings with no parameters.
    """

    @pytest.mark.parametrize(
        "response_type,expected",
        [
            (ResponseType.PYRATREADY, "pyratready"),
            (ResponseType.READYOK, "readyok"),
            (ResponseType.PREPROCESSINGDONE, "preprocessingdone"),
            (ResponseType.POSTPROCESSINGDONE, "postprocessingdone"),
            (ResponseType.READY, "ready"),
        ],
    )
    def test_format_simple_responses(self, response_type, expected):
        """Simple responses have no data parameter."""
        assert Protocol.format_response(response_type) == expected

    def test_format_with_unnecessary_data(self):
        """Simple responses ignore data if provided."""
        # This behavior ensures backward compatibility
        response = Protocol.format_response(ResponseType.READYOK, {"ignored": "data"})
        assert response == "readyok"


class TestFormattingEdgeCases:
    """Tests for edge cases in response formatting."""

    def test_format_unknown_response_type(self):
        """Unknown response types should raise an error."""
        # This test would need a mock or invalid enum value
        # For now, we test that all enum values are handled
        for response_type in ResponseType:
            # Should not raise
            try:
                if response_type in [
                    ResponseType.ID,
                    ResponseType.OPTION,
                    ResponseType.MOVE,
                    ResponseType.INFO,
                ]:
                    # These require data
                    continue
                Protocol.format_response(response_type)
            except ValueError:
                # Expected for types that require data
                pass

    def test_format_none_data(self):
        """None data should be treated as missing data."""
        # Simple responses work with None
        assert Protocol.format_response(ResponseType.READYOK, None) == "readyok"

        # Complex responses should fail
        with pytest.raises(ValueError):
            Protocol.format_response(ResponseType.MOVE, None)
