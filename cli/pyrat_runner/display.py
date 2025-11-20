"""Terminal display for PyRat games with enhanced visualization."""

import os
import sys
from dataclasses import dataclass
from typing import Dict, FrozenSet, List, Optional, Set, Tuple

from pyrat_engine.core import Direction
from pyrat_engine.core.types import direction_to_name


@dataclass(frozen=True)
class MazeStructures:
    """Immutable structure holding pre-computed wall and mud positions."""

    h_walls: FrozenSet[Tuple[int, int]]  # Horizontal walls (between rows)
    v_walls: FrozenSet[Tuple[int, int]]  # Vertical walls (between columns)
    h_mud: FrozenSet[Tuple[int, int]]  # Horizontal mud (between rows)
    v_mud: FrozenSet[Tuple[int, int]]  # Vertical mud (between columns)


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


# ===========================
# Pure rendering functions
# ===========================


def build_maze_structures(
    walls: List[Tuple[Tuple[int, int], Tuple[int, int]]],
    mud: Dict[Tuple[Tuple[int, int], Tuple[int, int]], int],
) -> MazeStructures:
    """Build wall and mud lookup structures from raw data.

    This is a pure function - no side effects, same inputs always produce same outputs.

    Args:
        walls: List of wall entries, each is ((x1, y1), (x2, y2))
        mud: Dict mapping cell pairs to mud turns

    Returns:
        MazeStructures containing sets for efficient lookup
    """
    h_walls = set()
    v_walls = set()
    h_mud = set()
    v_mud = set()

    # Process walls
    for (x1, y1), (x2, y2) in walls:
        if x1 == x2:  # Same column, different row (horizontal wall)
            min_y = min(y1, y2)
            h_walls.add((x1, min_y))
        else:  # Same row, different column (vertical wall)
            min_x = min(x1, x2)
            v_walls.add((min_x, y1))

    # Process mud
    for (cell1, cell2), turns in mud.items():
        x1, y1 = cell1[0], cell1[1]
        x2, y2 = cell2[0], cell2[1]
        if x1 == x2:  # Same column, different row (horizontal mud)
            min_y = min(y1, y2)
            h_mud.add((x1, min_y))
        else:  # Same row, different column (vertical mud)
            min_x = min(x1, x2)
            v_mud.add((min_x, y1))

    return MazeStructures(
        h_walls=frozenset(h_walls),
        v_walls=frozenset(v_walls),
        h_mud=frozenset(h_mud),
        v_mud=frozenset(v_mud),
    )


def get_cell_content(
    x: int,
    y: int,
    rat_pos: Tuple[int, int],
    python_pos: Tuple[int, int],
    cheese_set: Set[Tuple[int, int]],
) -> str:
    """Get the display content for a specific cell.

    Pure function - no dependencies on external state.

    Args:
        x: Cell x coordinate
        y: Cell y coordinate
        rat_pos: Rat position as (x, y)
        python_pos: Python position as (x, y)
        cheese_set: Set of cheese positions

    Returns:
        String representation of cell content with ANSI colors
    """
    at_rat = rat_pos[0] == x and rat_pos[1] == y
    at_python = python_pos[0] == x and python_pos[1] == y
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


def get_vertical_separator(x: int, y: int, structures: MazeStructures) -> str:
    """Get the vertical separator (wall/mud/nothing) at a position.

    Pure function.

    Args:
        x: X coordinate
        y: Y coordinate
        structures: Pre-computed maze structures

    Returns:
        Separator character (wall, mud, or space)
    """
    if (x, y) in structures.v_walls:
        return VERTICAL_WALL
    elif (x, y) in structures.v_mud:
        return VERTICAL_MUD
    else:
        return VERTICAL_NOTHING


def get_horizontal_separator(x: int, y: int, structures: MazeStructures) -> str:
    """Get the horizontal separator (wall/mud/nothing) at a position.

    Pure function.

    Args:
        x: X coordinate
        y: Y coordinate
        structures: Pre-computed maze structures

    Returns:
        Separator string (wall, mud, or spaces)
    """
    if (x, y) in structures.h_walls:
        return HORIZONTAL_WALL
    elif (x, y) in structures.h_mud:
        return HORIZONTAL_MUD
    else:
        return HORIZONTAL_NOTHING


def render_board(
    width: int,
    height: int,
    rat_pos: Tuple[int, int],
    python_pos: Tuple[int, int],
    cheese_set: Set[Tuple[int, int]],
    structures: MazeStructures,
) -> str:
    """Render the game board as a string.

    Pure function - returns board visualization without side effects.

    Args:
        width: Board width
        height: Board height
        rat_pos: Rat position as (x, y)
        python_pos: Python position as (x, y)
        cheese_set: Set of cheese positions
        structures: Pre-computed maze structures

    Returns:
        Multi-line string containing the board visualization
    """
    lines = []

    # X-axis coordinate labels (top)
    x_labels = EMPTY
    for x in range(width):
        x_labels += WALL_INTERSECTION + str(x).center(ELEMENT_WIDTH)
    x_labels += WALL_INTERSECTION
    lines.append(x_labels)

    # Top border
    horizontal_border = EMPTY + WALL_INTERSECTION
    for x in range(width):
        horizontal_border += HORIZONTAL_WALL + WALL_INTERSECTION
    lines.append(horizontal_border)

    # Render maze from top to bottom (high y to low y)
    for y in range(height - 1, -1, -1):
        # Cell contents row
        cells_row = str(y).center(ELEMENT_WIDTH) + VERTICAL_WALL
        for x in range(width):
            cells_row += get_cell_content(x, y, rat_pos, python_pos, cheese_set)
            if x < width - 1:
                cells_row += get_vertical_separator(x, y, structures)
            else:
                cells_row += VERTICAL_WALL
        lines.append(cells_row)

        # Horizontal separators row (except after last row)
        if y > 0:
            sep_row = EMPTY + WALL_INTERSECTION
            for x in range(width):
                sep_row += get_horizontal_separator(x, y - 1, structures)
                sep_row += WALL_INTERSECTION
            lines.append(sep_row)

    # Bottom border
    lines.append(horizontal_border)

    return "\n".join(lines)


def render_header(
    rat_pos: Tuple[int, int],
    rat_score: float,
    rat_move: Optional[Direction],
    python_pos: Tuple[int, int],
    python_score: float,
    python_move: Optional[Direction],
    turn: int,
) -> str:
    """Render the game header with player information.

    Pure function - returns header string without side effects.

    Args:
        rat_pos: Rat position as (x, y)
        rat_score: Rat's current score
        rat_move: Rat's last move (or None)
        python_pos: Python position as (x, y)
        python_score: Python's current score
        python_move: Python's last move (or None)
        turn: Current turn number

    Returns:
        Multi-line string containing the header
    """
    lines = []
    lines.append(
        f"\n{COLOR_RED}╔═══════════════════════════════════════════════════════════╗{COLOR_RESET}"
    )
    lines.append(
        f"{COLOR_RED}║{COLOR_RESET}                    PyRat Game Viewer                    {COLOR_RED}║{COLOR_RESET}"
    )
    lines.append(
        f"{COLOR_RED}╚═══════════════════════════════════════════════════════════╝{COLOR_RESET}\n"
    )

    # Player 1 (Rat) info
    lines.append(f"{COLOR_RED}Player 1 (Rat):{COLOR_RESET}")
    lines.append(f"    Position : ({rat_pos[0]}, {rat_pos[1]})")
    lines.append(f"    Score    : {rat_score:.1f}")
    if rat_move is not None:
        lines.append(f"    Last move: {direction_to_name(rat_move)}")
    lines.append("")

    # Player 2 (Python) info
    lines.append(f"{COLOR_GREEN}Player 2 (Python):{COLOR_RESET}")
    lines.append(f"    Position : ({python_pos[0]}, {python_pos[1]})")
    lines.append(f"    Score    : {python_score:.1f}")
    if python_move is not None:
        lines.append(f"    Last move: {direction_to_name(python_move)}")
    lines.append("")

    lines.append(f"Turn: {turn}")
    lines.append("")

    return "\n".join(lines)


def render_game_state(
    game,
    structures: MazeStructures,
    rat_move: Optional[Direction] = None,
    python_move: Optional[Direction] = None,
) -> str:
    """Render the complete game state as a string.

    Pure function - returns complete game visualization without side effects.
    Reads from game state but does not modify it.

    Args:
        game: PyRat game instance (read-only)
        structures: Pre-computed maze structures
        rat_move: Rat's last move (or None)
        python_move: Python's last move (or None)

    Returns:
        Multi-line string containing the complete game visualization
    """
    # Extract data from game state (read-only operations)
    rat_pos = (game.player1_pos[0], game.player1_pos[1])
    python_pos = (game.player2_pos[0], game.player2_pos[1])
    scores = game.scores
    cheese_set = set((c[0], c[1]) for c in game.cheese_positions)
    width = game._game.width
    height = game._game.height
    turn = game.turn

    header = render_header(
        rat_pos=rat_pos,
        rat_score=scores[0],
        rat_move=rat_move,
        python_pos=python_pos,
        python_score=scores[1],
        python_move=python_move,
        turn=turn,
    )

    board = render_board(
        width=width,
        height=height,
        rat_pos=rat_pos,
        python_pos=python_pos,
        cheese_set=cheese_set,
        structures=structures,
    )

    return header + board + "\n"


def render_winner_screen(winner: str, rat_score: float, python_score: float) -> str:
    """Render the game over screen.

    Pure function - returns winner screen string without side effects.

    Args:
        winner: "rat", "python", or "draw"
        rat_score: Final rat score
        python_score: Final python score

    Returns:
        Multi-line string containing the winner screen
    """
    lines = []
    lines.append("")
    lines.append(
        f"{COLOR_YELLOW}╔═══════════════════════════════════════════════════════════╗{COLOR_RESET}"
    )
    lines.append(
        f"{COLOR_YELLOW}║{COLOR_RESET}                        GAME OVER                          {COLOR_YELLOW}║{COLOR_RESET}"
    )
    lines.append(
        f"{COLOR_YELLOW}╚═══════════════════════════════════════════════════════════╝{COLOR_RESET}"
    )
    lines.append("")

    lines.append(f"{COLOR_RED}Rat (Player 1):{COLOR_RESET}    {rat_score:.1f} points")
    lines.append(
        f"{COLOR_GREEN}Python (Player 2):{COLOR_RESET} {python_score:.1f} points"
    )
    lines.append("")

    if winner == "draw":
        lines.append(f"Result: {COLOR_YELLOW}DRAW{COLOR_RESET}")
    elif winner == "rat":
        lines.append(f"Winner: {COLOR_RED}RAT (Player 1){COLOR_RESET} 🎉")
    elif winner == "python":
        lines.append(f"Winner: {COLOR_GREEN}PYTHON (Player 2){COLOR_RESET} 🎉")

    lines.append("")

    return "\n".join(lines)


def render_error_message(player: str, error: str) -> str:
    """Render an error message.

    Pure function - returns error message string without side effects.

    Args:
        player: "rat" or "python"
        error: Error description

    Returns:
        Error message string with color coding
    """
    color = COLOR_RED if player == "rat" else COLOR_GREEN
    return f"\n{color}ERROR ({player}):{COLOR_RESET} {error}\n"


class Display:
    """Terminal-based game visualization - thin wrapper around pure rendering functions.

    This class maintains minimal state and delegates all rendering to pure functions,
    making the rendering logic testable and reusable.
    """

    def __init__(self, game_state, delay: float = 0.5):
        """
        Initialize display.

        Args:
            game_state: PyRat instance
            delay: Delay in seconds between turns
        """
        self.game = game_state
        self.delay = delay
        # Pre-compute immutable maze structures once
        walls = game_state._game.wall_entries()
        mud = game_state.mud_positions
        self.structures = build_maze_structures(walls, mud)
        # Detect non-interactive environments to limit rendering in CI
        try:
            self._non_tty = not sys.stdout.isatty()  # type: ignore[attr-defined]
        except Exception:
            self._non_tty = True
        self._printed_once = False

    @staticmethod
    def clear():
        """Clear the terminal screen when in a TTY.

        Skip in non-interactive environments to avoid TERM errors and overhead.
        """
        try:
            if not sys.stdout.isatty():  # type: ignore[attr-defined]
                return
        except Exception:
            return
        os.system("cls" if os.name == "nt" else "clear")

    # Expose pure functions as instance methods for backward compatibility with tests
    def _get_cell_content(
        self, x: int, y: int, cheese_set: Set[Tuple[int, int]]
    ) -> str:
        """Get display content for a cell (delegates to pure function)."""
        rat_pos = (self.game.player1_pos[0], self.game.player1_pos[1])
        python_pos = (self.game.player2_pos[0], self.game.player2_pos[1])
        return get_cell_content(x, y, rat_pos, python_pos, cheese_set)

    def _get_vertical_separator(self, x: int, y: int) -> str:
        """Get vertical separator (delegates to pure function)."""
        return get_vertical_separator(x, y, self.structures)

    def _get_horizontal_separator(self, x: int, y: int) -> str:
        """Get horizontal separator (delegates to pure function)."""
        return get_horizontal_separator(x, y, self.structures)

    # For backward compatibility with tests, expose structure sets as properties
    @property
    def h_walls(self) -> FrozenSet[Tuple[int, int]]:
        """Horizontal walls set."""
        return self.structures.h_walls

    @property
    def v_walls(self) -> FrozenSet[Tuple[int, int]]:
        """Vertical walls set."""
        return self.structures.v_walls

    @property
    def h_mud(self) -> FrozenSet[Tuple[int, int]]:
        """Horizontal mud set."""
        return self.structures.h_mud

    @property
    def v_mud(self) -> FrozenSet[Tuple[int, int]]:
        """Vertical mud set."""
        return self.structures.v_mud

    def render(
        self,
        rat_move: Optional[Direction] = None,
        python_move: Optional[Direction] = None,
    ):
        """
        Render the current game state with enhanced visualization.

        Thin wrapper that delegates to pure render_game_state() function.
        Handles side effects (clear screen, print).

        Args:
            rat_move: Last move made by rat
            python_move: Last move made by python
        """
        # Avoid excessive rendering in non-interactive environments
        if self._non_tty and self._printed_once:
            return
        self.clear()

        # Use pure function to generate complete output
        output = render_game_state(
            game=self.game,
            structures=self.structures,
            rat_move=rat_move,
            python_move=python_move,
        )

        # Handle side effect (printing)
        print(output, end="")
        self._printed_once = True

    def show_winner(self, winner: str, rat_score: float, python_score: float):
        """
        Display game over message with enhanced styling.

        Args:
            winner: "rat", "python", or "draw"
            rat_score: Final rat score
            python_score: Final python score
        """
        output = render_winner_screen(winner, rat_score, python_score)
        print(output)

    def show_error(self, player: str, error: str):
        """
        Display an error message with color coding.

        Args:
            player: "rat" or "python"
            error: Error description
        """
        output = render_error_message(player, error)
        print(output, file=sys.stderr)
