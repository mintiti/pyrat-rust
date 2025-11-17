"""Additional edge case tests for replay system robustness."""

import tempfile

import pytest
from pyrat_engine.core import Direction

from pyrat_base.replay import (
    InitialState,
    Move,
    Replay,
    ReplayMetadata,
    ReplayPlayer,
    ReplayReader,
    ReplayWriter,
    StreamingReplayWriter,
)


class TestReplayEdgeCases:
    """Test edge cases and error conditions."""

    def test_out_of_bounds_positions(self):
        """Test handling of positions outside board bounds."""
        metadata = ReplayMetadata(maze_height=5, maze_width=5)
        initial_state = InitialState(
            width=5,
            height=5,
            cheese=[(2, 2)],
            rat_position=(10, 10),  # Out of bounds!
            python_position=(-1, -1),  # Negative!
        )
        replay = Replay(metadata=metadata, initial_state=initial_state)

        # Should write without error
        writer = ReplayWriter()
        content = writer.format_replay(replay)
        assert "R:(10,10)" in content
        assert "P:(-1,-1)" in content

        # But ReplayPlayer should fail when creating game
        with pytest.raises(ValueError):
            ReplayPlayer(replay)

    def test_duplicate_turn_numbers(self):
        """Test handling of duplicate turn numbers."""
        content = """[Event "Test"]
[Site "?"]
[Date "????.??.??"]
[Round "-"]
[Rat "?"]
[Python "?"]
[Result "*"]
[MazeHeight "5"]
[MazeWidth "5"]
[TimeControl "100+0+0"]

W:
M:
C:(2,2)
R:(0,0)
P:(4,4)

1. U/D
1. L/R
2. S/S
"""
        reader = ReplayReader()
        replay = reader.parse(content)

        # Should parse but have duplicate turns
        expected_moves_count = 3
        assert len(replay.moves) == expected_moves_count
        assert replay.moves[0].turn == 1
        assert replay.moves[1].turn == 1  # Duplicate!

    def test_out_of_order_turns(self):
        """Test handling of out-of-order turn numbers."""
        content = """[Event "Test"]
[Site "?"]
[Date "????.??.??"]
[Round "-"]
[Rat "?"]
[Python "?"]
[Result "*"]
[MazeHeight "5"]
[MazeWidth "5"]
[TimeControl "100+0+0"]

W:
M:
C:(2,2)
R:(0,0)
P:(4,4)

1. U/D
3. L/R
2. S/S
5. U/U
"""
        reader = ReplayReader()
        replay = reader.parse(content)

        # Should parse turns as-is
        assert replay.moves[0].turn == 1
        expected_turn_3 = 3
        expected_turn_2 = 2
        expected_turn_5 = 5
        assert replay.moves[1].turn == expected_turn_3
        assert replay.moves[2].turn == expected_turn_2
        assert replay.moves[3].turn == expected_turn_5

    def test_unicode_in_metadata(self):
        """Test Unicode characters in metadata."""
        metadata = ReplayMetadata(
            event="Tournoi Français 🇫🇷",
            rat="AlphaBot™",
            python="βετα-bot",
            rat_author="José García",
            python_author="李明",
        )
        initial_state = InitialState(width=5, height=5, cheese=[(2, 2)])
        replay = Replay(metadata=metadata, initial_state=initial_state)

        # Write and read back
        with tempfile.NamedTemporaryFile(mode="w", suffix=".pyrat", delete=False) as f:
            writer = ReplayWriter()
            writer.write_file(replay, f.name)

            reader = ReplayReader()
            loaded = reader.read_file(f.name)

        assert loaded.metadata.event == "Tournoi Français 🇫🇷"
        assert loaded.metadata.rat_author == "José García"
        assert loaded.metadata.python_author == "李明"

    def test_very_long_comment(self):
        """Test handling of very long comments."""
        long_comment = "A" * 1000  # 1000 character comment
        moves = [Move(1, Direction.UP, Direction.DOWN, comment=long_comment)]

        metadata = ReplayMetadata(maze_height=5, maze_width=5)
        initial_state = InitialState(width=5, height=5, cheese=[(2, 2)])
        replay = Replay(metadata=metadata, initial_state=initial_state, moves=moves)

        # Should handle long comment
        writer = ReplayWriter()
        content = writer.format_replay(replay)
        assert long_comment in content

        # Should parse back correctly
        reader = ReplayReader()
        loaded = reader.parse(content)
        assert loaded.moves[0].comment == long_comment

    def test_empty_replay_file(self):
        """Test parsing empty file."""
        reader = ReplayReader()
        with pytest.raises(ValueError) as exc_info:
            reader.parse("")
        assert "Empty replay file" in str(exc_info.value)

    def test_truncated_replay(self):
        """Test parsing truncated replay."""
        content = """[Event "Test"]
[Site "?"]
[Date "????.??.??"]
[Round "-"]
[Rat "?"]
[Python "?"]
[Result "*"]
[MazeHeight "5"]
[MazeWidth "5"]
[TimeControl "100+0+0"]

W:
M:
C:(2,2)
R:(0,0)
P:(4,4)

1. U/"""  # Truncated mid-move!

        reader = ReplayReader()
        replay = reader.parse(content)

        # Should parse what it can
        assert replay.metadata.event == "Test"
        assert replay.initial_state.cheese == [(2, 2)]
        assert len(replay.moves) == 0  # Incomplete move not parsed

    def test_invalid_move_notation(self):
        """Test parsing invalid move notations."""
        content = """[Event "Test"]
[Site "?"]
[Date "????.??.??"]
[Round "-"]
[Rat "?"]
[Python "?"]
[Result "*"]
[MazeHeight "5"]
[MazeWidth "5"]
[TimeControl "100+0+0"]

W:
M:
C:(2,2)
R:(0,0)
P:(4,4)

1. X/Y
2. UP/DOWN
3. S/STAY
"""
        reader = ReplayReader()
        replay = reader.parse(content)

        # Invalid moves should default to STAY
        assert len(replay.moves) == 1  # Only first line matches pattern

    def test_missing_required_tags(self):
        """Test parsing with missing required tags."""
        content = """[Event "Test"]
[MazeHeight "5"]

W:
M:
C:(2,2)
R:(0,0)
P:(4,4)

1. S/S
"""
        reader = ReplayReader()
        replay = reader.parse(content)

        # Should use defaults for missing tags
        assert replay.metadata.site == "?"
        assert replay.metadata.rat == "?"
        default_width = 10  # Default!
        assert replay.metadata.maze_width == default_width

    def test_replay_player_with_no_moves(self):
        """Test ReplayPlayer with replay containing no moves."""
        metadata = ReplayMetadata(maze_height=5, maze_width=5)
        initial_state = InitialState(width=5, height=5, cheese=[(2, 2)])
        replay = Replay(metadata=metadata, initial_state=initial_state, moves=[])

        player = ReplayPlayer(replay)
        assert player.current_turn == 0
        assert player.is_finished()
        assert player.step_forward() is None

    def test_streaming_writer_file_errors(self):
        """Test StreamingReplayWriter with file errors."""
        # Try to write to a directory (should fail)
        with pytest.raises(IsADirectoryError):
            with StreamingReplayWriter("/tmp"):
                pass

    def test_very_large_board(self):
        """Test with unusually large board dimensions."""
        metadata = ReplayMetadata(maze_height=1000, maze_width=1000)
        initial_state = InitialState(
            width=1000,
            height=1000,
            cheese=[(500, 500)],
            rat_position=(999, 999),
            python_position=(0, 0),
        )
        replay = Replay(metadata=metadata, initial_state=initial_state)

        # Should handle large dimensions
        writer = ReplayWriter()
        content = writer.format_replay(replay)
        assert '[MazeHeight "1000"]' in content
        assert '[MazeWidth "1000"]' in content

    def test_special_characters_in_moves(self):
        """Test comments with special characters."""
        moves = [
            Move(1, Direction.UP, Direction.DOWN, comment='Quote: "test"'),
            Move(2, Direction.LEFT, Direction.RIGHT, comment="Brace: {inner}"),
            Move(3, Direction.STAY, Direction.STAY, comment="Newline: \n test"),
        ]

        metadata = ReplayMetadata(maze_height=5, maze_width=5)
        initial_state = InitialState(width=5, height=5, cheese=[(2, 2)])
        replay = Replay(metadata=metadata, initial_state=initial_state, moves=moves)

        writer = ReplayWriter()
        content = writer.format_replay(replay)

        # Check special characters are preserved
        assert 'Quote: "test"' in content
        assert "Brace: {inner}" in content
        # Newline might be problematic

    def test_negative_time_values(self):
        """Test handling of negative time values."""
        # This shouldn't happen but let's be defensive
        move = Move(
            1, Direction.UP, Direction.DOWN, rat_time_ms=-50, python_time_ms=-100
        )

        metadata = ReplayMetadata(maze_height=5, maze_width=5)
        initial_state = InitialState(width=5, height=5, cheese=[(2, 2)])
        replay = Replay(metadata=metadata, initial_state=initial_state, moves=[move])

        writer = ReplayWriter()
        content = writer.format_replay(replay)

        # Should write negative times
        assert "(-50ms/-100ms)" in content
