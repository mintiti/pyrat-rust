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
    # Sorted alphabetically
    "CommandSequenceBuilder",
    "MockAI",
    "ProtocolExchangeBuilder",
    "ProtocolValidator",
    "ResponseValidator",
    "assert_protocol_compliant",
    "assert_valid_move_response",
    "capture_ai_execution",
    "compare_game_states",
    "create_game_with_obstacles",
    "create_minimal_game_sequence",
    "format_protocol_exchange",
    "mock_game_state",
    "run_protocol_sequence",
    "validate_game_state_consistency",
    "validate_handshake_response",
    "validate_move_format",
]
