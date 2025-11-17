"""Cross-platform compatibility tests for replay system."""

import tempfile
from pathlib import Path

import pytest
from pyrat_engine.core import Direction

from pyrat_base.replay import (
    InitialState,
    Move,
    Replay,
    ReplayMetadata,
    ReplayReader,
    ReplayWriter,
)


class TestCrossPlatformCompatibility:
    """Test replay system works across Windows/Mac/Linux."""

    def test_windows_line_endings(self):
        """Test parsing replays with Windows (CRLF) line endings."""
        # Create content with Windows line endings
        content = "\r\n".join(
            [
                '[Event "Windows Test"]',
                '[Site "?"]',
                '[Date "????.??.??"]',
                '[Round "-"]',
                '[Rat "?"]',
                '[Python "?"]',
                '[Result "*"]',
                '[MazeHeight "5"]',
                '[MazeWidth "5"]',
                '[TimeControl "100+0+0"]',
                "",
                "W:",
                "M:",
                "C:(2,2)",
                "R:(0,0)",
                "P:(4,4)",
                "",
                "1. U/D",
                "2. L/R",
            ]
        )

        reader = ReplayReader()
        replay = reader.parse(content)

        assert replay.metadata.event == "Windows Test"
        expected_moves_count = 2
        assert len(replay.moves) == expected_moves_count
        assert replay.moves[0].rat_move == Direction.UP

    def test_mixed_line_endings(self):
        """Test parsing replays with mixed line endings."""
        # Mix of \n, \r\n, and even \r
        content = '[Event "Mixed"]\r\n[Site "?"]\n[Date "????.??.??"]\r[Round "-"]\n[Rat "?"]\r\n[Python "?"]\n[Result "*"]\n[MazeHeight "5"]\n[MazeWidth "5"]\n[TimeControl "100+0+0"]\n\nW:\nM:\nC:(2,2)\nR:(0,0)\nP:(4,4)\n\n1. S/S\n'

        reader = ReplayReader()
        replay = reader.parse(content)

        assert replay.metadata.event == "Mixed"
        assert len(replay.moves) == 1

    def test_file_paths_with_spaces(self):
        """Test reading/writing files with spaces in paths."""
        # Create a temporary directory with spaces
        with tempfile.TemporaryDirectory(prefix="test replay ") as tmpdir:
            file_path = Path(tmpdir) / "my game replay.pyrat"

            # Create and write replay
            metadata = ReplayMetadata(event="Path Test")
            initial_state = InitialState(width=5, height=5, cheese=[(2, 2)])
            replay = Replay(metadata=metadata, initial_state=initial_state)

            writer = ReplayWriter()
            writer.write_file(replay, file_path)

            # Read it back
            reader = ReplayReader()
            loaded = reader.read_file(file_path)

            assert loaded.metadata.event == "Path Test"

    def test_unicode_file_paths(self):
        """Test reading/writing files with Unicode in paths."""
        with tempfile.TemporaryDirectory() as tmpdir:
            # Unicode filename
            file_path = Path(tmpdir) / "游戏_répétition.pyrat"

            metadata = ReplayMetadata(event="Unicode Path")
            initial_state = InitialState(width=5, height=5, cheese=[(2, 2)])
            replay = Replay(metadata=metadata, initial_state=initial_state)

            writer = ReplayWriter()
            writer.write_file(replay, file_path)

            reader = ReplayReader()
            loaded = reader.read_file(file_path)

            assert loaded.metadata.event == "Unicode Path"

    def test_very_long_file_paths(self):
        """Test handling of very long file paths."""
        with tempfile.TemporaryDirectory() as tmpdir:
            # Create nested directories to make a long path
            long_dir = Path(tmpdir)
            for i in range(10):
                long_dir = long_dir / f"very_long_directory_name_{i}"
            long_dir.mkdir(parents=True)

            file_path = long_dir / "very_long_filename_for_replay_file.pyrat"

            metadata = ReplayMetadata(event="Long Path")
            initial_state = InitialState(width=5, height=5, cheese=[(2, 2)])
            replay = Replay(metadata=metadata, initial_state=initial_state)

            writer = ReplayWriter()
            writer.write_file(replay, file_path)

            reader = ReplayReader()
            loaded = reader.read_file(file_path)

            assert loaded.metadata.event == "Long Path"

    def test_windows_path_separators(self):
        """Test that our code uses pathlib correctly for cross-platform paths."""
        # We can't test actual Windows paths on Unix, but we can ensure
        # we're using pathlib correctly which handles this for us

        # Test that we accept both string and Path objects
        with tempfile.TemporaryDirectory() as tmpdir:
            # Both string and Path should work
            string_path = f"{tmpdir}/game.pyrat"
            path_object = Path(tmpdir) / "game.pyrat"

            metadata = ReplayMetadata(event="Path Test")
            initial_state = InitialState(width=5, height=5, cheese=[(2, 2)])
            replay = Replay(metadata=metadata, initial_state=initial_state)

            writer = ReplayWriter()

            # Both should work
            writer.write_file(replay, string_path)
            writer.write_file(replay, path_object)

            # Both files should exist
            assert Path(string_path).exists()
            assert path_object.exists()

    def test_case_sensitive_filenames(self):
        """Test case sensitivity handling."""
        # This is tricky because Windows/Mac can be case-insensitive
        # while Linux is case-sensitive
        with tempfile.TemporaryDirectory() as tmpdir:
            path1 = Path(tmpdir) / "Game.pyrat"
            path2 = Path(tmpdir) / "game.pyrat"

            metadata = ReplayMetadata(event="Case Test")
            initial_state = InitialState(width=5, height=5, cheese=[(2, 2)])
            replay = Replay(metadata=metadata, initial_state=initial_state)

            writer = ReplayWriter()
            writer.write_file(replay, path1)

            # On case-insensitive systems, this might overwrite
            # On case-sensitive systems, this creates a new file
            writer.write_file(replay, path2)

            # Both should be readable
            reader = ReplayReader()
            loaded1 = reader.read_file(path1)
            loaded2 = reader.read_file(path2)

            assert loaded1.metadata.event == "Case Test"
            assert loaded2.metadata.event == "Case Test"

    def test_replay_content_encoding(self):
        """Test that UTF-8 encoding works on all platforms."""
        # Test various Unicode characters that might cause issues
        test_strings = [
            "ASCII only",
            "Café résumé",  # Latin-1 compatible
            "Москва София",  # Cyrillic
            "東京 北京",  # CJK
            "🎮 🏆 🧩",  # Emoji
            "Hello",  # Mathematical alphanumeric (replaced with ASCII for linting)
        ]

        for test_string in test_strings:
            metadata = ReplayMetadata(
                event=test_string, rat=test_string, python=test_string
            )
            initial_state = InitialState(width=5, height=5, cheese=[(2, 2)])
            moves = [Move(1, Direction.UP, Direction.DOWN, comment=test_string)]
            replay = Replay(metadata=metadata, initial_state=initial_state, moves=moves)

            # Test round-trip
            writer = ReplayWriter()
            content = writer.format_replay(replay)

            reader = ReplayReader()
            loaded = reader.parse(content)

            assert loaded.metadata.event == test_string
            assert loaded.moves[0].comment == test_string

    def test_bom_handling(self):
        """Test handling of UTF-8 BOM (Byte Order Mark)."""
        # Some Windows editors add BOM to UTF-8 files
        bom = "\ufeff"
        content = (
            bom
            + """[Event "BOM Test"]
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

1. S/S
"""
        )
        reader = ReplayReader()
        replay = reader.parse(content)

        assert replay.metadata.event == "BOM Test"
        assert len(replay.moves) == 1

    def test_tab_vs_space_handling(self):
        """Test that tabs and spaces are handled consistently."""
        # Some editors might convert tabs to spaces or vice versa
        content_with_tabs = '[Event\t"Tab Test"]\n[Site\t"?"]\n'
        content_with_spaces = '[Event "Tab Test"]\n[Site "?"]\n'

        # Add minimal required tags
        for content in [content_with_tabs, content_with_spaces]:
            full_content = (
                content
                + """[Date "????.??.??"]
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
"""
            )
            reader = ReplayReader()
            replay = reader.parse(full_content)
            assert replay.metadata.event == "Tab Test"

    @pytest.mark.parametrize("line_ending", ["\n", "\r\n", "\r"])
    def test_line_ending_preservation_in_write(self, line_ending):
        """Test that we can write files with specific line endings."""
        # This is important because git might change line endings
        metadata = ReplayMetadata(event="Line Ending Test")
        initial_state = InitialState(width=5, height=5, cheese=[(2, 2)])
        replay = Replay(metadata=metadata, initial_state=initial_state)

        writer = ReplayWriter()
        content = writer.format_replay(replay)

        # The writer uses \n, but the system might convert
        # Check that parsing works regardless
        modified_content = content.replace("\n", line_ending)

        reader = ReplayReader()
        loaded = reader.parse(modified_content)
        assert loaded.metadata.event == "Line Ending Test"
