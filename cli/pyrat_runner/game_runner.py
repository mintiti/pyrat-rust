"""Game orchestration and execution."""

import os
import sys
import time
from concurrent.futures import ThreadPoolExecutor
from datetime import datetime
from typing import Optional, Tuple

from pyrat_engine import PyRat
from pyrat_engine.core import Direction

from .display import Display
from .logger import GameLogger
from .move_providers import MoveProvider, SubprocessMoveProvider


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


# ===========================
# Decoupled game execution
# ===========================


def run_game(
    game: PyRat,
    rat_provider: "MoveProvider",
    python_provider: "MoveProvider",
    display: Optional[Display] = None,
    display_delay: float = 0.3,
) -> Tuple[bool, str, float, float]:
    """Run a PyRat game using move providers.

    Decoupled game execution function that works with any MoveProvider
    implementation, enabling headless execution, testing, and flexible
    move acquisition strategies (subprocess, direct calls, network, etc.).

    Args:
        game: PyRat game instance
        rat_provider: Move provider for rat player
        python_provider: Move provider for python player
        display: Optional display for visualization (None for headless)
        display_delay: Delay between turns for visualization

    Returns:
        Tuple of (success, winner, rat_score, python_score)
        - success: False if any AI crashed, True otherwise
        - winner: "rat", "python", or "draw"
        - rat_score: Rat's final score
        - python_score: Python's final score
    """
    rat_move = Direction.STAY
    python_move = Direction.STAY

    # Request both providers' moves in parallel each turn to keep per-turn wall time bounded
    with ThreadPoolExecutor(max_workers=2) as pool:
        while True:
            # Get moves from both providers concurrently
            fut_rat = pool.submit(rat_provider.get_move, rat_move, python_move)
            fut_py = pool.submit(python_provider.get_move, rat_move, python_move)
            rat_move_new = fut_rat.result()
            python_move_new = fut_py.result()

        # Notify providers of timeout to keep protocol in sync (if supported)
        if (
            rat_move_new is None
            and rat_provider.is_alive()
            and hasattr(rat_provider, "notify_timeout")
        ):
            try:
                getattr(rat_provider, "notify_timeout")(Direction.STAY)
            except Exception:
                pass
        if (
            python_move_new is None
            and python_provider.is_alive()
            and hasattr(python_provider, "notify_timeout")
        ):
            try:
                getattr(python_provider, "notify_timeout")(Direction.STAY)
            except Exception:
                pass

        # Handle rat provider errors
        should_continue, rat_move_processed, error_msg = classify_ai_move_error(
            rat_provider.is_alive(), rat_move_new
        )
        if error_msg and display:
            display.show_error("rat", error_msg)
        if not should_continue:
            # Rat AI crashed - determine final state and return
            scores = game.scores
            winner = determine_winner_from_scores(scores[0], scores[1])
            return False, winner, scores[0], scores[1]

        # Handle python provider errors
        should_continue, python_move_processed, error_msg = classify_ai_move_error(
            python_provider.is_alive(), python_move_new
        )
        if error_msg and display:
            display.show_error("python", error_msg)
        if not should_continue:
            # Python AI crashed - determine final state and return
            scores = game.scores
            winner = determine_winner_from_scores(scores[0], scores[1])
            return False, winner, scores[0], scores[1]

        # Update moves for next iteration
        rat_move = rat_move_processed
        python_move = python_move_processed

        # Execute move in game
        result = game.step(p1_move=rat_move, p2_move=python_move)

        # Update display if provided
        if display:
            display.render(rat_move, python_move)
            time.sleep(display_delay)

        # Check for game over
        if result.game_over:
            scores = game.scores
            winner = determine_winner_from_scores(scores[0], scores[1])
            return True, winner, scores[0], scores[1]


class GameRunner:
    """Orchestrates a PyRat game between two AI processes.

    This class provides a convenient high-level interface for running
    subprocess-based AI games with visualization. It uses the MoveProvider
    abstraction internally for flexibility.
    """

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
        headless: bool = False,
        max_turns: Optional[int] = None,
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
            log_dir: Directory to write logs (protocol, stderr, events)
            headless: If True, run without visualization
            max_turns: Optional cap on game turns (default: unlimited)
        """
        self.rat_script = rat_script
        self.python_script = python_script

        self.game = PyRat(
            width=width,
            height=height,
            cheese_count=cheese_count,
            seed=seed,
            max_turns=max_turns,
        )

        # Optional logger: create a timestamped subdirectory under log_dir
        self.logger: Optional[GameLogger] = None
        if log_dir:
            ts = datetime.now().strftime("%Y%m%d_%H%M%S")
            # Always nest logs under a timestamp to avoid collisions across runs
            actual_dir = os.path.join(log_dir, ts)
            self.logger = GameLogger(actual_dir)

        # Create move providers (using subprocess implementation)
        self.rat_provider: MoveProvider = SubprocessMoveProvider(
            rat_script, "rat", timeout=turn_timeout, logger=self.logger
        )
        self.python_provider: MoveProvider = SubprocessMoveProvider(
            python_script, "python", timeout=turn_timeout, logger=self.logger
        )

        # Display (optional for headless mode)
        self.display: Optional[Display] = (
            None if headless else Display(self.game, delay=display_delay)
        )

        # Configuration
        self.turn_timeout = turn_timeout
        self.preprocessing_timeout = preprocessing_timeout
        self.display_delay = display_delay
        self.headless = headless

    def _start_ai_processes(self) -> bool:
        """Start both AI move providers and display their information.

        Returns:
            True if both providers started successfully, False otherwise
        """
        print("Starting AI processes...")
        if self.logger:
            self.logger.event("Starting AI processes")

        if not self.rat_provider.start():
            print(f"Failed to start Rat AI: {self.rat_script}", file=sys.stderr)
            if self.logger:
                self.logger.event("Failed to start Rat AI")
            return False

        if not self.python_provider.start():
            print(f"Failed to start Python AI: {self.python_script}", file=sys.stderr)
            # Ensure we stop the rat provider if python fails to start
            self.rat_provider.stop()
            if self.logger:
                self.logger.event("Failed to start Python AI")
            return False

        # Display AI information
        print(f"Rat AI: {self.rat_provider.info.name}")
        if self.rat_provider.info.author:
            print(f"  Author: {self.rat_provider.info.author}")

        print(f"Python AI: {self.python_provider.info.name}")
        if self.python_provider.info.author:
            print(f"  Author: {self.python_provider.info.author}")

        print()
        return True

    def _initialize_game(self) -> None:
        """Initialize the game by sending game state to providers and showing initial display."""
        print("Initializing game...")
        if self.logger:
            self.logger.event("Initializing game")
        time.sleep(1)

        # Send game start to both providers
        self.rat_provider.send_game_start(self.game, self.preprocessing_timeout)
        self.python_provider.send_game_start(self.game, self.preprocessing_timeout)

        # Show initial state (if not headless)
        if self.display:
            self.display.render()
            time.sleep(self.display_delay)

    def _finalize_game(
        self, winner: str, rat_score: float, python_score: float
    ) -> None:
        """Finalize the game by notifying providers and displaying results.

        Args:
            winner: "rat", "python", or "draw"
            rat_score: Rat's final score
            python_score: Python's final score
        """
        # Send game over to providers
        self.rat_provider.send_game_over(winner, rat_score, python_score)
        self.python_provider.send_game_over(winner, rat_score, python_score)
        if self.logger:
            self.logger.event(
                f"Game over: winner={winner} score={rat_score}-{python_score}"
            )

        # Display final result (if not headless)
        if self.display:
            self.display.show_winner(winner, rat_score, python_score)

        # Stop providers
        self.rat_provider.stop()
        self.python_provider.stop()
        if self.logger:
            self.logger.close()

    def run(self) -> bool:
        """
        Run the game.

        Returns:
            True if successful, False if errors occurred
        """
        # Start move providers
        if not self._start_ai_processes():
            return False

        # Initialize game
        self._initialize_game()

        # Execute game loop using decoupled run_game function
        success, winner, rat_score, python_score = run_game(
            self.game,
            self.rat_provider,
            self.python_provider,
            self.display,
            self.display_delay,
        )

        # Finalize game
        self._finalize_game(winner, rat_score, python_score)

        return success
