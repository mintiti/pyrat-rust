"""AI process management and protocol communication."""

import os
import queue
import subprocess
import sys
import threading
import time
from dataclasses import dataclass
from enum import Enum
from typing import TYPE_CHECKING, Dict, Optional

from pyrat_engine.core import Direction

if TYPE_CHECKING:
    from pyrat_engine.core import PyRat
from pyrat_runner.logger import GameLogger


class AIState(Enum):
    """AI process state."""

    NOT_STARTED = "not_started"
    HANDSHAKE = "handshake"
    READY = "ready"
    PREPROCESSING = "preprocessing"
    PLAYING = "playing"
    CRASHED = "crashed"
    TIMED_OUT = "timed_out"


@dataclass
class AIInfo:
    """AI identification information."""

    name: str = "Unknown AI"
    author: Optional[str] = None
    options: Optional[Dict[str, str]] = None

    def __post_init__(self) -> None:
        if self.options is None:
            self.options = {}


class AIProcess:
    """Manages an AI subprocess and protocol communication."""

    def __init__(
        self,
        script_path: str,
        player_name: str,
        timeout: float = 1.0,
        logger: Optional[GameLogger] = None,
    ):
        """
        Initialize AI process manager.

        Args:
            script_path: Path to the AI script
            player_name: "rat" or "python"
            timeout: Default timeout in seconds for AI responses
        """
        self.script_path = script_path
        self.player_name = player_name
        self.timeout = timeout
        self.process: Optional[subprocess.Popen[str]] = None
        self.state = AIState.NOT_STARTED
        self.info = AIInfo()
        self._output_queue: queue.Queue[str] = queue.Queue()
        self._reader_thread: Optional[threading.Thread] = None
        self._stderr_thread: Optional[threading.Thread] = None
        self._logger = logger

    def _reader(self) -> None:
        """Background thread to read output from AI process."""
        try:
            while self.process and self.process.poll() is None:
                assert self.process.stdout is not None
                line = self.process.stdout.readline()
                if line:
                    stripped = line.strip()
                    if self._logger:
                        self._logger.protocol(self.player_name, "←", stripped)
                    self._output_queue.put(stripped)
                else:
                    break
        except (ValueError, OSError):  # Stream closed or I/O error
            pass

    def _stderr_reader(self) -> None:
        """Background thread to drain stderr and log it."""
        try:
            if not self.process or not self.process.stderr:
                return
            while self.process and self.process.poll() is None:
                line = self.process.stderr.readline()
                if not line:
                    break
                if self._logger:
                    self._logger.stderr(self.player_name, line)
        except (ValueError, OSError):
            pass

    def start(self) -> bool:
        """
        Start the AI process and perform handshake.

        Returns:
            True if successful, False otherwise
        """
        try:
            cmd = [sys.executable, "-u", self.script_path]
            env = os.environ.copy()
            env.setdefault("PYTHONUNBUFFERED", "1")
            env.setdefault("PYTHONIOENCODING", "utf-8")
            self.process = subprocess.Popen(
                cmd,
                stdin=subprocess.PIPE,
                stdout=subprocess.PIPE,
                stderr=subprocess.PIPE,
                text=True,
                bufsize=1,
                env=env,
            )
            self.state = AIState.HANDSHAKE

            # Start reader thread
            self._reader_thread = threading.Thread(target=self._reader, daemon=True)
            self._reader_thread.start()
            # Start stderr drainer
            self._stderr_thread = threading.Thread(
                target=self._stderr_reader, daemon=True
            )
            self._stderr_thread.start()

            # Send initial "pyrat" command
            self._write_line("pyrat")

            # Read AI identification
            deadline = time.time() + (self.timeout * 3)  # Give more time for handshake
            while time.time() < deadline:
                line = self._read_line(timeout=deadline - time.time())
                if line is None:
                    # Check if process crashed
                    if self.process.poll() is not None:
                        # Type narrowing: process exists and stderr is not None (we created it with PIPE)
                        assert self.process.stderr is not None
                        stderr_output = self.process.stderr.read()
                        print(
                            f"AI {self.player_name} crashed during handshake",
                            file=sys.stderr,
                        )
                        if stderr_output:
                            print(f"stderr: {stderr_output}", file=sys.stderr)
                        self.state = AIState.CRASHED
                        return False
                    continue

                if line.startswith("id name "):
                    self.info.name = line[8:].strip()
                elif line.startswith("id author "):
                    self.info.author = line[10:].strip()
                elif line.startswith("option "):
                    # Parse option (not implemented yet)
                    pass
                elif line == "pyratready":
                    self.state = AIState.READY
                    return True

            # Timeout during handshake
            print(f"AI {self.player_name} timed out during handshake", file=sys.stderr)
            self.state = AIState.TIMED_OUT
            return False

        except Exception as e:
            print(f"Error starting AI {self.player_name}: {e}", file=sys.stderr)
            import traceback

            traceback.print_exc()
            self.state = AIState.CRASHED
            return False

    def send_game_start(
        self, game_state: "PyRat", preprocessing_time: float = 3.0
    ) -> None:
        """
        Send game initialization messages.

        Args:
            game_state: PyRat instance
            preprocessing_time: Time allowed for preprocessing in seconds
        """
        self._write_line("newgame")

        # Send maze dimensions
        self._write_line(f"maze height:{game_state.height} width:{game_state.width}")

        # Send walls
        walls = game_state.wall_entries()
        if walls:
            walls_str = " ".join(
                f"({w.pos1.x},{w.pos1.y})-({w.pos2.x},{w.pos2.y})" for w in walls
            )
            self._write_line(f"walls {walls_str}")

        # Send mud
        mud_entries = game_state.mud_entries()
        if mud_entries:
            mud_parts = []
            for m in mud_entries:
                mud_parts.append(
                    f"({m.pos1.x},{m.pos1.y})-({m.pos2.x},{m.pos2.y}):{m.value}"
                )
            self._write_line(f"mud {' '.join(mud_parts)}")

        # Send cheese
        cheese = game_state.cheese_positions()
        if cheese:
            cheese_str = " ".join(f"({c.x},{c.y})" for c in cheese)
            self._write_line(f"cheese {cheese_str}")

        # Send player positions (protocol-compliant)
        p1_pos = game_state.player1_position
        p2_pos = game_state.player2_position
        self._write_line(f"player1 rat ({p1_pos.x},{p1_pos.y})")
        self._write_line(f"player2 python ({p2_pos.x},{p2_pos.y})")

        # Tell AI which player it is
        self._write_line(f"youare {self.player_name}")

        # Time controls (use move and preprocessing keys per spec)
        self._write_line(
            f"timecontrol move:{int(self.timeout * 1000)} preprocessing:{int(preprocessing_time * 1000)}"
        )

        # Start preprocessing
        self.state = AIState.PREPROCESSING
        self._write_line("startpreprocessing")

        # Wait for preprocessing done
        deadline = time.time() + preprocessing_time
        while time.time() < deadline:
            line = self._read_line(timeout=deadline - time.time())
            if line == "preprocessingdone":
                self.state = AIState.PLAYING
                return
            elif line and line.startswith("info "):
                # AI is sending info during preprocessing, ignore for now
                pass

        # Preprocessing timeout - continue anyway
        self.state = AIState.PLAYING

    def get_move(
        self, rat_move: Direction, python_move: Direction
    ) -> Optional[Direction]:
        """
        Request a move from the AI.

        Args:
            rat_move: Last move made by rat (or STAY on first turn)
            python_move: Last move made by python (or STAY on first turn)

        Returns:
            Direction or None if timeout/crash
        """
        if self.state != AIState.PLAYING:
            return None

        # Send previous moves
        rat_move_name = Direction(rat_move).name
        python_move_name = Direction(python_move).name
        self._write_line(f"moves rat:{rat_move_name} python:{python_move_name}")

        # Send "go" command
        self._write_line("go")

        # Wait for move response
        deadline = time.time() + self.timeout
        while time.time() < deadline:
            line = self._read_line(timeout=deadline - time.time())
            if line is None:
                continue

            if line.startswith("move "):
                move_str = line[5:].strip().upper()
                try:
                    return Direction[move_str]
                except KeyError:
                    return Direction.STAY
            elif line.startswith("info "):
                # AI is sending info during move calculation, ignore for now
                pass

        # Small grace window to catch just-late responses
        line = self._read_line(timeout=0.05)
        if line and line.startswith("move "):
            move_str = line[5:].strip().upper()
            try:
                return Direction[move_str]
            except KeyError:
                pass

        # Timeout: treat as non-fatal; caller will default to STAY
        return None

    def send_game_over(
        self, winner: str, rat_score: float, python_score: float
    ) -> None:
        """
        Notify AI that game is over.

        Args:
            winner: "rat", "python", or "draw"
            rat_score: Final rat score
            python_score: Final python score
        """
        self._write_line(f"gameover winner:{winner} score:{rat_score}-{python_score}")

        # Start postprocessing (give 1 second)
        self._write_line("startpostprocessing")
        deadline = time.time() + 1.0
        while time.time() < deadline:
            line = self._read_line(timeout=deadline - time.time())
            if line == "postprocessingdone":
                break

    def stop(self) -> None:
        """Stop the AI process."""
        if self.process:
            self._write_line("stop")
            try:
                self.process.wait(timeout=0.5)
            except subprocess.TimeoutExpired:
                self.process.kill()
                self.process.wait()

    def _write_line(self, line: str) -> None:
        """Write a line to AI stdin."""
        if self.process and self.process.stdin:
            try:
                if self._logger:
                    self._logger.protocol(self.player_name, "→", line)
                self.process.stdin.write(line + "\n")
                self.process.stdin.flush()
            except BrokenPipeError:
                self.state = AIState.CRASHED

    def _read_line(self, timeout: float) -> Optional[str]:
        """
        Read a line from AI stdout with timeout.

        Args:
            timeout: Timeout in seconds

        Returns:
            Line string (without newline) or None if timeout
        """
        try:
            return self._output_queue.get(timeout=timeout)
        except queue.Empty:
            return None

    def notify_timeout(self, default_move: Direction) -> None:
        """Inform the AI process that a move timed out.

        Sends a protocol timeout notification so the AI can adjust and
        keep its internal state in sync.

        Args:
            default_move: The move the engine defaulted to (usually STAY)
        """
        move_name = Direction(default_move).name
        self._write_line(f"timeout move:{move_name}")

    def ready_probe(self, timeout: float = 0.5) -> bool:
        """Probe the AI for responsiveness using isready/readyok.

        This uses the synchronization primitive that AIs must respond to
        even while calculating, providing a quick liveness check.

        Args:
            timeout: Maximum time to wait for 'readyok'

        Returns:
            True if 'readyok' is received, False otherwise
        """
        # Ask for readiness
        self._write_line("isready")

        deadline = time.time() + timeout
        while time.time() < deadline:
            remaining = max(0.0, deadline - time.time())
            line = self._read_line(timeout=remaining)
            if line is None:
                continue
            if line == "readyok":
                return True
            # Ignore other outputs (info/move) while probing
        return False

    def is_alive(self) -> bool:
        """Check if AI process is still alive (OS-level)."""
        return self.process is not None and self.process.poll() is None
