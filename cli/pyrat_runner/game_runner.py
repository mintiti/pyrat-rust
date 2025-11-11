"""Game orchestration and execution."""

import sys
import time
from typing import Optional

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

    def run(self) -> bool:
        """
        Run the game.

        Returns:
            True if successful, False if errors occurred
        """
        # Start AI processes
        print("Starting AI processes...")
        if not self.rat_ai.start():
            print(f"Failed to start Rat AI: {self.rat_script}", file=sys.stderr)
            return False

        if not self.python_ai.start():
            print(f"Failed to start Python AI: {self.python_script}", file=sys.stderr)
            self.rat_ai.stop()
            return False

        print(f"Rat AI: {self.rat_ai.info.name}")
        if self.rat_ai.info.author:
            print(f"  Author: {self.rat_ai.info.author}")

        print(f"Python AI: {self.python_ai.info.name}")
        if self.python_ai.info.author:
            print(f"  Author: {self.python_ai.info.author}")

        print()
        print("Initializing game...")
        time.sleep(1)

        # Send game start to both AIs
        self.rat_ai.send_game_start(self.game, self.preprocessing_timeout)
        self.python_ai.send_game_start(self.game, self.preprocessing_timeout)

        # Initial display
        self.display.render()
        time.sleep(self.display_delay)

        # Game loop
        rat_move = Direction.STAY
        python_move = Direction.STAY
        game_over = False
        error_occurred = False

        while not game_over:
            # Get moves from both AIs
            rat_move_new = self.rat_ai.get_move(rat_move, python_move)
            python_move_new = self.python_ai.get_move(rat_move, python_move)

            # Handle timeouts/crashes
            if rat_move_new is None:
                if not self.rat_ai.is_alive():
                    self.display.show_error("rat", "AI process crashed")
                    error_occurred = True
                    break
                else:
                    self.display.show_error("rat", "AI timed out, defaulting to STAY")
                    rat_move_new = Direction.STAY

            if python_move_new is None:
                if not self.python_ai.is_alive():
                    self.display.show_error("python", "AI process crashed")
                    error_occurred = True
                    break
                else:
                    self.display.show_error("python", "AI timed out, defaulting to STAY")
                    python_move_new = Direction.STAY

            rat_move = rat_move_new
            python_move = python_move_new

            # Execute move in game
            result = self.game.step(p1_move=rat_move, p2_move=python_move)

            # Update display
            self.display.render(rat_move, python_move)
            time.sleep(self.display_delay)

            # Check for game over
            if result.game_over:
                game_over = True

        # Determine winner
        scores = self.game.scores
        rat_score = scores[0]
        python_score = scores[1]

        if rat_score > python_score:
            winner = "rat"
        elif python_score > rat_score:
            winner = "python"
        else:
            winner = "draw"

        # Send game over to AIs
        self.rat_ai.send_game_over(winner, rat_score, python_score)
        self.python_ai.send_game_over(winner, rat_score, python_score)

        # Display final result
        self.display.show_winner(winner, rat_score, python_score)

        # Stop AI processes
        self.rat_ai.stop()
        self.python_ai.stop()

        return not error_occurred
