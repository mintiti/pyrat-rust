"""Terminal display for PyRat games."""

import os
import sys
from typing import Set, Tuple

from pyrat_engine.game import Direction


# Direction name mapping
DIRECTION_NAMES = {
    0: "UP",
    1: "DOWN",
    2: "LEFT",
    3: "RIGHT",
    4: "STAY"
}


def get_direction_name(direction: Direction) -> str:
    """Get the string name of a Direction."""
    if direction is None:
        return "NONE"
    return DIRECTION_NAMES.get(int(direction), "STAY")


class Display:
    """Terminal-based game visualization."""

    def __init__(self, game_state, delay: float = 0.5):
        """
        Initialize display.

        Args:
            game_state: PyRat instance
            delay: Delay in seconds between turns
        """
        self.game = game_state
        self.delay = delay
        self.width = game_state._game.width
        self.height = game_state._game.height

    def clear(self):
        """Clear the terminal screen."""
        os.system('cls' if os.name == 'nt' else 'clear')

    def render(self, rat_move: Direction = None, python_move: Direction = None):
        """
        Render the current game state.

        Args:
            rat_move: Last move made by rat
            python_move: Last move made by python
        """
        self.clear()

        # Get game state
        rat_pos = self.game.player1_pos
        python_pos = self.game.player2_pos
        # Convert cheese Coordinates to tuples for set lookup
        cheese_set = set((c[0], c[1]) for c in self.game.cheese_positions)
        scores = self.game.scores

        # Build wall sets for easy lookup
        # walls are stored as ((x1, y1), (x2, y2)) where cells are adjacent
        walls = self.game._game.wall_entries()
        h_walls = set()  # Horizontal walls (between rows)
        v_walls = set()  # Vertical walls (between columns)

        for ((x1, y1), (x2, y2)) in walls:
            if x1 == x2:  # Same column, different row (horizontal wall)
                min_y = min(y1, y2)
                h_walls.add((x1, min_y))
            else:  # Same row, different column (vertical wall)
                min_x = min(x1, x2)
                v_walls.add((min_x, y1))

        # Print header
        print("=" * (self.width * 4 + 1))
        print(f"PyRat Game - Turn {self.game.turn}")
        print(f"Rat (R): {scores[0]:.1f}  |  Python (P): {scores[1]:.1f}")
        if rat_move is not None and python_move is not None:
            rat_move_str = get_direction_name(rat_move)
            python_move_str = get_direction_name(python_move)
            print(f"Last moves - Rat: {rat_move_str:6s}  Python: {python_move_str:6s}")
        print("=" * (self.width * 4 + 1))
        print()

        # Render board from top to bottom
        # Each cell is represented as:
        #   +---+
        #   | X |
        # Where + are corners, - are horizontal walls, | are vertical walls

        for y in range(self.height - 1, -1, -1):  # Top to bottom
            # Draw top edge of row
            row_str = ""
            for x in range(self.width):
                row_str += "+"
                # Check if there's a horizontal wall above this cell
                if (x, y) in h_walls:
                    row_str += "---"
                else:
                    row_str += "   "
            row_str += "+"
            print(row_str)

            # Draw cell contents
            row_str = ""
            for x in range(self.width):
                # Check if there's a vertical wall to the left of this cell
                if (x, y) in v_walls:
                    row_str += "|"
                else:
                    row_str += " "

                # Cell content - compare using x,y coordinates
                at_rat = (rat_pos[0] == x and rat_pos[1] == y)
                at_python = (python_pos[0] == x and python_pos[1] == y)
                at_cheese = (x, y) in cheese_set

                if at_rat and at_python:
                    cell = "R+P"  # Both players on same cell
                elif at_rat:
                    cell = " R "
                elif at_python:
                    cell = " P "
                elif at_cheese:
                    cell = " * "
                else:
                    cell = "   "

                row_str += cell
            # Right edge
            row_str += "|" if (self.width - 1, y) in v_walls else " "
            print(row_str)

        # Draw bottom edge
        row_str = ""
        for x in range(self.width):
            row_str += "+"
            row_str += "---"
        row_str += "+"
        print(row_str)

    def show_winner(self, winner: str, rat_score: float, python_score: float):
        """
        Display game over message.

        Args:
            winner: "rat", "python", or "draw"
            rat_score: Final rat score
            python_score: Final python score
        """
        print()
        print("=" * (self.width * 4 + 1))
        print("GAME OVER")
        print("=" * (self.width * 4 + 1))
        print()
        print(f"Final Score - Rat: {rat_score:.1f}  Python: {python_score:.1f}")
        print()

        if winner == "draw":
            print("Result: DRAW")
        elif winner == "rat":
            print("Winner: RAT")
        elif winner == "python":
            print("Winner: PYTHON")

        print()

    def show_error(self, player: str, error: str):
        """
        Display an error message.

        Args:
            player: "rat" or "python"
            error: Error description
        """
        print()
        print(f"ERROR ({player}): {error}", file=sys.stderr)
        print()
