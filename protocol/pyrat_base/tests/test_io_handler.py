"""Tests for the IOHandler module."""

import io
import threading
import time
from unittest.mock import MagicMock, patch

from pyrat_base import CommandType, IOHandler


class TestIOHandler:
    """Test the IOHandler class."""

    def test_init(self):
        """Test IOHandler initialization."""
        handler = IOHandler()
        assert handler._running is True
        assert handler._command_queue.empty()
        assert handler._reader_thread.is_alive()
        handler.close()

    @patch("sys.stdin")
    def test_context_manager(self, mock_stdin):
        """Test IOHandler as context manager."""
        # Mock stdin to block on readline (simulating waiting for input)
        mock_stdin.isatty.return_value = False
        mock_stdin.readline = MagicMock(side_effect=lambda: time.sleep(10))

        with IOHandler() as handler:
            assert handler._running is True
            # Thread should be alive and waiting for input
            assert handler._reader_thread.is_alive()
        # After context exit, should be closed
        assert handler._running is False

    @patch("sys.stdin", new_callable=io.StringIO)
    def test_stdin_reading(self, mock_stdin):
        """Test reading commands from stdin."""
        mock_stdin.write("pyrat\nisready\ngo\n")
        mock_stdin.seek(0)
        mock_stdin.isatty = MagicMock(return_value=False)

        with IOHandler() as handler:
            # Give reader thread time to process
            time.sleep(0.1)

            # Should have read 3 commands
            cmd1 = handler.read_command()
            assert cmd1 is not None
            assert cmd1.type == CommandType.PYRAT

            cmd2 = handler.read_command()
            assert cmd2 is not None
            assert cmd2.type == CommandType.ISREADY

            cmd3 = handler.read_command()
            assert cmd3 is not None
            assert cmd3.type == CommandType.GO

            # No more commands
            assert handler.read_command() is None

    @patch("sys.stdin", new_callable=io.StringIO)
    def test_unknown_command_ignored(self, mock_stdin):
        """Test that unknown commands are ignored."""
        mock_stdin.write("pyrat\nunknowncommand\nisready\n")
        mock_stdin.seek(0)
        # Mock isatty to return False so we don't use select
        mock_stdin.isatty = MagicMock(return_value=False)

        with IOHandler() as handler:
            time.sleep(0.2)  # Give more time for processing

            # Should only get valid commands
            cmd1 = handler.read_command()
            assert cmd1 is not None
            assert cmd1.type == CommandType.PYRAT

            # Give time for the unknown command to be processed and ignored
            time.sleep(0.1)

            cmd2 = handler.read_command()
            assert cmd2 is not None
            assert cmd2.type == CommandType.ISREADY

            assert handler.read_command() is None

    def test_write_response(self, capsys):
        """Test writing responses to stdout."""
        with IOHandler() as handler:
            handler.write_response("pyratready")
            handler.write_response("readyok")

        captured = capsys.readouterr()
        assert "pyratready\n" in captured.out
        assert "readyok\n" in captured.out

    def test_write_response_thread_safe(self, capsys):
        """Test thread-safe writing."""
        responses = []

        def write_many(handler, prefix, count):
            for i in range(count):
                msg = f"{prefix}{i}"
                handler.write_response(msg)
                responses.append(msg)

        with IOHandler() as handler:
            # Create multiple threads writing concurrently
            threads = []
            for prefix in ["A", "B", "C"]:
                t = threading.Thread(target=write_many, args=(handler, prefix, 10))
                threads.append(t)
                t.start()

            for t in threads:
                t.join()

        captured = capsys.readouterr()
        # All responses should be in output
        for response in responses:
            assert response in captured.out

    def test_has_command(self):
        """Test checking for available commands."""
        with IOHandler() as handler:
            assert not handler.has_command()

            # Manually add a command to queue
            from pyrat_base.protocol import Command

            handler._command_queue.put(Command(CommandType.PYRAT, {}))

            assert handler.has_command()
            handler.read_command()
            assert not handler.has_command()

    def test_read_command_timeout(self):
        """Test read_command with timeout."""
        with IOHandler() as handler:
            # No commands available, should timeout
            start = time.time()
            cmd = handler.read_command(timeout=0.1)
            elapsed = time.time() - start

            assert cmd is None
            # Timing boundaries aren't magic numbers - they verify expected timeout behavior
            assert 0.08 < elapsed < 0.15  # noqa: PLR2004

    def test_move_calculation(self):
        """Test move calculation thread management."""

        def calculate_move(state, stop_event=None):
            # Simulate some calculation
            for _ in range(5):
                if stop_event and stop_event.is_set():
                    return "INTERRUPTED"
                time.sleep(0.01)
            return "UP"

        with IOHandler() as handler:
            # Start calculation
            handler.start_move_calculation(calculate_move, "dummy_state")

            # Wait for completion
            result, exception = handler.wait_for_calculation(timeout=1.0)
            assert result == "UP"
            assert exception is None

    def test_stop_calculation(self):
        """Test interrupting move calculation."""

        def slow_calculation(state, stop_event=None):
            # Check for interruption frequently
            for _ in range(100):
                if stop_event and stop_event.is_set():
                    return "STAY"  # Best move so far
                time.sleep(0.01)
            return "UP"

        with IOHandler() as handler:
            # Start calculation
            handler.start_move_calculation(slow_calculation, "dummy_state")

            # Let it run briefly
            time.sleep(0.05)

            # Stop it
            result = handler.stop_calculation()
            assert result == "STAY"

    def test_calculation_exception(self):
        """Test handling exceptions in calculation thread."""

        def failing_calculation(state, stop_event=None):
            raise ValueError("Calculation failed")

        with IOHandler() as handler:
            handler.start_move_calculation(failing_calculation, "dummy_state")

            result, exception = handler.wait_for_calculation(timeout=1.0)
            assert result is None
            assert isinstance(exception, ValueError)
            assert str(exception) == "Calculation failed"

    def test_multiple_calculations(self):
        """Test running multiple calculations sequentially."""

        def calc1(state, stop_event=None):
            time.sleep(0.05)
            return "UP"

        def calc2(state, stop_event=None):
            time.sleep(0.05)
            return "DOWN"

        with IOHandler() as handler:
            # First calculation
            handler.start_move_calculation(calc1, "state1")
            result1, _ = handler.wait_for_calculation()
            assert result1 == "UP"

            # Second calculation (should stop first if still running)
            handler.start_move_calculation(calc2, "state2")
            result2, _ = handler.wait_for_calculation()
            assert result2 == "DOWN"

    def test_debug_mode(self, capsys):
        """Test debug logging."""
        with IOHandler(debug=True) as handler:
            handler.write_response("test message")

        captured = capsys.readouterr()
        assert "[IOHandler] Sent: test message" in captured.err

    @patch("sys.stdin", new_callable=io.StringIO)
    def test_eof_handling(self, mock_stdin):
        """Test handling EOF on stdin."""
        mock_stdin.write("pyrat\n")
        mock_stdin.seek(0)
        mock_stdin.isatty = MagicMock(return_value=False)

        with IOHandler() as handler:
            time.sleep(0.1)

            # Should read one command
            cmd = handler.read_command()
            assert cmd.type == CommandType.PYRAT

            # Close stdin to simulate EOF
            mock_stdin.close()

            # Reader thread should handle EOF gracefully
            time.sleep(0.1)

            # No more commands
            assert handler.read_command() is None

    @patch("sys.stdin", new_callable=io.StringIO)
    def test_empty_lines_ignored(self, mock_stdin):
        """Test that empty lines are ignored."""
        mock_stdin.write("pyrat\n\n\nisready\n\n")
        mock_stdin.seek(0)
        mock_stdin.isatty = MagicMock(return_value=False)

        with IOHandler() as handler:
            time.sleep(0.1)

            # Should only get non-empty commands
            cmd1 = handler.read_command()
            assert cmd1.type == CommandType.PYRAT

            cmd2 = handler.read_command()
            assert cmd2.type == CommandType.ISREADY

            assert handler.read_command() is None

    def test_should_stop_method(self):
        """Test the should_stop method of CalculationThread."""
        from pyrat_base.io_handler import CalculationThread

        def dummy_calc(state, stop_event=None):
            return "DONE"

        thread = CalculationThread(dummy_calc, ("state",), {})
        assert not thread.should_stop()
        thread.stop()
        assert thread.should_stop()

    @patch("sys.stdin", new_callable=io.StringIO)
    def test_debug_logging_received(self, mock_stdin, capsys):
        """Test debug logging when receiving commands."""
        mock_stdin.write("pyrat\nunknowncommand\n")
        mock_stdin.seek(0)
        mock_stdin.isatty = MagicMock(return_value=False)

        with IOHandler(debug=True) as handler:
            time.sleep(0.1)

            # Read the valid command
            cmd = handler.read_command()
            assert cmd.type == CommandType.PYRAT

        captured = capsys.readouterr()
        assert "[IOHandler] Received: pyrat" in captured.err
        assert "[IOHandler] Unknown command ignored: unknowncommand" in captured.err

    def test_stop_existing_calculation(self):
        """Test that starting a new calculation stops the existing one."""
        stop_count = [0]

        def long_calc(state, stop_event=None):
            # This will be interrupted
            for _ in range(100):
                if stop_event and stop_event.is_set():
                    stop_count[0] += 1
                    return "INTERRUPTED"
                time.sleep(0.01)
            return "COMPLETED"

        def quick_calc(state, stop_event=None):
            return "QUICK"

        with IOHandler() as handler:
            # Start long calculation
            # Assignment documents that start_move_calculation returns a thread object
            thread1 = handler.start_move_calculation(long_calc, "state1")  # noqa: F841
            time.sleep(0.05)  # Let it run a bit

            # Start new calculation (should stop the first)
            # Assignment verifies the method returns a thread even when stopping previous
            thread2 = handler.start_move_calculation(quick_calc, "state2")  # noqa: F841

            # Wait for second calculation
            result, _ = handler.wait_for_calculation()
            assert result == "QUICK"

            # First thread should have been stopped
            assert stop_count[0] > 0

    def test_wait_calculation_no_thread(self):
        """Test wait_for_calculation when no thread exists."""
        with IOHandler() as handler:
            # No calculation thread started
            result, exception = handler.wait_for_calculation()
            assert result is None
            assert exception is None

    def test_stop_calculation_no_thread(self):
        """Test stop_calculation when no thread exists."""
        with IOHandler() as handler:
            # No calculation thread started
            result = handler.stop_calculation()
            assert result is None

    def test_calculation_with_exception_no_result(self):
        """Test that calculation with exception returns None result in stop_calculation."""

        def failing_calc(state, stop_event=None):
            raise ValueError("Calc failed")

        with IOHandler() as handler:
            handler.start_move_calculation(failing_calc, "state")
            time.sleep(0.05)  # Let it fail

            # stop_calculation should return None when there's an exception
            result = handler.stop_calculation()
            assert result is None

    def test_wait_calculation_timeout_still_running(self):
        """Test wait_for_calculation with timeout when thread is still running."""

        def slow_calc(state, stop_event=None):
            time.sleep(1.0)  # Very slow
            return "SLOW"

        with IOHandler() as handler:
            handler.start_move_calculation(slow_calc, "state")

            # Wait with short timeout
            result, exception = handler.wait_for_calculation(timeout=0.05)
            assert result is None  # Still running
            assert exception is None

            # Clean up
            handler.stop_calculation()

    @patch("sys.stdin", new_callable=io.StringIO)
    def test_reader_thread_exception_handling(self, mock_stdin, capsys):
        """Test that reader thread continues after exceptions."""
        # Create a stdin that causes an exception then recovers
        mock_stdin.readline = MagicMock(
            side_effect=[
                Exception("Read error"),  # First call fails
                "pyrat\n",  # Second call succeeds
                "",  # EOF
            ]
        )
        mock_stdin.isatty = MagicMock(return_value=False)

        with IOHandler(debug=True) as handler:
            time.sleep(0.2)  # Give reader time to handle exception and recover

            # Should still get the command after the exception
            cmd = handler.read_command()
            assert cmd is not None
            assert cmd.type == CommandType.PYRAT

        captured = capsys.readouterr()
        assert "[IOHandler] Reader thread error: Read error" in captured.err
