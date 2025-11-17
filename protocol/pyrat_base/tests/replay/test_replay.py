"""Tests for the PyRat replay system."""

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


class TestReplayMetadata:
    """Test replay metadata handling."""

    def test_default_metadata(self):
        """Test default metadata values."""
        meta = ReplayMetadata()
        assert meta.event == "?"
        assert meta.site == "?"
        assert meta.date == "????.??.??"
        assert meta.round_ == "-"
        assert meta.result == "*"
        expected_size = 10
        assert meta.maze_height == expected_size
        assert meta.maze_width == expected_size

    def test_custom_metadata(self):
        """Test creating metadata with custom values."""
        meta = ReplayMetadata(
            event="Test Tournament",
            rat="GreedyBot",
            python="RandomBot",
            result="1-0",
            maze_height=15,
            maze_width=21,
        )
        assert meta.event == "Test Tournament"
        assert meta.rat == "GreedyBot"
        assert meta.result == "1-0"
        expected_maze_height = 15
        assert meta.maze_height == expected_maze_height


class TestInitialState:
    """Test initial state handling."""

    def test_default_initial_state(self):
        """Test default initial state."""
        state = InitialState(width=10, height=10)
        expected_size = 10
        assert state.width == expected_size
        assert state.height == expected_size
        assert state.walls == []
        assert state.mud == []
        assert state.cheese == []
        assert state.rat_position == (0, 0)
        assert state.python_position == (0, 0)
        expected_max_turns = 300
        assert state.max_turns == expected_max_turns

    def test_custom_initial_state(self):
        """Test creating initial state with custom values."""
        walls = [((0, 0), (0, 1)), ((1, 1), (2, 1))]
        mud = [(((2, 2), (2, 3)), 3)]
        cheese = [(5, 5), (7, 8)]

        state = InitialState(
            width=15,
            height=10,
            walls=walls,
            mud=mud,
            cheese=cheese,
            rat_position=(14, 9),
            python_position=(0, 0),
        )
        assert state.walls == walls
        assert state.mud == mud
        assert state.cheese == cheese
        assert state.rat_position == (14, 9)


class TestReplayReaderWriter:
    """Test replay reading and writing."""

    def test_minimal_replay(self):
        """Test reading/writing minimal replay."""
        # Create minimal replay
        metadata = ReplayMetadata(maze_height=5, maze_width=5)
        initial_state = InitialState(width=5, height=5, cheese=[(2, 2)])
        moves = [
            Move(turn=1, rat_move=Direction.STAY, python_move=Direction.UP),
            Move(turn=2, rat_move=Direction.RIGHT, python_move=Direction.STAY),
        ]
        replay = Replay(metadata=metadata, initial_state=initial_state, moves=moves)

        # Write and read back
        with tempfile.NamedTemporaryFile(mode="w", suffix=".pyrat", delete=False) as f:
            writer = ReplayWriter()
            writer.write_file(replay, f.name)

            reader = ReplayReader()
            loaded_replay = reader.read_file(f.name)

        # Verify
        expected_size = 5
        assert loaded_replay.metadata.maze_height == expected_size
        assert loaded_replay.metadata.maze_width == expected_size
        assert loaded_replay.initial_state.cheese == [(2, 2)]
        expected_moves_count = 2
        assert len(loaded_replay.moves) == expected_moves_count
        assert loaded_replay.moves[0].rat_move == Direction.STAY
        assert loaded_replay.moves[1].python_move == Direction.STAY

    def test_full_replay(self):
        """Test reading/writing complete replay with all features."""
        # Create full replay
        metadata = ReplayMetadata(
            event="Test Championship",
            site="Online",
            date="2025.01.15",
            round_="3",
            rat="AlphaBot",
            python="BetaBot",
            result="0-1",
            maze_height=10,
            maze_width=15,
            time_control="100+3000+1000",
            rat_author="Alice",
            python_author="Bob",
            replay_id="test-123",
            termination="score_threshold",
            final_score="2-3",
            total_turns=42,
        )

        initial_state = InitialState(
            width=15,
            height=10,
            walls=[((0, 0), (0, 1)), ((5, 5), (6, 5))],
            mud=[(((2, 2), (3, 2)), 2), (((7, 7), (7, 8)), 3)],
            cheese=[(1, 1), (8, 8), (14, 9)],
            rat_position=(14, 9),
            python_position=(0, 0),
        )

        moves = [
            Move(1, Direction.UP, Direction.RIGHT, 50, 75, "Opening"),
            Move(2, Direction.LEFT, "*", 30, None, "Python timeout!"),
            Move(3, Direction.DOWN, Direction.UP, 45, 80),
        ]

        replay = Replay(
            metadata=metadata,
            initial_state=initial_state,
            moves=moves,
            preprocessing_done=True,
            postprocessing_done=True,
        )

        # Write and read back
        with tempfile.NamedTemporaryFile(mode="w", suffix=".pyrat", delete=False) as f:
            writer = ReplayWriter()
            writer.write_file(replay, f.name)

            reader = ReplayReader()
            loaded_replay = reader.read_file(f.name)

        # Verify metadata
        assert loaded_replay.metadata.event == "Test Championship"
        assert loaded_replay.metadata.rat_author == "Alice"
        expected_total_turns = 42
        assert loaded_replay.metadata.total_turns == expected_total_turns

        # Verify initial state
        expected_walls_count = 2
        expected_mud_count = 2
        assert len(loaded_replay.initial_state.walls) == expected_walls_count
        assert len(loaded_replay.initial_state.mud) == expected_mud_count
        assert loaded_replay.initial_state.rat_position == (14, 9)

        # Verify moves
        expected_moves_count = 3
        assert len(loaded_replay.moves) == expected_moves_count
        assert loaded_replay.moves[0].comment == "Opening"
        assert loaded_replay.moves[1].python_move == "*"  # Timeout
        assert loaded_replay.moves[1].python_time_ms is None

        # Verify phase markers
        assert loaded_replay.preprocessing_done
        assert loaded_replay.postprocessing_done

    def test_parse_move_notations(self):
        """Test parsing different move notations."""
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

1. S/U
2. L/R (50ms/75ms)
3. D/* (30ms/100ms) {Timeout}
4. U/S (45ms/?) {Unknown time}
"""
        reader = ReplayReader()
        replay = reader.parse(content)

        expected_moves_count = 4
        assert len(replay.moves) == expected_moves_count
        assert replay.moves[0].rat_move == Direction.STAY
        expected_time_ms = 50
        assert replay.moves[1].rat_time_ms == expected_time_ms
        assert replay.moves[2].python_move == "*"
        assert replay.moves[2].comment == "Timeout"
        assert replay.moves[3].python_time_ms is None

    def test_custom_tags(self):
        """Test handling of custom tags."""
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
[CustomTag "CustomValue"]
[AnotherTag "AnotherValue"]

W:
M:
C:
R:(0,0)
P:(4,4)
"""
        reader = ReplayReader()
        replay = reader.parse(content)

        assert "CustomTag" in replay.metadata.custom_tags
        assert replay.metadata.custom_tags["CustomTag"] == "CustomValue"
        assert replay.metadata.custom_tags["AnotherTag"] == "AnotherValue"


class TestStreamingReplayWriter:
    """Test streaming replay writer."""

    def test_streaming_write(self):
        """Test writing replay incrementally."""
        with tempfile.NamedTemporaryFile(mode="w", suffix=".pyrat", delete=False) as f:
            filename = f.name

        # Write incrementally
        with StreamingReplayWriter(filename) as writer:
            # Write metadata
            metadata = ReplayMetadata(
                event="Streaming Test",
                rat="Bot1",
                python="Bot2",
                maze_height=5,
                maze_width=5,
            )
            writer.write_metadata(metadata)

            # Write initial state
            initial_state = InitialState(
                width=5,
                height=5,
                cheese=[(2, 2)],
                rat_position=(0, 0),
                python_position=(4, 4),
            )
            writer.write_initial_state(initial_state)

            # Write preprocessing marker
            writer.write_preprocessing_done()

            # Write moves one by one
            writer.write_move(Move(1, Direction.UP, Direction.DOWN, 50, 60))
            writer.write_move(Move(2, Direction.RIGHT, Direction.LEFT, 45, 55))

            # Write postprocessing marker
            writer.write_postprocessing_done()

        # Read back and verify
        reader = ReplayReader()
        replay = reader.read_file(filename)

        assert replay.metadata.event == "Streaming Test"
        assert replay.initial_state.cheese == [(2, 2)]
        expected_moves_count = 2
        assert len(replay.moves) == expected_moves_count
        assert replay.preprocessing_done
        assert replay.postprocessing_done

    def test_streaming_writer_errors(self):
        """Test error handling in streaming writer."""
        with tempfile.NamedTemporaryFile(mode="w", suffix=".pyrat", delete=False) as f:
            filename = f.name

        with StreamingReplayWriter(filename) as writer:
            # Try to write initial state before metadata
            with pytest.raises(RuntimeError, match="Must write metadata first"):
                writer.write_initial_state(InitialState(5, 5))

            # Write metadata
            writer.write_metadata(ReplayMetadata())

            # Try to write metadata again
            with pytest.raises(RuntimeError, match="Metadata already written"):
                writer.write_metadata(ReplayMetadata())

            # Write initial state
            writer.write_initial_state(InitialState(5, 5))

            # Try to write initial state again
            with pytest.raises(RuntimeError, match="Initial state already written"):
                writer.write_initial_state(InitialState(5, 5))


class TestReplayPlayer:
    """Test replay player functionality."""

    def test_replay_simple_game(self):
        """Test replaying a simple game."""
        # Create a simple replay
        metadata = ReplayMetadata(maze_height=5, maze_width=5)
        initial_state = InitialState(
            width=5,
            height=5,
            cheese=[(2, 2)],
            rat_position=(0, 0),
            python_position=(4, 4),
        )
        moves = [
            Move(1, Direction.RIGHT, Direction.LEFT),
            Move(2, Direction.RIGHT, Direction.LEFT),
            Move(3, Direction.UP, Direction.DOWN),
        ]
        replay = Replay(metadata=metadata, initial_state=initial_state, moves=moves)

        # Create player and verify initial state
        player = ReplayPlayer(replay)
        assert player.current_turn == 0
        assert not player.is_finished()

        # Step through moves
        result1 = player.step_forward()
        assert result1 is not None
        assert player.current_turn == 1

        player.step_forward()
        expected_turn_2 = 2
        assert player.current_turn == expected_turn_2

        player.step_forward()
        expected_turn_3 = 3
        assert player.current_turn == expected_turn_3

        # No more moves
        result4 = player.step_forward()
        assert result4 is None
        assert player.is_finished()

    def test_jump_to_turn(self):
        """Test jumping to specific turns."""
        # Create replay with 5 moves
        metadata = ReplayMetadata(maze_height=5, maze_width=5)
        initial_state = InitialState(
            width=5,
            height=5,
            cheese=[(2, 2)],
            rat_position=(0, 0),
            python_position=(4, 4),
        )
        moves = [Move(i, Direction.STAY, Direction.STAY) for i in range(1, 6)]
        replay = Replay(metadata=metadata, initial_state=initial_state, moves=moves)

        player = ReplayPlayer(replay)

        # Jump forward
        player.jump_to_turn(3)
        expected_turn = 3
        assert player.current_turn == expected_turn

        # Jump backward (should reset and replay)
        player.jump_to_turn(1)
        assert player.current_turn == 1

        # Jump to end
        player.jump_to_turn(10)  # Beyond end
        expected_turn = 5
        assert player.current_turn == expected_turn
        assert player.is_finished()

    def test_get_move_at_current_turn(self):
        """Test getting the current move."""
        metadata = ReplayMetadata(maze_height=5, maze_width=5)
        initial_state = InitialState(width=5, height=5)
        moves = [
            Move(1, Direction.UP, Direction.DOWN, comment="First move"),
            Move(2, Direction.LEFT, Direction.RIGHT, comment="Second move"),
        ]
        replay = Replay(metadata=metadata, initial_state=initial_state, moves=moves)

        player = ReplayPlayer(replay)

        # Before any moves
        assert player.get_move_at_current_turn() is None

        # After first move
        player.step_forward()
        current_move = player.get_move_at_current_turn()
        assert current_move is not None
        assert current_move.comment == "First move"
        assert current_move.rat_move == Direction.UP

    def test_replay_with_timeouts(self):
        """Test replaying games with timeout moves."""
        metadata = ReplayMetadata(maze_height=5, maze_width=5)
        initial_state = InitialState(width=5, height=5)
        moves = [
            Move(1, Direction.UP, "*"),  # Python timeout
            Move(2, "*", Direction.DOWN),  # Rat timeout
        ]
        replay = Replay(metadata=metadata, initial_state=initial_state, moves=moves)

        player = ReplayPlayer(replay)

        # Timeouts should be converted to STAY
        result1 = player.step_forward()
        assert result1 is not None
        # Game state will have executed UP for rat, STAY for python

        result2 = player.step_forward()
        assert result2 is not None
        # Game state will have executed STAY for rat, DOWN for python


class TestReplayFormatExamples:
    """Test parsing example replay formats from the specification."""

    def test_minimal_valid_replay(self):
        """Test parsing the minimal valid replay from spec."""
        content = """[Event "?"]
[Site "?"]
[Date "????.??.??"]
[Round "-"]
[Rat "?"]
[Python "?"]
[Result "*"]
[MazeHeight "10"]
[MazeWidth "10"]
[TimeControl "100+0+0"]

W:
M:
C:(5,5)
R:(9,9)
P:(0,0)

1. S/S
"""
        reader = ReplayReader()
        replay = reader.parse(content)

        expected_maze_height = 10
        assert replay.metadata.maze_height == expected_maze_height
        assert replay.initial_state.cheese == [(5, 5)]
        assert replay.initial_state.rat_position == (9, 9)
        assert len(replay.moves) == 1

    def test_comments_and_annotations(self):
        """Test parsing replays with comments."""
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

; This is a comment about the game
W:(0,0)-(0,1)
M:
C:(2,2)
R:(0,0)
P:(4,4)

; Opening phase
1. U/D {Both players move vertically}
2. R/L {Now horizontally}
# This is an analysis comment
3. S/S {Both stay}
"""
        reader = ReplayReader()
        replay = reader.parse(content)

        expected_moves_count = 3
        assert len(replay.moves) == expected_moves_count
        assert replay.moves[0].comment == "Both players move vertically"
        assert replay.moves[2].rat_move == Direction.STAY
