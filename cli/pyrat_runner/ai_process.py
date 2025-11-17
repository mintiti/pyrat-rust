"""AI process management and protocol communication."""

import queue
import subprocess
import sys
import threading
import time
from dataclasses import dataclass
from enum import Enum
from typing import Optional

from pyrat_engine.game import Direction


# Direction name mapping
DIRECTION_NAMES = {0: "UP", 1: "RIGHT", 2: "DOWN", 3: "LEFT", 4: "STAY"}

# Reverse mapping for parsing
DIRECTION_FROM_NAME = {
    "UP": Direction.UP,
    "DOWN": Direction.DOWN,
    "LEFT": Direction.LEFT,
    "RIGHT": Direction.RIGHT,
    "STAY": Direction.STAY,
}


def get_direction_name(direction: Direction) -> str:
    """Get the string name of a Direction."""
    return DIRECTION_NAMES.get(int(direction), "STAY")


def parse_direction(name: str) -> Direction:
    """Parse a Direction from its string name."""
    return DIRECTION_FROM_NAME.get(name, Direction.STAY)


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
    options: dict = None

    def __post_init__(self):
        if self.options is None:
            self.options = {}


class AIProcess:
    """Manages an AI subprocess and protocol communication."""

    def __init__(self, script_path: str, player_name: str, timeout: float = 1.0):
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
        self.process: Optional[subprocess.Popen] = None
        self.state = AIState.NOT_STARTED
        self.info = AIInfo()
        self._output_queue: queue.Queue[str] = queue.Queue()
        self._reader_thread = None

    def _reader(self):
        """Background thread to read output from AI process."""
        try:
            while self.process and self.process.poll() is None:
                line = self.process.stdout.readline()
                if line:
                    self._output_queue.put(line.strip())
                else:
                    break
        except (ValueError, OSError):  # Stream closed or I/O error
            pass

    def start(self) -> bool:
        """
        Start the AI process and perform handshake.

        Returns:
            True if successful, False otherwise
        """
        try:
            self.process = subprocess.Popen(
                [sys.executable, self.script_path],
                stdin=subprocess.PIPE,
                stdout=subprocess.PIPE,
                stderr=subprocess.PIPE,
                text=True,
                bufsize=1,
            )
            self.state = AIState.HANDSHAKE

            # Start reader thread
            self._reader_thread = threading.Thread(target=self._reader, daemon=True)
            self._reader_thread.start()

            # Send initial "pyrat" command
            self._write_line("pyrat")

            # Read AI identification
            deadline = time.time() + (self.timeout * 3)  # Give more time for handshake
            while time.time() < deadline:
                line = self._read_line(timeout=deadline - time.time())
                if line is None:
                    # Check if process crashed
                    if self.process.poll() is not None:
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

    def send_game_start(self, game_state, preprocessing_time: float = 3.0):
        """
        Send game initialization messages.

        Args:
            game_state: PyRat instance
            preprocessing_time: Time allowed for preprocessing in seconds
        """
        self._write_line("newgame")

        # Send maze dimensions
        self._write_line(
            f"maze height:{game_state._game.height} width:{game_state._game.width}"
        )

        # Send walls
        walls = game_state._game.wall_entries()
        if walls:
            walls_str = " ".join(
                f"({w[0][0]},{w[0][1]})-({w[1][0]},{w[1][1]})" for w in walls
            )
            self._write_line(f"walls {walls_str}")

        # Send mud
        mud = game_state.mud_positions
        if mud:
            mud_parts = []
            for (cell1, cell2), turns in mud.items():
                mud_parts.append(
                    f"({cell1[0]},{cell1[1]})-({cell2[0]},{cell2[1]}):{turns}"
                )
            self._write_line(f"mud {' '.join(mud_parts)}")

        # Send cheese
        cheese = game_state.cheese_positions
        if cheese:
            cheese_str = " ".join(f"({c[0]},{c[1]})" for c in cheese)
            self._write_line(f"cheese {cheese_str}")

        # Send player positions
        p1_pos = game_state.player1_pos
        p2_pos = game_state.player2_pos
        self._write_line(f"rat position:({p1_pos[0]},{p1_pos[1]})")
        self._write_line(f"python position:({p2_pos[0]},{p2_pos[1]})")

        # Tell AI which player it is
        self._write_line(f"youare {self.player_name}")

        # Time controls (using defaults from spec)
        self._write_line(
            f"timecontrol preprocessing:{int(preprocessing_time * 1000)} turn:{int(self.timeout * 1000)}"
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
        rat_move_name = get_direction_name(rat_move)
        python_move_name = get_direction_name(python_move)
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
                move_str = line[5:].strip()
                direction = parse_direction(move_str)
                if direction is None:
                    print(
                        f"Invalid move from {self.player_name}: {move_str}",
                        file=sys.stderr,
                    )
                    return Direction.STAY
                return direction
            elif line.startswith("info "):
                # AI is sending info during move calculation, ignore for now
                pass

        # Timeout
        self.state = AIState.TIMED_OUT
        return None

    def send_game_over(self, winner: str, rat_score: float, python_score: float):
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

    def stop(self):
        """Stop the AI process."""
        if self.process:
            self._write_line("stop")
            try:
                self.process.wait(timeout=0.5)
            except subprocess.TimeoutExpired:
                self.process.kill()
                self.process.wait()

    def _write_line(self, line: str):
        """Write a line to AI stdin."""
        if self.process and self.process.stdin:
            try:
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

    def is_alive(self) -> bool:
        """Check if AI process is still alive."""
        return (
            self.process is not None
            and self.process.poll() is None
            and self.state not in [AIState.CRASHED, AIState.TIMED_OUT]
        )
