"""Test fixtures and utilities for PyRat protocol tests."""

from .builders import CommandSequenceBuilder, ProtocolExchangeBuilder
from .helpers import (
    MockAI,
    assert_protocol_compliant,
    assert_valid_move_response,
    capture_ai_execution,
    compare_game_states,
    create_game_with_obstacles,
    create_minimal_game_sequence,
    format_protocol_exchange,
    mock_game_state,
    run_protocol_sequence,
)
from .validators import (
    ProtocolValidator,
    ResponseValidator,
    validate_game_state_consistency,
    validate_handshake_response,
    validate_move_format,
)

__all__ = [
    # Builders
    "CommandSequenceBuilder",
    "ProtocolExchangeBuilder",
    # Helpers
    "MockAI",
    "run_protocol_sequence",
    "create_minimal_game_sequence",
    "create_game_with_obstacles",
    "assert_valid_move_response",
    "assert_protocol_compliant",
    "capture_ai_execution",
    "mock_game_state",
    "compare_game_states",
    "format_protocol_exchange",
    # Validators
    "ProtocolValidator",
    "ResponseValidator",
    "validate_move_format",
    "validate_handshake_response",
    "validate_game_state_consistency",
]
