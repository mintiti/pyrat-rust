"""Terminal display for PyRat games with enhanced visualization."""

import os
import sys
from typing import Dict, Set, Tuple

from pyrat_engine.game import Direction


# Direction name mapping
DIRECTION_NAMES = {
    0: "UP",
    1: "RIGHT",
    2: "DOWN",
    3: "LEFT",
    4: "STAY"
}


def get_direction_name(direction: Direction) -> str:
    """Get the string name of a Direction."""
    if direction is None:
        return "NONE"
    return DIRECTION_NAMES.get(int(direction), "STAY")


# Display configuration
ELEMENT_WIDTH = 5

# ANSI color codes
COLOR_RED = "\033[1;31m"
COLOR_GREEN = "\033[1;32m"
COLOR_YELLOW = "\033[1;33m"
COLOR_RESET = "\033[0m"

# Cell assets
RAT = COLOR_RED + "R".center(ELEMENT_WIDTH) + COLOR_RESET
PYTHON = COLOR_GREEN + "P".center(ELEMENT_WIDTH) + COLOR_RESET
RAT_AND_PYTHON = COLOR_RED + "RP".center(ELEMENT_WIDTH) + COLOR_RESET
CHEESE = COLOR_YELLOW + "C".center(ELEMENT_WIDTH) + COLOR_RESET
RAT_AND_CHEESE = COLOR_RED + "RC".center(ELEMENT_WIDTH) + COLOR_RESET
PYTHON_AND_CHEESE = COLOR_GREEN + "PC".center(ELEMENT_WIDTH) + COLOR_RESET
RAT_AND_PYTHON_AND_CHEESE = COLOR_RED + "RPC".center(ELEMENT_WIDTH) + COLOR_RESET
EMPTY = "".center(ELEMENT_WIDTH)

# Wall and mud rendering
VERTICAL_WALL = "│"
VERTICAL_MUD = "┊"
VERTICAL_NOTHING = " "
HORIZONTAL_WALL = "─" * ELEMENT_WIDTH
HORIZONTAL_MUD = "┈" * ELEMENT_WIDTH
HORIZONTAL_NOTHING = " " * ELEMENT_WIDTH
WALL_INTERSECTION = "┼"
CORNER = "+"


class Display:
    """Terminal-based game visualization with enhanced rendering."""

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
        self._build_maze_structures()

    def clear(self):
        """Clear the terminal screen."""
        os.system('cls' if os.name == 'nt' else 'clear')

    def _build_maze_structures(self):
        """Build wall and mud lookup structures."""
        # Get walls and mud from game
        walls = self.game._game.wall_entries()
        mud = self.game.mud_positions

        # Build horizontal structures (between rows)
        self.h_walls = set()
        self.h_mud = set()

        # Build vertical structures (between columns)
        self.v_walls = set()
        self.v_mud = set()

        # Process walls
        for ((x1, y1), (x2, y2)) in walls:
            if x1 == x2:  # Same column, different row (horizontal wall)
                min_y = min(y1, y2)
                self.h_walls.add((x1, min_y))
            else:  # Same row, different column (vertical wall)
                min_x = min(x1, x2)
                self.v_walls.add((min_x, y1))

        # Process mud
        for (cell1, cell2), turns in mud.items():
            x1, y1 = cell1[0], cell1[1]
            x2, y2 = cell2[0], cell2[1]
            if x1 == x2:  # Same column, different row (horizontal mud)
                min_y = min(y1, y2)
                self.h_mud.add((x1, min_y))
            else:  # Same row, different column (vertical mud)
                min_x = min(x1, x2)
                self.v_mud.add((min_x, y1))

    def _get_cell_content(self, x: int, y: int, cheese_set: Set[Tuple[int, int]]) -> str:
        """Get the display content for a specific cell."""
        rat_pos = self.game.player1_pos
        python_pos = self.game.player2_pos

        at_rat = (rat_pos[0] == x and rat_pos[1] == y)
        at_python = (python_pos[0] == x and python_pos[1] == y)
        at_cheese = (x, y) in cheese_set

        # Determine cell content based on occupancy
        if at_rat and at_python and at_cheese:
            return RAT_AND_PYTHON_AND_CHEESE
        elif at_rat and at_python:
            return RAT_AND_PYTHON
        elif at_rat and at_cheese:
            return RAT_AND_CHEESE
        elif at_python and at_cheese:
            return PYTHON_AND_CHEESE
        elif at_rat:
            return RAT
        elif at_python:
            return PYTHON
        elif at_cheese:
            return CHEESE
        else:
            return EMPTY

    def _get_vertical_separator(self, x: int, y: int) -> str:
        """Get the vertical separator (wall/mud/nothing) to the left of a cell."""
        if (x, y) in self.v_walls:
            return VERTICAL_WALL
        elif (x, y) in self.v_mud:
            return VERTICAL_MUD
        else:
            return VERTICAL_NOTHING

    def _get_horizontal_separator(self, x: int, y: int) -> str:
        """Get the horizontal separator (wall/mud/nothing) above a cell."""
        if (x, y) in self.h_walls:
            return HORIZONTAL_WALL
        elif (x, y) in self.h_mud:
            return HORIZONTAL_MUD
        else:
            return HORIZONTAL_NOTHING

    def render(self, rat_move: Direction = None, python_move: Direction = None):
        """
        Render the current game state with enhanced visualization.

        Args:
            rat_move: Last move made by rat
            python_move: Last move made by python
        """
        self.clear()

        # Get game state
        cheese_set = set((c[0], c[1]) for c in self.game.cheese_positions)
        scores = self.game.scores

        # Print header with player information
        print(f"\n{COLOR_RED}╔═══════════════════════════════════════════════════════════╗{COLOR_RESET}")
        print(f"{COLOR_RED}║{COLOR_RESET}                    PyRat Game Viewer                    {COLOR_RED}║{COLOR_RESET}")
        print(f"{COLOR_RED}╚═══════════════════════════════════════════════════════════╝{COLOR_RESET}\n")

        # Player 1 (Rat) info
        rat_pos = self.game.player1_pos
        # Note: mud status would require accessing player mud turns from game state
        print(f"{COLOR_RED}Player 1 (Rat):{COLOR_RESET}")
        print(f"    Position : ({rat_pos[0]}, {rat_pos[1]})")
        print(f"    Score    : {scores[0]:.1f}")
        if rat_move is not None:
            print(f"    Last move: {get_direction_name(rat_move)}")
        print()

        # Player 2 (Python) info
        python_pos = self.game.player2_pos
        print(f"{COLOR_GREEN}Player 2 (Python):{COLOR_RESET}")
        print(f"    Position : ({python_pos[0]}, {python_pos[1]})")
        print(f"    Score    : {scores[1]:.1f}")
        if python_move is not None:
            print(f"    Last move: {get_direction_name(python_move)}")
        print()

        print(f"Turn: {self.game.turn}")
        print()

        # Build the maze visualization
        # X-axis coordinate labels (top)
        x_labels = EMPTY
        for x in range(self.width):
            x_labels += WALL_INTERSECTION + str(x).center(ELEMENT_WIDTH)
        x_labels += WALL_INTERSECTION
        print(x_labels)

        # Top border
        horizontal_border = EMPTY + WALL_INTERSECTION
        for x in range(self.width):
            horizontal_border += HORIZONTAL_WALL + WALL_INTERSECTION
        print(horizontal_border)

        # Render maze from top to bottom (high y to low y)
        for y in range(self.height - 1, -1, -1):
            # Cell contents row
            cells_row = str(y).center(ELEMENT_WIDTH) + VERTICAL_WALL
            for x in range(self.width):
                cells_row += self._get_cell_content(x, y, cheese_set)
                if x < self.width - 1:
                    cells_row += self._get_vertical_separator(x + 1, y)
                else:
                    cells_row += VERTICAL_WALL
            print(cells_row)

            # Horizontal separators row (except after last row)
            if y > 0:
                sep_row = EMPTY + WALL_INTERSECTION
                for x in range(self.width):
                    sep_row += self._get_horizontal_separator(x, y - 1)
                    sep_row += WALL_INTERSECTION
                print(sep_row)

        # Bottom border
        print(horizontal_border)
        print()

    def show_winner(self, winner: str, rat_score: float, python_score: float):
        """
        Display game over message with enhanced styling.

        Args:
            winner: "rat", "python", or "draw"
            rat_score: Final rat score
            python_score: Final python score
        """
        print()
        print(f"{COLOR_YELLOW}╔═══════════════════════════════════════════════════════════╗{COLOR_RESET}")
        print(f"{COLOR_YELLOW}║{COLOR_RESET}                        GAME OVER                          {COLOR_YELLOW}║{COLOR_RESET}")
        print(f"{COLOR_YELLOW}╚═══════════════════════════════════════════════════════════╝{COLOR_RESET}")
        print()

        print(f"{COLOR_RED}Rat (Player 1):{COLOR_RESET}    {rat_score:.1f} points")
        print(f"{COLOR_GREEN}Python (Player 2):{COLOR_RESET} {python_score:.1f} points")
        print()

        if winner == "draw":
            print(f"Result: {COLOR_YELLOW}DRAW{COLOR_RESET}")
        elif winner == "rat":
            print(f"Winner: {COLOR_RED}RAT (Player 1){COLOR_RESET} 🎉")
        elif winner == "python":
            print(f"Winner: {COLOR_GREEN}PYTHON (Player 2){COLOR_RESET} 🎉")

        print()

    def show_error(self, player: str, error: str):
        """
        Display an error message with color coding.

        Args:
            player: "rat" or "python"
            error: Error description
        """
        color = COLOR_RED if player == "rat" else COLOR_GREEN
        print()
        print(f"{color}ERROR ({player}):{COLOR_RESET} {error}", file=sys.stderr)
        print()
