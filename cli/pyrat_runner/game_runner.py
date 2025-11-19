"""Game orchestration and execution."""

import sys
import time
from typing import Optional, Tuple

from pyrat_engine import PyRat
from pyrat_engine.core import Direction

from .ai_process import AIProcess
from .display import Display
from .logger import GameLogger


# ===========================
# Pure game logic functions
# ===========================


def determine_winner_from_scores(rat_score: float, python_score: float) -> str:
    """Determine the winner based on final scores.

    Pure function - no side effects, same inputs always produce same outputs.

    Args:
        rat_score: Rat's final score
        python_score: Python's final score

    Returns:
        Winner: "rat", "python", or "draw"
    """
    if rat_score > python_score:
        return "rat"
    elif python_score > rat_score:
        return "python"
    else:
        return "draw"


def classify_ai_move_error(
    is_alive: bool, move: Optional[Direction]
) -> Tuple[bool, Direction, Optional[str]]:
    """Classify AI move error and determine appropriate response.

    Pure function - decision logic without side effects.

    Args:
        is_alive: Whether the AI process is still alive
        move: Move returned by AI (None if timeout/crash)

    Returns:
        Tuple of (should_continue, move_to_use, error_message)
        - should_continue: False if AI crashed, True if just timeout or success
        - move_to_use: Direction to use (original move or STAY)
        - error_message: Error message to display (None if no error)
    """
    if move is None:
        if not is_alive:
            return False, Direction.STAY, "AI process crashed"
        else:
            return True, Direction.STAY, "AI timed out, defaulting to STAY"
    return True, move, None


class GameRunner:
    """Orchestrates a PyRat game between two AI processes."""

    def __init__(
        self,
        rat_script: str,
        python_script: str,
        width: int = 21,
        height: int = 15,
        cheese_count: int = 41,
        seed: Optional[int] = None,
        turn_timeout: float = 1.0,
        preprocessing_timeout: float = 3.0,
        display_delay: float = 0.3,
        log_dir: Optional[str] = None,
    ):
        """
        Initialize game runner.

        Args:
            rat_script: Path to rat AI script
            python_script: Path to python AI script
            width: Maze width
            height: Maze height
            cheese_count: Number of cheese pieces
            seed: Random seed (None for random)
            turn_timeout: Timeout for AI move in seconds
            preprocessing_timeout: Timeout for preprocessing in seconds
            display_delay: Delay between turns for visualization
        """
        self.rat_script = rat_script
        self.python_script = python_script

        # Create game
        self.game = PyRat(
            width=width, height=height, cheese_count=cheese_count, seed=seed
        )

        # Optional logger
        self.logger: Optional[GameLogger] = GameLogger(log_dir) if log_dir else None

        # Create AI processes
        self.rat_ai = AIProcess(
            rat_script, "rat", timeout=turn_timeout, logger=self.logger
        )
        self.python_ai = AIProcess(
            python_script, "python", timeout=turn_timeout, logger=self.logger
        )

        # Display
        self.display = Display(self.game, delay=display_delay)

        # Configuration
        self.turn_timeout = turn_timeout
        self.preprocessing_timeout = preprocessing_timeout
        self.display_delay = display_delay

    def _start_ai_processes(self) -> bool:
        """Start both AI processes and display their information.

        Returns:
            True if both processes started successfully, False otherwise
        """
        print("Starting AI processes...")
        if self.logger:
            self.logger.event("Starting AI processes")

        if not self.rat_ai.start():
            print(f"Failed to start Rat AI: {self.rat_script}", file=sys.stderr)
            if self.logger:
                self.logger.event("Failed to start Rat AI")
            return False

        if not self.python_ai.start():
            print(f"Failed to start Python AI: {self.python_script}", file=sys.stderr)
            if self.logger:
                self.logger.event("Failed to start Python AI")
            self.rat_ai.stop()
            return False

        # Display AI information
        print(f"Rat AI: {self.rat_ai.info.name}")
        if self.rat_ai.info.author:
            print(f"  Author: {self.rat_ai.info.author}")

        print(f"Python AI: {self.python_ai.info.name}")
        if self.python_ai.info.author:
            print(f"  Author: {self.python_ai.info.author}")

        print()
        return True

    def _initialize_game(self) -> None:
        """Initialize the game by sending game state to AIs and showing initial display."""
        print("Initializing game...")
        if self.logger:
            self.logger.event("Initializing game")
        time.sleep(1)

        # Send game start to both AIs
        self.rat_ai.send_game_start(self.game, self.preprocessing_timeout)
        self.python_ai.send_game_start(self.game, self.preprocessing_timeout)

        # Show initial state
        self.display.render()
        time.sleep(self.display_delay)

    def _handle_ai_move_error(
        self, player: str, ai: AIProcess, move: Optional[Direction]
    ) -> Tuple[bool, Direction]:
        """Handle AI move timeout or crash.

        Args:
            player: Player name ("rat" or "python")
            ai: AIProcess instance
            move: Move returned by AI (None if timeout/crash)

        Returns:
            Tuple of (should_continue, move_to_use)
            - should_continue: False if AI crashed, True if just timeout
            - move_to_use: Direction.STAY if timeout, original move otherwise
        """
        # Use pure function to classify error
        should_continue, move_to_use, error_message = classify_ai_move_error(
            ai.is_alive(), move
        )

        # Handle timeout case with protocol notifications
        if move is None and should_continue:
            # Inform AI that we defaulted the move
            ai.notify_timeout(Direction.STAY)
            # Probe responsiveness quickly via isready/readyok
            responsive = ai.ready_probe(timeout=0.5)
            if not responsive:
                # Add supplemental warning but keep playing
                extra = "AI did not respond to isready after timeout"
                self.display.show_error(player, extra)

        # Handle side effect (display error) if needed
        if error_message:
            self.display.show_error(player, error_message)

        return should_continue, move_to_use

    def _get_ai_moves(
        self, rat_prev_move: Direction, python_prev_move: Direction
    ) -> Tuple[bool, Optional[Direction], Optional[Direction]]:
        """Get moves from both AIs with error handling.

        Args:
            rat_prev_move: Rat's previous move
            python_prev_move: Python's previous move

        Returns:
            Tuple of (success, rat_move, python_move)
            - success: False if any AI crashed
            - rat_move: Move from rat AI (or Direction.STAY if timeout)
            - python_move: Move from python AI (or Direction.STAY if timeout)
        """
        # Request moves from both AIs
        rat_move = self.rat_ai.get_move(rat_prev_move, python_prev_move)
        python_move = self.python_ai.get_move(rat_prev_move, python_prev_move)

        # Handle rat AI errors
        should_continue, rat_move = self._handle_ai_move_error(
            "rat", self.rat_ai, rat_move
        )
        if not should_continue:
            return False, None, None

        # Handle python AI errors
        should_continue, python_move = self._handle_ai_move_error(
            "python", self.python_ai, python_move
        )
        if not should_continue:
            return False, None, None

        return True, rat_move, python_move

    def _execute_game_loop(self) -> bool:
        """Execute the main game loop.

        Returns:
            True if game completed without errors, False if AI crashed
        """
        rat_move = Direction.STAY
        python_move = Direction.STAY

        while True:
            # Get moves from both AIs
            success, rat_move_new, python_move_new = self._get_ai_moves(
                rat_move, python_move
            )
            if not success:
                return False

            # Type narrowing: success=True guarantees moves are not None
            assert rat_move_new is not None
            assert python_move_new is not None
            rat_move = rat_move_new
            python_move = python_move_new

            # Execute move in game
            result = self.game.step(p1_move=rat_move, p2_move=python_move)

            # Update display
            self.display.render(rat_move, python_move)
            time.sleep(self.display_delay)

            # Check for game over
            if result.game_over:
                return True

    def _determine_winner(self) -> Tuple[str, float, float]:
        """Determine the winner based on final scores.

        Returns:
            Tuple of (winner, rat_score, python_score)
            - winner: "rat", "python", or "draw"
            - rat_score: Rat's final score
            - python_score: Python's final score
        """
        scores = self.game.scores
        rat_score = scores[0]
        python_score = scores[1]

        # Use pure function to determine winner
        winner = determine_winner_from_scores(rat_score, python_score)

        return winner, rat_score, python_score

    def _finalize_game(
        self, winner: str, rat_score: float, python_score: float
    ) -> None:
        """Finalize the game by notifying AIs and displaying results.

        Args:
            winner: "rat", "python", or "draw"
            rat_score: Rat's final score
            python_score: Python's final score
        """
        # Send game over to AIs
        self.rat_ai.send_game_over(winner, rat_score, python_score)
        self.python_ai.send_game_over(winner, rat_score, python_score)
        if self.logger:
            self.logger.event(
                f"Game over: winner={winner} score={rat_score}-{python_score}"
            )

        # Display final result
        self.display.show_winner(winner, rat_score, python_score)

        # Stop AI processes
        self.rat_ai.stop()
        self.python_ai.stop()
        if self.logger:
            self.logger.close()

    def run(self) -> bool:
        """
        Run the game.

        Returns:
            True if successful, False if errors occurred
        """
        # Start AI processes
        if not self._start_ai_processes():
            return False

        # Initialize game
        self._initialize_game()

        # Execute game loop
        success = self._execute_game_loop()

        # Determine winner and finalize
        winner, rat_score, python_score = self._determine_winner()
        self._finalize_game(winner, rat_score, python_score)

        return success
