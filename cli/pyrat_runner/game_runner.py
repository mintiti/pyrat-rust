"""Game orchestration and execution."""

import sys
import time
from typing import Optional, Tuple

from pyrat_engine import PyRat
from pyrat_engine.game import Direction

from .ai_process import AIProcess
from .display import Display


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
            width=width,
            height=height,
            cheese_count=cheese_count,
            seed=seed
        )

        # Create AI processes
        self.rat_ai = AIProcess(rat_script, "rat", timeout=turn_timeout)
        self.python_ai = AIProcess(python_script, "python", timeout=turn_timeout)

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

        if not self.rat_ai.start():
            print(f"Failed to start Rat AI: {self.rat_script}", file=sys.stderr)
            return False

        if not self.python_ai.start():
            print(f"Failed to start Python AI: {self.python_script}", file=sys.stderr)
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
        time.sleep(1)

        # Send game start to both AIs
        self.rat_ai.send_game_start(self.game, self.preprocessing_timeout)
        self.python_ai.send_game_start(self.game, self.preprocessing_timeout)

        # Show initial state
        self.display.render()
        time.sleep(self.display_delay)

    def _handle_ai_move_error(self, player: str, ai: AIProcess, move: Optional[Direction]) -> Tuple[bool, Direction]:
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
        if move is None:
            if not ai.is_alive():
                self.display.show_error(player, "AI process crashed")
                return False, Direction.STAY
            else:
                self.display.show_error(player, "AI timed out, defaulting to STAY")
                return True, Direction.STAY
        return True, move

    def _get_ai_moves(
        self,
        rat_prev_move: Direction,
        python_prev_move: Direction
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
        should_continue, rat_move = self._handle_ai_move_error("rat", self.rat_ai, rat_move)
        if not should_continue:
            return False, None, None

        # Handle python AI errors
        should_continue, python_move = self._handle_ai_move_error("python", self.python_ai, python_move)
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
            success, rat_move_new, python_move_new = self._get_ai_moves(rat_move, python_move)
            if not success:
                return False

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

        if rat_score > python_score:
            winner = "rat"
        elif python_score > rat_score:
            winner = "python"
        else:
            winner = "draw"

        return winner, rat_score, python_score

    def _finalize_game(self, winner: str, rat_score: float, python_score: float) -> None:
        """Finalize the game by notifying AIs and displaying results.

        Args:
            winner: "rat", "python", or "draw"
            rat_score: Rat's final score
            python_score: Python's final score
        """
        # Send game over to AIs
        self.rat_ai.send_game_over(winner, rat_score, python_score)
        self.python_ai.send_game_over(winner, rat_score, python_score)

        # Display final result
        self.display.show_winner(winner, rat_score, python_score)

        # Stop AI processes
        self.rat_ai.stop()
        self.python_ai.stop()

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
