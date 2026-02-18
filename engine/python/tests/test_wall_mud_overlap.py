"""Test that walls and mud never overlap in the game state."""

from pyrat_engine import GameConfig

# ruff: noqa: PLR2004


def test_no_wall_mud_overlap_small_maze():
    """Test that walls and mud don't overlap in a small maze."""
    # Test the specific case that was failing: 5x5 with seed=0
    game = GameConfig.classic(5, 5, 5).create(seed=0)

    walls = game.wall_entries()
    mud = game.mud_entries()

    # Create normalized sets for comparison
    wall_set = set()
    for wall in walls:
        p1, p2 = (wall.pos1.x, wall.pos1.y), (wall.pos2.x, wall.pos2.y)
        normalized = (min(p1, p2), max(p1, p2))
        wall_set.add(normalized)

    mud_set = set()
    for m in mud:
        p1, p2 = (m.pos1.x, m.pos1.y), (m.pos2.x, m.pos2.y)
        normalized = (min(p1, p2), max(p1, p2))
        mud_set.add(normalized)

    # Check for overlaps
    overlaps = wall_set & mud_set
    assert len(overlaps) == 0, f"Found {len(overlaps)} wall/mud overlaps: {overlaps}"


def test_no_wall_mud_overlap_default_maze():
    """Test that walls and mud don't overlap in a default maze."""
    game = GameConfig.classic(21, 15, 41).create()

    walls = game.wall_entries()
    mud = game.mud_entries()

    # Create normalized sets for comparison
    wall_set = set()
    for wall in walls:
        p1, p2 = (wall.pos1.x, wall.pos1.y), (wall.pos2.x, wall.pos2.y)
        normalized = (min(p1, p2), max(p1, p2))
        wall_set.add(normalized)

    mud_set = set()
    for m in mud:
        p1, p2 = (m.pos1.x, m.pos1.y), (m.pos2.x, m.pos2.y)
        normalized = (min(p1, p2), max(p1, p2))
        mud_set.add(normalized)

    # Check for overlaps
    overlaps = wall_set & mud_set
    assert len(overlaps) == 0, f"Found {len(overlaps)} wall/mud overlaps"


def test_no_wall_mud_overlap_multiple_seeds():
    """Test multiple seeds that were known to have issues."""
    # Seeds that previously had overlaps in 5x5
    problematic_seeds = [0, 5, 8, 9, 11, 13, 16, 17, 18]

    config = GameConfig.classic(5, 5, 5)
    for seed in problematic_seeds:
        game = config.create(seed=seed)

        walls = game.wall_entries()
        mud = game.mud_entries()

        # Create normalized sets for comparison
        wall_set = set()
        for wall in walls:
            p1, p2 = (wall.pos1.x, wall.pos1.y), (wall.pos2.x, wall.pos2.y)
            normalized = (min(p1, p2), max(p1, p2))
            wall_set.add(normalized)

        mud_set = set()
        for m in mud:
            p1, p2 = (m.pos1.x, m.pos1.y), (m.pos2.x, m.pos2.y)
            normalized = (min(p1, p2), max(p1, p2))
            mud_set.add(normalized)

        # Check for overlaps
        overlaps = wall_set & mud_set
        assert (
            len(overlaps) == 0
        ), f"Seed {seed}: Found {len(overlaps)} wall/mud overlaps: {overlaps}"


def test_wall_entries_reasonable_count():
    """Test that wall_entries returns a reasonable number of walls."""
    # For a 5x5 maze, there are 40 possible walls (internal connections)
    # A typical maze should have around 10-20 walls
    game = GameConfig.classic(5, 5, 5).create(seed=0)
    walls = game.wall_entries()

    assert 5 <= len(walls) <= 30, f"Unexpected number of walls: {len(walls)}"

    # For default 21x15 maze
    game = GameConfig.classic(21, 15, 41).create()
    walls = game.wall_entries()

    # Maximum possible internal walls: 20*15 + 21*14 = 594
    # Typical maze should have 200-400 walls
    assert 150 <= len(walls) <= 450, f"Unexpected number of walls: {len(walls)}"


def test_walls_are_between_adjacent_cells():
    """Test that all walls are between adjacent cells."""
    game = GameConfig.classic(5, 5, 5).create(seed=0)
    walls = game.wall_entries()

    for wall in walls:
        x1, y1 = wall.pos1.x, wall.pos1.y
        x2, y2 = wall.pos2.x, wall.pos2.y
        # Check that cells are adjacent (Manhattan distance = 1)
        dx = abs(x1 - x2)
        dy = abs(y1 - y2)
        assert dx + dy == 1, f"Wall between non-adjacent cells: ({x1},{y1})-({x2},{y2})"

        # Check that both positions are within bounds
        assert 0 <= x1 < game.width, f"x1={x1} out of bounds"
        assert 0 <= y1 < game.height, f"y1={y1} out of bounds"
        assert 0 <= x2 < game.width, f"x2={x2} out of bounds"
        assert 0 <= y2 < game.height, f"y2={y2} out of bounds"


def test_mud_only_on_passages():
    """Test that mud only exists where there are no walls (i.e., on passages)."""
    game = GameConfig.classic(7, 7, 9).create(seed=42)

    walls = game.wall_entries()
    mud = game.mud_entries()

    # Create wall set
    wall_set = set()
    for wall in walls:
        p1, p2 = (wall.pos1.x, wall.pos1.y), (wall.pos2.x, wall.pos2.y)
        normalized = (min(p1, p2), max(p1, p2))
        wall_set.add(normalized)

    # Verify each mud entry is on a passage (not a wall)
    for m in mud:
        p1, p2 = (m.pos1.x, m.pos1.y), (m.pos2.x, m.pos2.y)
        normalized = (min(p1, p2), max(p1, p2))
        assert normalized not in wall_set, f"Mud on wall at {normalized}"
        assert m.value >= 2, f"Invalid mud value {m.value} (must be >= 2)"
