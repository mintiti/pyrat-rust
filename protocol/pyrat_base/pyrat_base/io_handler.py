"""Non-blocking I/O handler for PyRat protocol communication.

This module provides the IOHandler class which manages:
- Non-blocking stdin reading in a background thread
- Command queue for buffering incoming protocol commands
- Move calculation thread management with interruption support
- Thread-safe stdout writing
"""

import platform
import queue
import select
import sys
import threading
import time
from typing import Any, Callable, Dict, Optional, Tuple

from .protocol import Command, Protocol


class CalculationThread(threading.Thread):
    """Thread for running move calculations that can be interrupted."""

    def __init__(
        self,
        callback: Callable[..., str],
        args: Tuple[Any, ...],
        kwargs: Dict[str, Any],
    ):
        super().__init__(daemon=True)
        self.callback = callback
        self.args = args
        self.kwargs = kwargs
        self.result: Optional[str] = None
        self.exception: Optional[Exception] = None
        self._stop_event = threading.Event()

    def run(self) -> None:
        """Run the calculation with interruption support."""
        try:
            # Pass the stop event to the callback so it can check for interruption
            self.result = self.callback(
                *self.args, stop_event=self._stop_event, **self.kwargs
            )
        except Exception as e:
            self.exception = e

    def stop(self) -> None:
        """Signal the calculation to stop."""
        self._stop_event.set()

    def should_stop(self) -> bool:
        """Check if calculation should stop."""
        return self._stop_event.is_set()


class IOHandler:
    """Handles non-blocking I/O for PyRat protocol communication.

    This class manages:
    - A background thread that continuously reads from stdin
    - A thread-safe queue for buffering parsed commands
    - Move calculation in a separate interruptible thread
    - Synchronized writing to stdout
    """

    def __init__(self, debug: bool = False):
        """Initialize the IOHandler.

        Args:
            debug: If True, enables debug logging of protocol messages
        """
        self.debug = debug
        self._command_queue: queue.Queue[Command] = queue.Queue()
        self._running = True
        self._protocol = Protocol()
        self._calculation_thread: Optional[CalculationThread] = None
        self._write_lock = threading.Lock()

        # Start stdin reader thread
        self._reader_thread = threading.Thread(target=self._stdin_reader, daemon=True)
        self._reader_thread.start()

    def _stdin_reader(self) -> None:  # noqa: C901
        """Background thread that continuously reads from stdin.

        Note: Complexity warning disabled as this method handles multiple
        platform-specific cases, EOF, and error handling in one cohesive flow.
        Breaking it up would harm readability.
        """
        while self._running:
            try:
                # Platform-specific stdin availability check
                if platform.system() != "Windows" and sys.stdin.isatty():
                    # On Unix-like systems, use select for non-blocking check
                    if sys.stdin in select.select([sys.stdin], [], [], 0.1)[0]:
                        line = sys.stdin.readline()
                    else:
                        continue
                else:
                    # On Windows or non-interactive, readline blocks
                    # but that's okay in a separate thread
                    line = sys.stdin.readline()

                if not line:  # EOF
                    break

                line = line.strip()
                if not line:
                    continue

                if self.debug:
                    self._debug_log(f"Received: {line}")

                # Parse command and add to queue
                command = self._protocol.parse_command(line)
                if command is not None:
                    self._command_queue.put(command)
                elif self.debug:
                    self._debug_log(f"Unknown command ignored: {line}")

            except Exception as e:
                if self.debug:
                    self._debug_log(f"Reader thread error: {e}")
                # Add backoff to prevent tight error loops
                time.sleep(0.01)  # 10ms backoff on error
                # Continue reading even on errors

    def read_command(self, timeout: float = 0) -> Optional[Command]:
        """Read a parsed command from the queue.

        Args:
            timeout: Maximum time to wait for a command (0 = non-blocking)

        Returns:
            Parsed Command object or None if no command available
        """
        try:
            if timeout > 0:
                return self._command_queue.get(timeout=timeout)
            else:
                return self._command_queue.get_nowait()
        except queue.Empty:
            return None

    def has_command(self) -> bool:
        """Check if any commands are available in the queue."""
        return not self._command_queue.empty()

    def requeue_command(self, command: Command) -> None:
        """Put a command back into the queue for later processing.

        This is used when a command is read during move calculation but
        can't be processed immediately (e.g., MOVES commands that arrive
        while the AI is still calculating). The command will be processed
        in the next iteration of the main event loop.

        Args:
            command: The command to re-queue
        """
        self._command_queue.put(command)

    def write_response(self, response: str) -> None:
        """Write a response to stdout with proper flushing.

        Args:
            response: The response string to write
        """
        with self._write_lock:
            print(response, flush=True)
            if self.debug:
                self._debug_log(f"Sent: {response}")

    def start_move_calculation(
        self, callback: Callable[..., str], *args: Any, **kwargs: Any
    ) -> CalculationThread:
        """Start move calculation in a separate thread.

        The callback function should accept a 'stop_event' keyword argument
        and periodically check stop_event.is_set() to handle interruption.

        Args:
            callback: Function that calculates and returns a move
            *args: Positional arguments for callback
            **kwargs: Keyword arguments for callback

        Returns:
            The calculation thread object
        """
        # Stop any existing calculation
        if self._calculation_thread and self._calculation_thread.is_alive():
            self.stop_calculation()

        self._calculation_thread = CalculationThread(callback, args, kwargs)
        self._calculation_thread.start()
        return self._calculation_thread

    def stop_calculation(self) -> Optional[str]:
        """Stop the current move calculation.

        Returns:
            The result if calculation completed, None otherwise
        """
        if not self._calculation_thread:
            return None

        self._calculation_thread.stop()
        # Give it a short time to finish gracefully
        self._calculation_thread.join(timeout=0.1)

        if not self._calculation_thread.exception:
            return self._calculation_thread.result
        return None

    def wait_for_calculation(
        self, timeout: Optional[float] = None
    ) -> Tuple[Optional[str], Optional[Exception]]:
        """Wait for the current calculation to complete.

        Args:
            timeout: Maximum time to wait (None = wait forever)

        Returns:
            Tuple of (result, exception) where result is the move or None
        """
        if not self._calculation_thread:
            return None, None

        self._calculation_thread.join(timeout=timeout)

        if self._calculation_thread.is_alive():
            # Still running after timeout
            return None, None

        return self._calculation_thread.result, self._calculation_thread.exception

    def close(self) -> None:
        """Clean shutdown of all threads."""
        self._running = False

        # Stop any ongoing calculation
        if self._calculation_thread and self._calculation_thread.is_alive():
            self.stop_calculation()

        # Wait for reader thread to finish
        self._reader_thread.join(timeout=1.0)

    def _debug_log(self, message: str) -> None:
        """Log debug messages to stderr."""
        print(f"[IOHandler] {message}", file=sys.stderr, flush=True)

    def __enter__(self) -> "IOHandler":
        """Context manager support."""
        return self

    def __exit__(self, exc_type: Any, exc_val: Any, exc_tb: Any) -> None:
        """Context manager cleanup."""
        self.close()
