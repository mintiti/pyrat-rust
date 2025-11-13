"""PyRat AI Base Class.

This module provides the base class for developing PyRat AIs that communicate
via the PyRat protocol. Users inherit from PyRatAI and implement the get_move()
method to create their AI strategy.

Example:
    >>> from pyrat_base import PyRatAI, ProtocolState
    >>> from pyrat_engine.core.types import Direction
    >>>
    >>> class MyAI(PyRatAI):
    ...     def get_move(self, state: ProtocolState) -> Direction:
    ...         # Implement your strategy here
    ...         return Direction.UP
    >>>
    >>> if __name__ == "__main__":
    ...     ai = MyAI("MyBot v1.0", "Student Name")
    ...     ai.run()
"""

import os
import sys
import time
from typing import Any, Dict, List, Optional, Tuple

from pyrat_engine.core import DirectionType
from pyrat_engine.core.game import GameState as PyGameState
from pyrat_engine.core.types import Direction

from .enums import CommandType, GameResult, Player, ResponseType
from .io_handler import IOHandler
from .protocol import DIRECTION_INT_TO_NAME, Protocol
from .protocol_state import ProtocolState


class PyRatAI:
    """Base class for PyRat AI implementations.

    This class handles all protocol communication, allowing users to focus on
    implementing their AI strategy. Users must implement the get_move() method
    and can optionally override preprocessing, postprocessing, and option handling.

    Attributes:
        name: The name of the AI
        author: The author of the AI (optional)
        debug: Whether debug mode is enabled
    """

    def __init__(self, name: str, author: Optional[str] = None):
        """Initialize the AI with name and optional author.

        Args:
            name: The name of your AI (e.g., "GreedyBot v1.0")
            author: Your name (optional)
        """
        self.name = name
        self.author = author
        # Enable debug if PYRAT_DEBUG environment variable is set
        self.debug = bool(
            os.environ.get("PYRAT_DEBUG", "").lower() in ("1", "true", "yes")
        )

        if self.debug:
            print(
                f"[PyRatAI] Debug mode enabled for {name}", file=sys.stderr, flush=True
            )

        # Internal state - pass debug flag to IOHandler
        self._io = IOHandler(debug=self.debug)
        self._protocol = Protocol()
        self._state = "INITIAL"
        self._game_state: Optional[PyGameState] = None
        self._player: Optional[Player] = None
        self._options: Dict[str, Any] = {}
        self._time_limits: Dict[str, int] = {
            "move": 100,  # Default 100ms per move
            "preprocessing": 3000,  # Default 3s preprocessing
            "postprocessing": 1000,  # Default 1s postprocessing
        }

        # For building game state during initialization
        self._game_config: Dict[str, Any] = {}

        # Track game result for postprocessing
        self._game_result: Optional[GameResult] = None
        self._final_score: Optional[Tuple[float, float]] = None

    # Abstract method - users must implement
    def get_move(self, state: ProtocolState) -> DirectionType:
        """Calculate the next move given the current game state.

        This is the main method users must implement. It receives the current
        game state and should return a direction value (int) indicating the desired move.

        The method can be interrupted by a 'stop' command, in which case the
        best move found so far should be returned.

        Args:
            state: The current game state from the AI's perspective

        Returns:
            The direction to move (UP, DOWN, LEFT, RIGHT, or STAY)

        Raises:
            NotImplementedError: If not overridden by subclass
        """
        raise NotImplementedError("Subclasses must implement get_move()")

    # Optional hooks for users to override
    def preprocess(self, state: ProtocolState, time_limit_ms: int) -> None:
        """Preprocessing phase for analyzing the maze structure.

        This method is called once at the start of the game after the maze
        has been initialized but before the first move. Use it to analyze
        the maze structure, precompute paths, or initialize data structures.

        Args:
            state: The initial game state
            time_limit_ms: Time limit in milliseconds for preprocessing
        """
        pass

    def postprocess(
        self, state: ProtocolState, result: GameResult, time_limit_ms: int
    ) -> None:
        """Postprocessing phase for learning and analysis.

        This method is called once after the game ends. Use it to analyze
        the game, update learning parameters, or save data.

        Args:
            state: The final game state
            result: The game result (RAT, PYTHON, or DRAW)
            time_limit_ms: Time limit in milliseconds for postprocessing
        """
        pass

    def get_options(self) -> List[Dict[str, Any]]:
        """Declare configuration options for your AI.

        Override this method to declare options that can be configured
        before the game starts. Options are sent during the handshake phase.

        Returns:
            List of option dictionaries with keys:
                - name: Option name
                - type: One of "check", "spin", "combo", "string", "button"
                - default: Default value
                - Additional keys depending on type (min/max for spin, var for combo)

        Example:
            return [
                {"name": "SearchDepth", "type": "spin", "default": 3, "min": 1, "max": 10},
                {"name": "Strategy", "type": "combo", "default": "Balanced",
                 "var": ["Aggressive", "Balanced", "Defensive"]}
            ]
        """
        return []

    def on_option_set(self, name: str, value: str) -> None:
        """Handle option changes from the engine.

        This method is called when the engine sets an option value.
        Override it to update your AI's configuration.

        Args:
            name: The option name
            value: The new value as a string
        """
        self._options[name] = value

    # Convenience methods for users
    def send_info(self, **kwargs: Any) -> None:
        """Send information messages during move calculation.

        Use this to report progress, current evaluation, or debug information.
        Multiple key-value pairs can be sent in one call.

        Supported keys:
            - nodes: Number of nodes evaluated
            - depth: Current search depth
            - time: Time spent in milliseconds
            - currmove: Move currently being evaluated
            - currline: Current line being analyzed (list of moves)
            - score: Position evaluation
            - pv: Principal variation (best line found)
            - target: Current target position
            - string: Any debug/status message

        Example:
            self.send_info(depth=3, nodes=1000, string="Analyzing defensive moves")
        """
        response = self._protocol.format_response(ResponseType.INFO, kwargs)
        self._io.write_response(response)

    def log(self, message: str) -> None:
        """Log a debug message (only shown if debug mode is enabled).

        Args:
            message: The message to log
        """
        if self.debug:
            self.send_info(string=f"[DEBUG] {message}")

    # Main protocol loop
    def run(self) -> None:  # noqa: C901, PLR0912
        """Run the AI protocol loop.

        This is the main entry point that handles all protocol communication.
        It runs until the process is terminated or receives EOF on stdin.

        Note: Complexity warnings disabled as this is a protocol state machine
        that requires handling many different states and commands.
        """
        if self.debug:
            print(
                f"[PyRatAI] Starting protocol loop in state: {self._state}",
                file=sys.stderr,
                flush=True,
            )

        try:
            while True:
                # Check for commands
                cmd = self._io.read_command(timeout=0.1)
                if cmd is None:
                    continue

                # Always handle isready
                if cmd.type == CommandType.ISREADY:
                    self._io.write_response("readyok")
                    continue

                # Dispatch based on current state
                if self._state == "INITIAL":
                    self._handle_initial(cmd)
                elif self._state == "HANDSHAKE":
                    self._handle_handshake(cmd)
                elif self._state == "READY":
                    self._handle_ready(cmd)
                elif self._state == "GAME_INIT":
                    self._handle_game_init(cmd)
                elif self._state == "PREPROCESSING":
                    self._handle_preprocessing(cmd)
                elif self._state == "PLAYING":
                    self._handle_playing(cmd)
                elif self._state == "POSTPROCESSING":
                    self._handle_postprocessing(cmd)

        except KeyboardInterrupt:
            pass
        except Exception as e:
            # Log error but don't crash
            if self.debug:
                print(f"[ERROR] {e}", file=sys.stderr, flush=True)
        finally:
            self._io.close()

    def _handle_initial(self, cmd: Any) -> None:
        """Handle commands in INITIAL state."""
        if cmd.type == CommandType.PYRAT:
            if self.debug:
                print(
                    "[PyRatAI] Received PYRAT command, sending identification",
                    file=sys.stderr,
                    flush=True,
                )

            # Send identification
            response = self._protocol.format_response(
                ResponseType.ID, {"name": self.name}
            )
            self._io.write_response(response)

            if self.author:
                response = self._protocol.format_response(
                    ResponseType.ID, {"author": self.author}
                )
                self._io.write_response(response)

            # Send options
            for option in self.get_options():
                response = self._protocol.format_response(ResponseType.OPTION, option)
                self._io.write_response(response)

            # Send ready
            self._io.write_response("pyratready")
            self._state = "HANDSHAKE"

            if self.debug:
                print(
                    "[PyRatAI] Handshake complete, transitioning to HANDSHAKE state",
                    file=sys.stderr,
                    flush=True,
                )

    def _handle_handshake(self, cmd: Any) -> None:
        """Handle commands in HANDSHAKE state."""
        if cmd.type == CommandType.DEBUG:
            self.debug = cmd.data.get("enabled", False)
        elif cmd.type == CommandType.SETOPTION:
            name = cmd.data.get("name", "")
            value = cmd.data.get("value", "")
            self.on_option_set(name, value)
        elif cmd.type == CommandType.NEWGAME:
            self._state = "GAME_INIT"
            self._game_config = {}
        elif cmd.type == CommandType.RECOVER:
            # Recovery protocol - wait for full state
            self._state = "GAME_INIT"
            self._game_config = {"recover": True}

    def _handle_ready(self, cmd: Any) -> None:
        """Handle commands in READY state."""
        if cmd.type == CommandType.DEBUG:
            self.debug = cmd.data.get("enabled", False)
        elif cmd.type == CommandType.SETOPTION:
            name = cmd.data.get("name", "")
            value = cmd.data.get("value", "")
            self.on_option_set(name, value)
        elif cmd.type == CommandType.NEWGAME:
            self._state = "GAME_INIT"
            self._game_config = {}
        elif cmd.type == CommandType.STARTPREPROCESSING:
            self._state = "PREPROCESSING"
            self._run_preprocessing()
        elif cmd.type == CommandType.GO:
            self._state = "PLAYING"
            self._handle_go_command()
        elif cmd.type == CommandType.READY:
            # Ready check after timeout
            self._io.write_response("ready")

    def _handle_game_init(self, cmd: Any) -> None:  # noqa: C901, PLR0912, PLR0915
        """Handle commands during game initialization.

        Note: Complexity warnings disabled as game initialization requires
        handling many different protocol commands to build the game state.
        """
        # Collect game configuration
        if cmd.type == CommandType.MAZE:
            self._game_config["width"] = cmd.data["width"]
            self._game_config["height"] = cmd.data["height"]
        elif cmd.type == CommandType.WALLS:
            self._game_config["walls"] = cmd.data.get("positions", [])
        elif cmd.type == CommandType.MUD:
            self._game_config["mud"] = cmd.data.get("entries", [])
        elif cmd.type == CommandType.CHEESE:
            self._game_config["cheese"] = cmd.data.get("cheese", [])
        elif cmd.type == CommandType.PLAYER1:
            self._game_config["player1_pos"] = cmd.data["position"]
        elif cmd.type == CommandType.PLAYER2:
            self._game_config["player2_pos"] = cmd.data["position"]
        elif cmd.type == CommandType.YOUARE:
            self._player = Player.RAT if cmd.data["player"] == "rat" else Player.PYTHON
            # Check if we have all required data to create game state
            self._try_create_game_state()
        elif cmd.type == CommandType.TIMECONTROL:
            if "move" in cmd.data:
                self._time_limits["move"] = cmd.data["move"]
            if "preprocessing" in cmd.data:
                self._time_limits["preprocessing"] = cmd.data["preprocessing"]
            if "postprocessing" in cmd.data:
                self._time_limits["postprocessing"] = cmd.data["postprocessing"]
        elif cmd.type == CommandType.STARTPREPROCESSING:
            self._state = "PREPROCESSING"
            self._run_preprocessing()
        elif cmd.type == CommandType.GO:
            # Skip preprocessing, go directly to playing
            self._state = "PLAYING"
            self._handle_go_command()
        elif cmd.type == CommandType.MOVES_HISTORY:
            # Recovery: replay all moves
            if self._game_state:
                history = cmd.data.get("history", [])
                # Pair up moves (rat, python) for each turn
                for i in range(0, len(history) - 1, 2):
                    rat_move = history[i]
                    python_move = history[i + 1]
                    self._game_state.step(
                        self._parse_direction(rat_move),
                        self._parse_direction(python_move),
                    )
                # Note: If there's an odd number of moves, the last one is ignored
                # This should only happen if recovery occurs mid-turn
        elif cmd.type == CommandType.CURRENT_POSITION:
            # Recovery: verify positions match
            if self._game_state and "positions" in cmd.data:
                positions = cmd.data["positions"]
                if Player.RAT in positions and Player.PYTHON in positions:
                    actual_rat = self._game_state.player1_position
                    actual_python = self._game_state.player2_position
                    expected_rat = positions[Player.RAT]
                    expected_python = positions[Player.PYTHON]

                    if actual_rat != expected_rat or actual_python != expected_python:
                        self.send_info(
                            warning=f"Position mismatch during recovery! "
                            f"Expected rat:{expected_rat} python:{expected_python}, "
                            f"but have rat:{actual_rat} python:{actual_python}"
                        )
        elif cmd.type == CommandType.SCORE:
            # Recovery: verify scores match
            if self._game_state and "scores" in cmd.data:
                scores = cmd.data["scores"]
                if Player.RAT in scores and Player.PYTHON in scores:
                    actual_rat_score = self._game_state.player1_score
                    actual_python_score = self._game_state.player2_score
                    expected_rat_score = scores[Player.RAT]
                    expected_python_score = scores[Player.PYTHON]

                    if (
                        actual_rat_score != expected_rat_score
                        or actual_python_score != expected_python_score
                    ):
                        self.send_info(
                            warning=f"Score mismatch during recovery! "
                            f"Expected rat:{expected_rat_score} python:{expected_python_score}, "
                            f"but have rat:{actual_rat_score} python:{actual_python_score}"
                        )

    def _handle_preprocessing(self, cmd: Any) -> None:
        """Handle commands during preprocessing phase."""
        # During preprocessing, we might receive timeout
        if cmd.type == CommandType.TIMEOUT and "preprocessing" in cmd.data.get(
            "phase", ""
        ):
            # Preprocessing timed out
            self._state = "READY"

    def _handle_playing(self, cmd: Any) -> None:  # noqa: C901, PLR0912
        """Handle commands during playing state.

        Note: Complexity warning disabled as playing state requires
        handling multiple protocol commands.
        """
        if cmd.type == CommandType.MOVES:
            # Update game state with the moves that were executed
            moves = cmd.data.get("moves", {})
            # Handle both Player enum keys and string keys
            if Player.RAT in moves:
                rat_move = self._parse_direction(moves[Player.RAT])
                python_move = self._parse_direction(moves[Player.PYTHON])
            else:
                # Fallback to string keys
                rat_move = self._parse_direction(moves.get("rat", "STAY"))
                python_move = self._parse_direction(moves.get("python", "STAY"))

            if self._game_state:
                self._game_state.step(rat_move, python_move)
        elif cmd.type == CommandType.GO:
            self._handle_go_command()
        elif cmd.type == CommandType.STOP:
            # Interrupt current calculation
            result = self._io.stop_calculation()
            if result:
                self._io.write_response(f"move {result}")
            else:
                self._io.write_response("move STAY")
        elif cmd.type == CommandType.TIMEOUT:
            # We timed out - engine will use default move
            pass
        elif cmd.type == CommandType.READY:
            # Ready check after timeout
            self._io.write_response("ready")
        elif cmd.type == CommandType.GAMEOVER:
            # Game ended
            winner = cmd.data.get("winner", "draw")
            self._game_result = self._parse_game_result(winner)
            score = cmd.data.get("score", "0-0")
            score_parts = score.split("-")
            if len(score_parts) == 2:  # noqa: PLR2004
                self._final_score = (float(score_parts[0]), float(score_parts[1]))
            self._state = "READY"
        elif cmd.type == CommandType.STARTPOSTPROCESSING:
            self._state = "POSTPROCESSING"
            self._run_postprocessing()

    def _handle_postprocessing(self, cmd: Any) -> None:
        """Handle commands during postprocessing phase."""
        if cmd.type == CommandType.TIMEOUT and "postprocessing" in cmd.data.get(
            "phase", ""
        ):
            # Postprocessing timed out
            self._state = "READY"

    def _try_create_game_state(self) -> None:
        """Try to create game state if we have all required data."""
        required = {
            "width",
            "height",
            "walls",
            "mud",
            "cheese",
            "player1_pos",
            "player2_pos",
        }
        if required.issubset(self._game_config.keys()) and self._player:
            # Create the game state
            self._game_state = PyGameState.create_custom(
                width=self._game_config["width"],
                height=self._game_config["height"],
                walls=self._game_config["walls"],
                mud=self._game_config["mud"],
                cheese=self._game_config["cheese"],
                player1_pos=self._game_config["player1_pos"],
                player2_pos=self._game_config["player2_pos"],
            )
            self._state = "READY"

    def _run_preprocessing(self) -> None:
        """Run preprocessing in a separate thread."""
        if not self._game_state or not self._player:
            # Can't preprocess without game state
            self._io.write_response("preprocessingdone")
            self._state = "READY"
            return

        def preprocess_wrapper(stop_event: Any) -> str:
            try:
                assert self._game_state is not None
                assert self._player is not None
                state = ProtocolState(self._game_state, self._player)
                self.preprocess(state, self._time_limits["preprocessing"])
            except Exception as e:
                if self.debug:
                    self.log(f"Preprocessing error: {e}")
            return "done"

        # Start preprocessing in thread
        self._io.start_move_calculation(preprocess_wrapper)

        # Wait for completion or timeout
        result, exception = self._io.wait_for_calculation(
            timeout=self._time_limits["preprocessing"] / 1000.0
        )

        # Send response
        self._io.write_response("preprocessingdone")
        self._state = "READY"

    def _run_postprocessing(self) -> None:
        """Run postprocessing in a separate thread."""
        if not self._game_state or not self._player or not self._game_result:
            # Can't postprocess without game state and result
            self._io.write_response("postprocessingdone")
            self._state = "READY"
            return

        def postprocess_wrapper(stop_event: Any) -> str:
            try:
                assert self._game_state is not None
                assert self._player is not None
                assert self._game_result is not None
                state = ProtocolState(self._game_state, self._player)
                self.postprocess(
                    state, self._game_result, self._time_limits["postprocessing"]
                )
            except Exception as e:
                if self.debug:
                    self.log(f"Postprocessing error: {e}")
            return "done"

        # Start postprocessing in thread
        self._io.start_move_calculation(postprocess_wrapper)

        # Wait for completion or timeout
        result, exception = self._io.wait_for_calculation(
            timeout=self._time_limits["postprocessing"] / 1000.0
        )

        # Send response
        self._io.write_response("postprocessingdone")
        self._state = "READY"

    def _handle_go_command(self) -> None:  # noqa: C901
        """Handle the go command to calculate a move.

        Note: Complexity warning disabled as this method handles
        concurrent command processing while calculating moves.
        """
        if not self._game_state or not self._player:
            # Can't calculate without game state
            self._io.write_response("move STAY")
            return

        def calculate_move(stop_event: Any) -> str:
            try:
                # Create protocol state wrapper
                assert self._game_state is not None
                assert self._player is not None
                state = ProtocolState(self._game_state, self._player)

                # Store stop event for user's get_move to check
                self._current_stop_event = stop_event

                # Call user's get_move
                move = self.get_move(state)

                # Convert Direction to string
                return DIRECTION_INT_TO_NAME[move]

            except Exception as e:
                if self.debug:
                    self.log(f"Move calculation error: {e}")
                return "STAY"

        # Start calculation in thread
        thread = self._io.start_move_calculation(calculate_move)

        # Process commands while calculating
        deadline = self._time_limits["move"] / 1000.0
        start_time = time.time()

        while thread.is_alive() and (time.time() - start_time) < deadline:
            # Check for urgent commands
            cmd = self._io.read_command(timeout=0.01)
            if cmd:
                if cmd.type == CommandType.STOP:
                    # Interrupt calculation
                    result = self._io.stop_calculation()
                    if result:
                        self._io.write_response(f"move {result}")
                    else:
                        self._io.write_response("move STAY")
                    return
                elif cmd.type == CommandType.ISREADY:
                    self._io.write_response("readyok")
                # Important: Don't drop other commands!
                # Any command that isn't STOP or ISREADY should be re-queued
                # so it can be processed after the move calculation completes.
                # This is critical for MOVES commands which update game state.
                else:
                    # Re-queue the command for processing after calculation
                    self._io.requeue_command(cmd)

        # Get result
        result, exception = self._io.wait_for_calculation(timeout=0.1)

        if result:
            self._io.write_response(f"move {result}")
        else:
            self._io.write_response("move STAY")

    def _parse_direction(self, move_str: str) -> DirectionType:
        """Parse a move string to direction value."""
        if not move_str:
            return Direction.STAY
        move_str = str(move_str).upper()
        if move_str == "UP":
            return Direction.UP
        elif move_str == "DOWN":
            return Direction.DOWN
        elif move_str == "LEFT":
            return Direction.LEFT
        elif move_str == "RIGHT":
            return Direction.RIGHT
        else:
            return Direction.STAY

    def _parse_game_result(self, result_str: str) -> GameResult:
        """Parse a game result string to GameResult enum."""
        if result_str == "rat":
            return GameResult.RAT
        elif result_str == "python":
            return GameResult.PYTHON
        else:
            return GameResult.DRAW

    @property
    def is_computing(self) -> bool:
        """Check if currently computing a move.

        This can be useful in get_move() to periodically check if
        calculation should be interrupted.

        Returns:
            True if a stop event has been signaled
        """
        if hasattr(self, "_current_stop_event") and self._current_stop_event:
            return bool(self._current_stop_event.is_set())
        return False
