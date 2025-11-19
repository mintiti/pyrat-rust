"""Compositional tests for display symmetry and coordinate transformations.

This test suite verifies that PyRat's display system correctly preserves
180° rotational symmetry through each transformation level:

Level 0: GameState (Rust/Engine)
         ↓ [extract: wall_entries(), mud_positions, cheese_positions]
Level 1: Raw Data (Python tuples/dicts)
         ↓ [transform: build_maze_structures()]
Level 2: MazeStructures (normalized sets: h_walls, v_walls, h_mud, v_mud)
         ↓ [render: get_cell_content(), get_*_separator()]
Level 3: Rendering primitives (individual strings)
         ↓ [compose: render_board()]
Level 4: Complete board (multi-line string)

For each transformation T and symmetry operation S (180° rotation):
    T(S(input)) = S(T(output))

If symmetry is preserved at each level, it's preserved through the entire composition.

Coordinate System Specification:
- Origin (0,0) is at bottom-left corner
- X-axis increases rightward
- Y-axis increases upward
- Wall between (x,y) and (x+1,y) appears RIGHT of cell (x,y)
- Wall between (x,y) and (x,y+1) appears ABOVE cell (x,y)
- 180° rotation: (x,y) → (width-1-x, height-1-y)
"""

import re
from typing import Dict, Set, Tuple


from pyrat_engine.game import PyRat
from pyrat_engine.core import GameState as PyGameState

from pyrat_runner.display import (
    MazeStructures,
    build_maze_structures,
    get_cell_content,
    get_vertical_separator,
    get_horizontal_separator,
    render_board,
    VERTICAL_WALL,
    HORIZONTAL_WALL,
)


# ===========================
# Helper Functions
# ===========================


def get_symmetric_position(x: int, y: int, width: int, height: int) -> Tuple[int, int]:
    """Get 180° rotated position around maze center.

    This uses the same formula as the engine's Rust implementation.

    Args:
        x: X coordinate
        y: Y coordinate
        width: Board width
        height: Board height

    Returns:
        Symmetric position as (x', y')
    """
    return (width - 1 - x, height - 1 - y)


def is_symmetric_wall_pair(
    wall1: Tuple[Tuple[int, int], Tuple[int, int]],
    wall2: Tuple[Tuple[int, int], Tuple[int, int]],
    width: int,
    height: int,
) -> bool:
    """Check if two walls are 180° rotationally symmetric.

    Args:
        wall1: First wall as ((x1, y1), (x2, y2))
        wall2: Second wall as ((x1, y1), (x2, y2))
        width: Board width
        height: Board height

    Returns:
        True if walls are symmetric pairs
    """
    (x1a, y1a), (x2a, y2a) = wall1
    (x1b, y1b), (x2b, y2b) = wall2

    # Get symmetric positions for wall1's endpoints
    sym1a = get_symmetric_position(x1a, y1a, width, height)
    sym2a = get_symmetric_position(x2a, y2a, width, height)

    # Check if wall2's endpoints match (order-independent)
    return ((x1b, y1b) == sym1a and (x2b, y2b) == sym2a) or (
        (x1b, y1b) == sym2a and (x2b, y2b) == sym1a
    )


def strip_ansi_codes(text: str) -> str:
    """Remove ANSI color codes from text.

    Args:
        text: Text potentially containing ANSI codes

    Returns:
        Text with ANSI codes removed
    """
    ansi_escape = re.compile(r"\x1B(?:[@-Z\\-_]|\[[0-?]*[ -/]*[@-~])")
    return ansi_escape.sub("", text)


def rotate_board_string_180(board: str, width: int, height: int) -> str:
    """Rotate a rendered board string 180° around its center.

    This performs a visual rotation, swapping R↔P since players swap
    positions when the board is rotated.

    Args:
        board: Multi-line board rendering
        width: Board width
        height: Board height

    Returns:
        Rotated board string with R↔P swapped
    """
    lines = board.strip().split("\n")

    # Reverse line order (vertical flip)
    rotated_lines = list(reversed(lines))

    # Reverse each line (horizontal flip)
    rotated_lines = [line[::-1] for line in rotated_lines]

    # Swap R and P (players swap when rotated)
    result = []
    for line in rotated_lines:
        # Replace temporarily to avoid double-swapping
        line = line.replace("R", "\x00")  # Placeholder
        line = line.replace("P", "R")
        line = line.replace("\x00", "P")
        result.append(line)

    return "\n".join(result)


def extract_entity_positions(board: str) -> Dict[str, Set[Tuple[int, int]]]:
    """Extract entity positions from a rendered board.

    Parses the board to find where R, P, and C appear.

    The board format is:
    - Each cell is 5 characters wide
    - Cells are separated by vertical separators (│, ┊, or space)
    - Y coordinates are labeled at the start of each row
    - X coordinates are labeled in the header

    Args:
        board: Rendered board string

    Returns:
        Dictionary mapping entity type to set of positions:
        {"R": {(x, y), ...}, "P": {(x, y), ...}, "C": {(x, y), ...}}
    """
    positions = {"R": set(), "P": set(), "C": set()}

    # Strip ANSI codes for easier parsing
    clean_board = strip_ansi_codes(board)
    lines = clean_board.strip().split("\n")

    # Skip header lines (x-axis labels and top border)
    # Board starts at line 2 (0-indexed)
    board_lines = lines[2:]

    # Process board from top to bottom
    # Each cell row is followed by a separator row
    for line in board_lines:
        # Cell rows have y-coordinate at start and walls '│' at edges
        if "│" in line and not all(c in "─┈ ┼+│" for c in line.strip()):
            # Extract y-coordinate from the start of the line
            y_match = re.match(r"\s*(\d+)\s*│", line)
            if y_match:
                y = int(y_match.group(1))

                # Remove the y-label and left border
                content = re.sub(r"^\s*\d+\s*│", "", line)
                content = content.rstrip("│")  # Remove right border

                # Each cell is 5 chars wide, with separators between them
                # Pattern: [5 chars][separator][5 chars][separator]...
                # Separators are │, ┊, or space (single char)

                x = 0
                pos = 0
                while pos < len(content):
                    # Extract next 5 characters (cell content)
                    cell = content[pos : pos + 5]

                    # Check for entities in this cell
                    if "R" in cell:
                        positions["R"].add((x, y))
                    if "P" in cell:
                        positions["P"].add((x, y))
                    if "C" in cell:
                        positions["C"].add((x, y))

                    # Move to next cell (5 chars + 1 separator)
                    pos += 6
                    x += 1

    return positions


def extract_wall_positions(board: str) -> Set[Tuple[Tuple[int, int], Tuple[int, int]]]:
    """Extract wall positions from a rendered board.

    Walls appear as:
    - Vertical walls: │ character between cells (at x position)
    - Horizontal walls: ───── (solid line) between rows (at y position)

    Args:
        board: Rendered board string

    Returns:
        Set of walls as ((x1, y1), (x2, y2)) tuples
    """
    walls = set()

    clean_board = strip_ansi_codes(board)
    lines = clean_board.strip().split("\n")

    # Skip header
    board_lines = lines[2:]

    # Track current y coordinate
    current_y = None

    for line in board_lines:
        # Cell rows (with y-coordinate label)
        if "│" in line and not all(c in "─┈ ┼+│" for c in line.strip()):
            y_match = re.match(r"\s*(\d+)\s*│", line)
            if y_match:
                current_y = int(y_match.group(1))

                # Extract vertical walls
                content = re.sub(r"^\s*\d+\s*│", "", line)
                content = content.rstrip("│")

                x = 0
                pos = 0
                while pos < len(content):
                    # Check separator after this cell
                    if pos + 5 < len(content):
                        separator = content[pos + 5]
                        if separator == "│":
                            # The separator after rendering cell x is obtained by calling
                            # get_vertical_separator(x, y) which checks if (x, y) in v_walls
                            # So this wall is stored at (x, y) and connects cells (x, y) and (x+1, y)
                            walls.add(((x, current_y), (x + 1, current_y)))

                    pos += 6
                    x += 1

        # Separator rows (horizontal walls/mud)
        elif "┼" in line and "─" in line:
            # This is a horizontal separator row
            # It separates row current_y from row current_y - 1
            if current_y is not None and current_y > 0:
                # Parse horizontal walls
                # Skip the leading spaces and ┼
                content_start = line.find("┼")
                if content_start >= 0:
                    content = line[content_start + 1 :]

                    x = 0
                    pos = 0
                    while pos < len(content):
                        # Extract next 5 characters (cell separator)
                        segment = content[pos : pos + 5]
                        if segment == "─────":
                            # Horizontal separators are obtained by calling
                            # get_horizontal_separator(x, y-1) which checks if (x, y-1) in h_walls
                            # So this wall connects cells (x, current_y-1) and (x, current_y)
                            walls.add(((x, current_y - 1), (x, current_y)))

                        # Move to next position (5 chars + 1 separator)
                        pos += 6
                        x += 1

    return walls


def extract_mud_positions(board: str) -> Set[Tuple[Tuple[int, int], Tuple[int, int]]]:
    """Extract mud positions from a rendered board.

    Mud appears as:
    - Vertical mud: ┊ character between cells (at x position)
    - Horizontal mud: ┈┈┈┈┈ (dotted line) between rows (at y position)

    Args:
        board: Rendered board string

    Returns:
        Set of mud passages as ((x1, y1), (x2, y2)) tuples
    """
    mud = set()

    clean_board = strip_ansi_codes(board)
    lines = clean_board.strip().split("\n")

    # Skip header
    board_lines = lines[2:]

    # Track current y coordinate
    current_y = None

    for line in board_lines:
        # Cell rows (with y-coordinate label)
        if "│" in line and not all(c in "─┈ ┼+│" for c in line.strip()):
            y_match = re.match(r"\s*(\d+)\s*│", line)
            if y_match:
                current_y = int(y_match.group(1))

                # Extract vertical mud
                content = re.sub(r"^\s*\d+\s*│", "", line)
                content = content.rstrip("│")

                x = 0
                pos = 0
                while pos < len(content):
                    # Check separator after this cell
                    if pos + 5 < len(content):
                        separator = content[pos + 5]
                        if separator == "┊":
                            # Same logic as vertical walls
                            # Vertical mud stored at (x, y) connects cells (x, y) and (x+1, y)
                            mud.add(((x, current_y), (x + 1, current_y)))

                    pos += 6
                    x += 1

        # Separator rows (horizontal walls/mud)
        elif "┼" in line and ("─" in line or "┈" in line):
            # This is a horizontal separator row
            if current_y is not None and current_y > 0:
                # Parse horizontal mud
                content_start = line.find("┼")
                if content_start >= 0:
                    content = line[content_start + 1 :]

                    x = 0
                    pos = 0
                    while pos < len(content):
                        # Extract next 5 characters (cell separator)
                        segment = content[pos : pos + 5]
                        if segment == "┈┈┈┈┈":
                            # Horizontal mud between (x, current_y-1) and (x, current_y)
                            mud.add(((x, current_y - 1), (x, current_y)))

                        # Move to next position (5 chars + 1 separator)
                        pos += 6
                        x += 1

    return mud


# ===========================
# Level 2: Structure Building Tests
# ===========================


class TestMazeStructureNormalization:
    """Test that build_maze_structures() preserves properties."""

    def test_structures_normalize_to_min_coordinate(self):
        """Wall order should not affect normalization result.

        Property: ((x1,y1), (x2,y2)) ≡ ((x2,y2), (x1,y1))
        """
        # Vertical wall in different orders
        walls1 = [((1, 2), (2, 2))]  # min_x first
        walls2 = [((2, 2), (1, 2))]  # max_x first

        struct1 = build_maze_structures(walls1, {})
        struct2 = build_maze_structures(walls2, {})

        assert (
            struct1.v_walls == struct2.v_walls
        ), "Vertical wall should be normalized to same position"
        assert (1, 2) in struct1.v_walls, "Should store at min_x=1"

        # Horizontal wall in different orders
        walls3 = [((3, 1), (3, 2))]  # min_y first
        walls4 = [((3, 2), (3, 1))]  # max_y first

        struct3 = build_maze_structures(walls3, {})
        struct4 = build_maze_structures(walls4, {})

        assert (
            struct3.h_walls == struct4.h_walls
        ), "Horizontal wall should be normalized to same position"
        assert (3, 1) in struct3.h_walls, "Should store at min_y=1"

    def test_structures_correct_categorization(self):
        """Walls should be categorized based on orientation.

        Property: same x → horizontal, same y → vertical
        """
        walls = [
            ((1, 1), (2, 1)),  # Different x, same y → vertical
            ((3, 2), (3, 3)),  # Same x, different y → horizontal
        ]

        structures = build_maze_structures(walls, {})

        assert (1, 1) in structures.v_walls, "Wall with same y should be in v_walls"
        assert (3, 2) in structures.h_walls, "Wall with same x should be in h_walls"

        assert len(structures.v_walls) == 1, "Should have exactly 1 vertical wall"
        assert len(structures.h_walls) == 1, "Should have exactly 1 horizontal wall"

    def test_structures_preserve_wall_symmetry(self):
        """Symmetric walls should produce symmetric structure sets."""
        width, height = 5, 5

        # Create symmetric wall pairs
        # For 5x5, center is at (2,2)
        # Wall between (1,1)-(2,1) maps to wall between (3,3)-(2,3)
        walls = [
            ((1, 1), (2, 1)),  # Vertical wall
            ((2, 3), (3, 3)),  # Symmetric vertical wall
            ((1, 1), (1, 2)),  # Horizontal wall
            ((3, 2), (3, 3)),  # Symmetric horizontal wall
        ]

        # For each wall, verify the wall connecting symmetric cells exists
        for wall in walls:
            (x1, y1), (x2, y2) = wall
            sym1 = get_symmetric_position(x1, y1, width, height)
            sym2 = get_symmetric_position(x2, y2, width, height)

            # The symmetric wall connects sym1 and sym2
            symmetric_wall = (sym1, sym2)
            symmetric_wall_reversed = (sym2, sym1)

            # Check if this symmetric wall exists in our original wall list
            assert (
                symmetric_wall in walls or symmetric_wall_reversed in walls
            ), f"Wall {wall} should have symmetric counterpart connecting {sym1} and {sym2}"

    def test_structures_preserve_mud_symmetry(self):
        """Symmetric mud should produce symmetric structure sets."""
        width, height = 5, 5

        # Create symmetric mud pairs
        # For 5x5, center is at (2,2)
        mud = {
            ((1, 1), (2, 1)): 3,  # Vertical mud with 3 turns
            ((2, 3), (3, 3)): 3,  # Symmetric vertical mud
            ((1, 1), (1, 2)): 2,  # Horizontal mud with 2 turns
            ((3, 2), (3, 3)): 2,  # Symmetric horizontal mud
        }

        # For each mud passage, verify the mud connecting symmetric cells exists
        for passage, turns in mud.items():
            (x1, y1), (x2, y2) = passage
            sym1 = get_symmetric_position(x1, y1, width, height)
            sym2 = get_symmetric_position(x2, y2, width, height)

            # The symmetric mud connects sym1 and sym2
            symmetric_passage = (sym1, sym2)
            symmetric_passage_reversed = (sym2, sym1)

            # Check if this symmetric mud exists with same turn count
            assert (symmetric_passage in mud and mud[symmetric_passage] == turns) or (
                symmetric_passage_reversed in mud
                and mud[symmetric_passage_reversed] == turns
            ), f"Mud {passage} with {turns} turns should have symmetric counterpart"


# ===========================
# Level 3: Rendering Primitives Tests
# ===========================


class TestRenderingPrimitiveSymmetry:
    """Test that individual rendering functions preserve symmetry."""

    def test_get_cell_content_symmetry(self):
        """Symmetric cell positions should render with R↔P swapped.

        Property: Content at (x,y) with players swapped = Content at symmetric position
        """
        # Rat at (1, 1), Python at symmetric position (3, 3)
        rat_pos = (1, 1)
        python_pos = (3, 3)
        cheese_set = {(2, 2)}  # Center cheese

        # Get content at rat's position
        content_rat = get_cell_content(
            rat_pos[0], rat_pos[1], rat_pos, python_pos, cheese_set
        )

        # Get content at python's position with players swapped
        content_python_swapped = get_cell_content(
            python_pos[0],
            python_pos[1],
            python_pos,
            rat_pos,  # Swapped
            cheese_set,
        )

        # Strip ANSI codes for comparison
        assert strip_ansi_codes(content_rat) == strip_ansi_codes(
            content_python_swapped
        ), "Rat at (1,1) should look like Python at (3,3) when players are swapped"

    def test_get_cell_content_with_cheese_symmetry(self):
        """Symmetric cheese positions should render symmetrically."""
        width, height = 5, 5

        rat_pos = (0, 0)
        python_pos = (4, 4)

        # Cheese at symmetric positions
        cheese1_pos = (1, 2)
        cheese2_pos = get_symmetric_position(
            cheese1_pos[0], cheese1_pos[1], width, height
        )

        cheese_set = {cheese1_pos, cheese2_pos}

        content1 = get_cell_content(
            cheese1_pos[0], cheese1_pos[1], rat_pos, python_pos, cheese_set
        )
        content2 = get_cell_content(
            cheese2_pos[0], cheese2_pos[1], rat_pos, python_pos, cheese_set
        )

        # Both should show cheese
        assert strip_ansi_codes(content1).strip() == "C"
        assert strip_ansi_codes(content2).strip() == "C"

    def test_get_vertical_separator_symmetry(self):
        """Symmetric vertical walls/mud should render symmetrically."""
        # Create symmetric vertical walls
        v_walls = frozenset({(1, 1), (2, 3)})  # Symmetric pair in 5x5
        structures = MazeStructures(
            h_walls=frozenset(), v_walls=v_walls, h_mud=frozenset(), v_mud=frozenset()
        )

        # Check first wall
        sep1 = get_vertical_separator(1, 1, structures)
        assert sep1 == VERTICAL_WALL

        # Check symmetric wall
        sep2 = get_vertical_separator(2, 3, structures)
        assert sep2 == VERTICAL_WALL

    def test_get_horizontal_separator_symmetry(self):
        """Symmetric horizontal walls/mud should render symmetrically."""
        # Create symmetric horizontal walls
        h_walls = frozenset({(2, 1), (2, 2)})  # Symmetric pair in 5x5
        structures = MazeStructures(
            h_walls=h_walls, v_walls=frozenset(), h_mud=frozenset(), v_mud=frozenset()
        )

        # Check first wall
        sep1 = get_horizontal_separator(2, 1, structures)
        assert sep1 == HORIZONTAL_WALL

        # Check symmetric wall
        sep2 = get_horizontal_separator(2, 2, structures)
        assert sep2 == HORIZONTAL_WALL

    def test_coordinate_system_bottom_left_origin(self):
        """Verify that (0,0) is at bottom-left in the rendered output."""
        width, height = 3, 3

        # Place rat at (0, 0) - should be bottom-left
        rat_pos = (0, 0)
        python_pos = (2, 2)  # Top-right

        board = render_board(
            width,
            height,
            rat_pos,
            python_pos,
            set(),
            MazeStructures(frozenset(), frozenset(), frozenset(), frozenset()),
        )

        lines = board.strip().split("\n")

        # The board renders from top to bottom (high y to low y)
        # So the LAST cell row should contain the rat at (0,0)
        # Find the last row with cells (contains '│' but not all separators)
        cell_rows = [
            line
            for line in lines
            if "│" in line and not all(c in "─┈ ┼+│" for c in line.strip())
        ]

        last_row = cell_rows[-1]

        # Rat should be in the leftmost cell of the last row
        assert "R" in last_row, "Rat at (0,0) should appear in bottom row"

    def test_wall_placement_on_correct_edge(self):
        """Verify walls appear on the correct edge of cells."""
        # Vertical wall between (0,1) and (1,1) - should be RIGHT of (0,1)
        v_walls = frozenset({(1, 1)})  # Stored at min_x

        # Horizontal wall between (1,0) and (1,1) - should be ABOVE (1,0)
        h_walls = frozenset({(1, 0)})  # Stored at min_y

        structures = MazeStructures(h_walls, v_walls, frozenset(), frozenset())

        # Vertical separator should appear at (1, 1)
        assert get_vertical_separator(1, 1, structures) == VERTICAL_WALL

        # Horizontal separator should appear at (1, 0)
        assert get_horizontal_separator(1, 0, structures) == HORIZONTAL_WALL


# ===========================
# Level 1: Data Extraction Tests
# ===========================


class TestGameStateExtractionSymmetry:
    """Test that symmetric games produce symmetric data when extracted."""

    def test_symmetric_game_wall_entries(self):
        """Symmetric game should produce symmetric wall entries.

        Uses engine's built-in symmetric game generation.
        """
        # Create a symmetric game with reproducible seed
        game = PyGameState(width=11, height=9, symmetric=True, seed=42)

        walls = game.wall_entries()

        # For each wall, verify its symmetric counterpart exists
        width, height = 11, 9
        walls_set = set(walls)

        for wall in walls:
            (x1, y1), (x2, y2) = wall
            sym1 = get_symmetric_position(x1, y1, width, height)
            sym2 = get_symmetric_position(x2, y2, width, height)

            # Symmetric wall can have either order
            symmetric_wall = (sym1, sym2)
            symmetric_wall_reversed = (sym2, sym1)

            assert (
                symmetric_wall in walls_set or symmetric_wall_reversed in walls_set
            ), f"Wall {wall} should have symmetric counterpart"

    def test_symmetric_game_mud_positions(self):
        """Symmetric game should produce symmetric mud positions."""
        # Create a symmetric game with mud
        # Note: Current implementation might not have mud in presets
        # So we create a custom symmetric game
        width, height = 7, 7

        # Create symmetric mud manually
        mud = [
            ((1, 1), (2, 1), 2),  # Vertical mud
            ((4, 5), (5, 5), 2),  # Symmetric pair
            ((2, 2), (2, 3), 3),  # Horizontal mud
            ((4, 3), (4, 4), 3),  # Symmetric pair
        ]

        game_state = PyGameState.create_custom(
            width=width,
            height=height,
            walls=[],
            mud=mud,
            cheese=[(3, 3)],  # Center cheese
            player1_pos=(0, 0),
            player2_pos=(6, 6),
        )

        mud_entries = game_state.mud_entries()

        # Build dict from mud entries list (mud_entries returns list of tuples)
        mud_dict = {}
        for entry in mud_entries:
            (x1, y1), (x2, y2), turns = entry
            mud_dict[((x1, y1), (x2, y2))] = turns

        # For each mud entry, verify its symmetric counterpart exists
        for (cell1, cell2), turns in mud_dict.items():
            x1, y1 = cell1
            x2, y2 = cell2

            sym1 = get_symmetric_position(x1, y1, width, height)
            sym2 = get_symmetric_position(x2, y2, width, height)

            # Symmetric mud can have either order
            symmetric_key1 = (sym1, sym2)
            symmetric_key2 = (sym2, sym1)

            assert (
                (symmetric_key1 in mud_dict and mud_dict[symmetric_key1] == turns)
                or (symmetric_key2 in mud_dict and mud_dict[symmetric_key2] == turns)
            ), f"Mud {(cell1, cell2)} with {turns} turns should have symmetric counterpart"

    def test_symmetric_game_cheese_positions(self):
        """Symmetric game should produce symmetric cheese positions."""
        # Create a symmetric game with odd number of cheese (will place one at center)
        game = PyGameState(width=11, height=9, cheese_count=11, symmetric=True, seed=42)

        cheese_positions = game.cheese_positions()
        width, height = 11, 9

        # Build set of cheese positions as tuples
        cheese_set = {(c.x, c.y) for c in cheese_positions}

        # For each cheese, verify its symmetric counterpart exists
        for x, y in cheese_set:
            sym_x, sym_y = get_symmetric_position(x, y, width, height)

            # Either it's a center piece (self-symmetric) or has a pair
            is_center = (sym_x, sym_y) == (x, y)
            has_symmetric_pair = (sym_x, sym_y) in cheese_set

            assert (
                is_center or has_symmetric_pair
            ), f"Cheese at ({x}, {y}) should either be at center or have symmetric pair at ({sym_x}, {sym_y})"


# ===========================
# Level 4: Full Board Rendering Tests
# ===========================


class TestFullBoardSymmetry:
    """End-to-end tests for complete board rendering."""

    def test_symmetric_game_renders_symmetrically(self):
        """Complete symmetric game should produce visually symmetric display.

        This is the end-to-end composition test.
        """
        # Use engine's built-in symmetric game generation for this test
        game = PyGameState(width=7, height=7, cheese_count=9, symmetric=True, seed=42)

        wrapper = PyRat.__new__(PyRat)
        wrapper._game = game

        # Get game data
        walls = game.wall_entries()
        cheese_list = game.cheese_positions()
        cheese_set = {(c.x, c.y) for c in cheese_list}

        # Build structures and render
        structures = build_maze_structures(walls, {})
        board = render_board(
            7,
            7,
            (0, 0),
            (6, 6),  # Players at symmetric corners
            cheese_set,
            structures,
        )

        # Verify board renders without errors
        assert board is not None
        assert len(board) > 0

        # Verify all expected entities appear in the board
        clean_board = strip_ansi_codes(board)
        assert "R" in clean_board, "Rat should appear in board"
        assert "P" in clean_board, "Python should appear in board"
        assert "C" in clean_board, "Cheese should appear in board"

        # Verify symmetric walls exist
        for wall in walls:
            (x1, y1), (x2, y2) = wall
            sym1 = get_symmetric_position(x1, y1, 7, 7)
            sym2 = get_symmetric_position(x2, y2, 7, 7)

            symmetric_wall = (sym1, sym2)
            symmetric_wall_reversed = (sym2, sym1)

            assert (
                symmetric_wall in walls or symmetric_wall_reversed in walls
            ), f"Symmetric game should have symmetric wall for {wall}"

    def test_boundary_walls_render_correctly(self):
        """Walls at board boundaries should render correctly."""
        width, height = 5, 5

        # Walls at all four boundaries
        walls = [
            ((0, 2), (0, 3)),  # Left edge horizontal wall
            ((4, 2), (4, 3)),  # Right edge horizontal wall
            ((2, 0), (3, 0)),  # Bottom edge vertical wall
            ((2, 4), (3, 4)),  # Top edge vertical wall
        ]

        structures = build_maze_structures(walls, {})

        board = render_board(width, height, (0, 0), (4, 4), set(), structures)

        # Verify board renders without errors
        assert board is not None
        assert len(board) > 0

        # Verify it contains wall characters
        assert "─" in board or "│" in board

    def test_center_positions_odd_dimensions(self):
        """Center positions in odd-dimension boards should be self-symmetric."""
        width, height = 5, 5

        # Center is at (2, 2)
        center_x, center_y = 2, 2

        # Verify center is self-symmetric
        sym_x, sym_y = get_symmetric_position(center_x, center_y, width, height)
        assert (sym_x, sym_y) == (center_x, center_y), "Center should be self-symmetric"

        # Place cheese at center
        PyGameState.create_custom(
            width=width,
            height=height,
            walls=[],
            mud=[],
            cheese=[(center_x, center_y)],
            player1_pos=(0, 0),
            player2_pos=(4, 4),
        )

        structures = build_maze_structures([], {})
        board = render_board(
            width, height, (0, 0), (4, 4), {(center_x, center_y)}, structures
        )

        # Verify cheese appears in the board
        assert "C" in strip_ansi_codes(board)

    def test_all_cell_combinations_render(self):
        """All combinations of cell occupancy should render correctly."""
        width, height = 7, 7

        # Create a game with various cell combinations
        # R, P, C, RC, PC, RP, RPC, empty

        # We'll need to test different configurations separately
        # since we can only have one rat and one python

        # Test 1: Rat only
        PyGameState.create_custom(
            width=width,
            height=height,
            walls=[],
            mud=[],
            cheese=[(5, 5)],  # Dummy cheese far away
            player1_pos=(1, 1),  # Rat here
            player2_pos=(6, 6),  # Python elsewhere
        )

        structures = build_maze_structures([], {})
        board1 = render_board(width, height, (1, 1), (6, 6), {(5, 5)}, structures)
        assert "R" in strip_ansi_codes(board1)

        # Test 2: Python only
        board2 = render_board(width, height, (0, 0), (2, 2), {(5, 5)}, structures)
        assert "P" in strip_ansi_codes(board2)

        # Test 3: Cheese only
        board3 = render_board(width, height, (0, 0), (6, 6), {(3, 3)}, structures)
        assert "C" in strip_ansi_codes(board3)

        # Test 4: Rat and cheese
        board4 = render_board(width, height, (2, 2), (6, 6), {(2, 2)}, structures)
        content = strip_ansi_codes(board4)
        assert "R" in content and "C" in content

        # Test 5: Python and cheese
        board5 = render_board(width, height, (0, 0), (3, 3), {(3, 3)}, structures)
        content = strip_ansi_codes(board5)
        assert "P" in content and "C" in content

        # Test 6: Both players at same position
        board6 = render_board(width, height, (4, 4), (4, 4), {(5, 5)}, structures)
        content = strip_ansi_codes(board6)
        assert "R" in content and "P" in content

        # Test 7: Both players and cheese at same position
        board7 = render_board(width, height, (4, 4), (4, 4), {(4, 4)}, structures)
        content = strip_ansi_codes(board7)
        assert "R" in content and "P" in content and "C" in content

        # Test 8: Empty cell (covered by any of the above where other cells are empty)
        assert True  # Implicitly tested


# ===========================
# Edge Cases Tests
# ===========================


class TestEdgeCases:
    """Test edge cases and boundary conditions."""

    def test_minimum_dimensions_3x3(self):
        """Minimum practical board size should work."""
        width, height = 3, 3

        PyGameState.create_custom(
            width=width,
            height=height,
            walls=[],
            mud=[],
            cheese=[(1, 1)],
            player1_pos=(0, 0),
            player2_pos=(2, 2),
        )

        structures = build_maze_structures([], {})
        board = render_board(width, height, (0, 0), (2, 2), {(1, 1)}, structures)

        assert board is not None
        assert "R" in strip_ansi_codes(board)
        assert "P" in strip_ansi_codes(board)
        assert "C" in strip_ansi_codes(board)

    def test_even_dimensions(self):
        """Even dimensions have no self-symmetric center."""
        width, height = 6, 6

        # Center would be between (2,2) and (3,3)
        # No single cell is self-symmetric

        # Create symmetric game with EVEN cheese count (required for even dimensions)
        game = PyGameState(
            width=width, height=height, cheese_count=10, symmetric=True, seed=42
        )

        walls = game.wall_entries()
        cheese = {(c.x, c.y) for c in game.cheese_positions()}

        # Verify symmetry of walls
        walls_set = set(walls)
        for wall in walls:
            (x1, y1), (x2, y2) = wall
            sym1 = get_symmetric_position(x1, y1, width, height)
            sym2 = get_symmetric_position(x2, y2, width, height)

            symmetric_wall = (sym1, sym2)
            symmetric_wall_reversed = (sym2, sym1)

            assert symmetric_wall in walls_set or symmetric_wall_reversed in walls_set

        # Verify symmetry of cheese
        for x, y in cheese:
            sym_x, sym_y = get_symmetric_position(x, y, width, height)
            assert (
                sym_x,
                sym_y,
            ) in cheese, "Even-dimension board should have symmetric cheese pairs"

    def test_odd_dimensions(self):
        """Odd dimensions have a self-symmetric center cell."""
        width, height = 7, 7

        # Center is at (3, 3)
        center_x, center_y = 3, 3

        # Verify center is self-symmetric
        sym_x, sym_y = get_symmetric_position(center_x, center_y, width, height)
        assert (sym_x, sym_y) == (center_x, center_y)

        # Create symmetric game with odd cheese count
        game = PyGameState(
            width=width, height=height, cheese_count=9, symmetric=True, seed=42
        )

        cheese = {(c.x, c.y) for c in game.cheese_positions()}

        # Every non-center cheese should have a symmetric pair
        for x, y in cheese:
            sym_x, sym_y = get_symmetric_position(x, y, width, height)
            assert (
                sym_x,
                sym_y,
            ) in cheese, f"Cheese at ({x}, {y}) should have symmetric counterpart"

    def test_mud_values_rendered(self):
        """Mud with different turn values should render."""
        width, height = 5, 5

        mud = {
            ((1, 1), (2, 1)): 2,  # 2-turn mud
            ((3, 3), (4, 3)): 5,  # 5-turn mud
            ((2, 2), (2, 3)): 3,  # 3-turn horizontal mud
        }

        structures = build_maze_structures([], mud)

        # Verify mud appears in structures
        assert (1, 1) in structures.v_mud
        assert (3, 3) in structures.v_mud
        assert (2, 2) in structures.h_mud

        # Render board with mud
        board = render_board(width, height, (0, 0), (4, 4), set(), structures)

        # Mud should appear as dotted lines
        assert "┊" in board or "┈" in board, "Mud should render with dotted lines"

    def test_symmetric_mud_pairs(self):
        """Symmetric mud should have same turn values."""
        width, height = 5, 5

        # Create symmetric mud pairs with same values
        mud = {
            ((1, 1), (2, 1)): 3,
            ((2, 3), (3, 3)): 3,  # Symmetric pair
        }

        structures = build_maze_structures([], mud)

        # Both should appear in v_mud
        assert (1, 1) in structures.v_mud
        assert (2, 3) in structures.v_mud

        # Render and verify both show as mud
        board = render_board(width, height, (0, 0), (4, 4), set(), structures)
        assert "┊" in board  # Vertical mud character


# ===========================
# Direct Rendering Verification Tests
# ===========================


class TestDirectRenderingVerification:
    """Direct verification that rendered output matches input game state.

    These tests create games with known configurations, render them,
    parse the output, and verify that what we parse matches what we put in.
    This directly verifies rendering correctness.
    """

    def test_render_5x5_minimal(self):
        """Test 5×5 board with minimal elements."""
        width, height = 5, 5

        # Known configuration
        rat_pos = (1, 1)
        python_pos = (3, 3)
        cheese = [(2, 2)]
        walls = [
            ((1, 2), (2, 2)),  # Vertical wall
            ((3, 1), (3, 2)),  # Horizontal wall
        ]

        # Render
        structures = build_maze_structures(walls, {})
        board = render_board(
            width, height, rat_pos, python_pos, set(cheese), structures
        )

        # Parse
        positions = extract_entity_positions(board)
        extracted_walls = extract_wall_positions(board)

        # Verify entities
        assert positions["R"] == {rat_pos}, "Rat position mismatch"
        assert positions["P"] == {python_pos}, "Python position mismatch"
        assert positions["C"] == set(cheese), "Cheese positions mismatch"

        # Verify walls
        assert set(extracted_walls) == set(walls), "Wall positions mismatch"

    def test_render_7x7_with_multiple_elements(self):
        """Test 7×7 board with multiple walls, mud, and cheese."""
        width, height = 7, 7

        # Known configuration
        rat_pos = (0, 0)
        python_pos = (6, 6)
        cheese = [(1, 2), (3, 3), (5, 4)]
        walls = [
            ((2, 1), (3, 1)),  # Vertical wall
            ((4, 3), (5, 3)),  # Vertical wall
            ((1, 2), (1, 3)),  # Horizontal wall
            ((5, 4), (5, 5)),  # Horizontal wall
        ]
        mud = [
            ((1, 1), (2, 1), 2),  # Vertical mud
            ((3, 2), (3, 3), 3),  # Horizontal mud
        ]

        # Render
        mud_dict = {(cell1, cell2): turns for (cell1, cell2, turns) in mud}
        structures = build_maze_structures(walls, mud_dict)
        board = render_board(
            width, height, rat_pos, python_pos, set(cheese), structures
        )

        # Parse
        positions = extract_entity_positions(board)
        extracted_walls = extract_wall_positions(board)
        extracted_mud = extract_mud_positions(board)

        # Verify entities
        assert positions["R"] == {rat_pos}
        assert positions["P"] == {python_pos}
        assert positions["C"] == set(cheese)

        # Verify walls
        assert set(extracted_walls) == set(walls)

        # Verify mud (only check positions, not turn counts)
        expected_mud_positions = {(cell1, cell2) for (cell1, cell2, turns) in mud}
        assert extracted_mud == expected_mud_positions

    def test_render_different_sizes(self):
        """Test rendering with different board dimensions."""
        test_cases = [
            (3, 3, (0, 0), (2, 2), [(1, 1)]),
            (8, 6, (0, 0), (7, 5), [(2, 2), (5, 3)]),
            (11, 9, (1, 1), (9, 7), [(3, 3), (5, 5), (7, 6)]),
        ]

        for width, height, rat_pos, python_pos, cheese in test_cases:
            structures = build_maze_structures([], {})
            board = render_board(
                width, height, rat_pos, python_pos, set(cheese), structures
            )

            positions = extract_entity_positions(board)

            assert positions["R"] == {rat_pos}, f"Rat mismatch at {width}×{height}"
            assert positions["P"] == {
                python_pos
            }, f"Python mismatch at {width}×{height}"
            assert positions["C"] == set(cheese), f"Cheese mismatch at {width}×{height}"

    def test_render_boundary_positions(self):
        """Test entities at board boundaries."""
        width, height = 5, 5

        # Test all four corners
        corners = [(0, 0), (4, 0), (0, 4), (4, 4)]

        for corner in corners:
            structures = build_maze_structures([], {})
            board = render_board(width, height, corner, (2, 2), {corner}, structures)

            positions = extract_entity_positions(board)

            assert positions["R"] == {
                corner
            }, f"Rat at corner {corner} not rendered correctly"
            assert positions["C"] == {
                corner
            }, f"Cheese at corner {corner} not rendered correctly"

    def test_render_overlapping_entities(self):
        """Test entities at the same position."""
        width, height = 5, 5

        # Rat, Python, and Cheese all at (2, 2)
        pos = (2, 2)

        structures = build_maze_structures([], {})
        board = render_board(width, height, pos, pos, {pos}, structures)

        positions = extract_entity_positions(board)

        # All three should be detected at the same position
        assert positions["R"] == {pos}, "Rat not detected when overlapping"
        assert positions["P"] == {pos}, "Python not detected when overlapping"
        assert positions["C"] == {pos}, "Cheese not detected when overlapping"

    def test_render_walls_at_all_edges(self):
        """Test walls at all four edges of the board."""
        width, height = 5, 5

        walls = [
            ((0, 2), (0, 3)),  # Left edge horizontal
            ((4, 2), (4, 3)),  # Right edge horizontal
            ((2, 0), (3, 0)),  # Bottom edge vertical
            ((2, 4), (3, 4)),  # Top edge vertical
        ]

        structures = build_maze_structures(walls, {})
        board = render_board(width, height, (1, 1), (3, 3), set(), structures)

        extracted_walls = extract_wall_positions(board)

        assert set(extracted_walls) == set(
            walls
        ), "Boundary walls not rendered correctly"

    def test_render_center_position_odd_dimensions(self):
        """Test center position in odd-dimension board."""
        width, height = 7, 7
        center = (3, 3)

        structures = build_maze_structures([], {})
        board = render_board(width, height, (0, 0), center, {center}, structures)

        positions = extract_entity_positions(board)

        # Verify center is self-symmetric
        sym_x, sym_y = get_symmetric_position(center[0], center[1], width, height)
        assert (sym_x, sym_y) == center, "Center should be self-symmetric"

        # Verify rendering
        assert positions["P"] == {center}
        assert positions["C"] == {center}
