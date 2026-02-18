"""Tests for the GameBuilder + GameConfig API."""

# ruff: noqa: PLR2004

import pytest
from pyrat_engine import GameBuilder, GameConfig


class TestGameConfigPresets:
    """Test GameConfig preset and classic shortcuts."""

    def test_preset_medium(self):
        config = GameConfig.preset("medium")
        assert config.width == 21
        assert config.height == 15
        assert config.max_turns == 300

    def test_preset_all(self):
        for name in ["tiny", "small", "medium", "large", "huge", "open", "asymmetric"]:
            config = GameConfig.preset(name)
            game = config.create(seed=42)
            assert game.width == config.width
            assert game.height == config.height
            assert len(game.cheese_positions()) > 0

    def test_preset_invalid(self):
        with pytest.raises(ValueError, match="Unknown preset"):
            GameConfig.preset("nonexistent")

    def test_classic(self):
        config = GameConfig.classic(21, 15, 41)
        assert config.width == 21
        assert config.height == 15
        game = config.create(seed=42)
        assert len(game.cheese_positions()) == 41

    def test_config_properties(self):
        config = GameConfig.preset("tiny")
        assert config.width == 11
        assert config.height == 9
        assert config.max_turns == 150

    def test_config_repr(self):
        config = GameConfig.classic(21, 15, 41)
        assert "21x15" in repr(config)

    def test_config_copy(self):
        import copy

        config = GameConfig.classic(21, 15, 41)
        config2 = copy.copy(config)
        assert config2.width == config.width


class TestGameConfigReuse:
    """Test that GameConfig can stamp out multiple games."""

    def test_same_seed_same_game(self):
        config = GameConfig.classic(11, 9, 13)
        game1 = config.create(seed=42)
        game2 = config.create(seed=42)
        assert game1.cheese_positions() == game2.cheese_positions()
        assert game1.player1_position == game2.player1_position

    def test_different_seeds_different_games(self):
        config = GameConfig.classic(21, 15, 41)
        game1 = config.create(seed=1)
        game2 = config.create(seed=2)
        # Overwhelmingly likely to differ
        cheese1 = {(c.x, c.y) for c in game1.cheese_positions()}
        cheese2 = {(c.x, c.y) for c in game2.cheese_positions()}
        assert cheese1 != cheese2

    def test_same_dimensions_across_creates(self):
        config = GameConfig.preset("large")
        for seed in range(5):
            game = config.create(seed=seed)
            assert game.width == config.width
            assert game.height == config.height
            assert game.max_turns == config.max_turns


class TestGameBuilderMazeStrategies:
    """Test each maze strategy through the builder."""

    def test_classic_maze(self):
        config = (
            GameBuilder(11, 9)
            .with_classic_maze()
            .with_corner_positions()
            .with_random_cheese(13)
            .build()
        )
        game = config.create(seed=42)
        assert game.width == 11
        assert len(game.wall_entries()) > 0

    def test_open_maze(self):
        config = (
            GameBuilder(11, 9)
            .with_open_maze()
            .with_corner_positions()
            .with_random_cheese(13)
            .build()
        )
        game = config.create(seed=42)
        assert len(game.wall_entries()) == 0
        assert len(game.mud_entries()) == 0

    def test_random_maze_custom_params(self):
        config = (
            GameBuilder(11, 9)
            .with_random_maze(wall_density=0.3, mud_density=0.0, symmetric=False)
            .with_corner_positions()
            .with_random_cheese(13, symmetric=False)
            .build()
        )
        game = config.create(seed=42)
        assert game.width == 11
        assert len(game.mud_entries()) == 0

    def test_custom_maze(self):
        config = (
            GameBuilder(5, 5)
            .with_custom_maze(
                walls=[((0, 0), (0, 1)), ((4, 4), (4, 3))],
                mud=[((1, 1), (1, 2), 3)],
            )
            .with_corner_positions()
            .with_custom_cheese([(2, 2)])
            .build()
        )
        game = config.create()
        assert len(game.wall_entries()) == 2
        assert len(game.mud_entries()) == 1

    def test_custom_maze_walls_only(self):
        config = (
            GameBuilder(5, 5)
            .with_custom_maze(walls=[((0, 0), (0, 1))])
            .with_corner_positions()
            .with_custom_cheese([(2, 2)])
            .build()
        )
        game = config.create()
        assert len(game.wall_entries()) == 1


class TestGameBuilderPlayerStrategies:
    """Test each player placement strategy."""

    def test_corner_positions(self):
        config = (
            GameBuilder(11, 9)
            .with_open_maze()
            .with_corner_positions()
            .with_random_cheese(5)
            .build()
        )
        game = config.create(seed=42)
        assert game.player1_position.x == 0
        assert game.player1_position.y == 0
        assert game.player2_position.x == 10
        assert game.player2_position.y == 8

    def test_random_positions(self):
        config = (
            GameBuilder(11, 9)
            .with_open_maze()
            .with_random_positions()
            .with_random_cheese(5, symmetric=False)
            .build()
        )
        game = config.create(seed=42)
        assert game.player1_position != game.player2_position

    def test_custom_positions(self):
        config = (
            GameBuilder(11, 9)
            .with_open_maze()
            .with_custom_positions((3, 3), (7, 5))
            .with_random_cheese(5, symmetric=False)
            .build()
        )
        game = config.create(seed=42)
        assert game.player1_position.x == 3
        assert game.player1_position.y == 3
        assert game.player2_position.x == 7
        assert game.player2_position.y == 5

    def test_custom_positions_out_of_bounds(self):
        with pytest.raises(ValueError, match="outside board bounds"):
            GameBuilder(5, 5).with_open_maze().with_custom_positions((10, 10), (0, 0))


class TestGameBuilderCheeseStrategies:
    """Test each cheese placement strategy."""

    def test_random_cheese(self):
        config = (
            GameBuilder(11, 9)
            .with_open_maze()
            .with_corner_positions()
            .with_random_cheese(13)
            .build()
        )
        game = config.create(seed=42)
        assert len(game.cheese_positions()) == 13

    def test_custom_cheese(self):
        config = (
            GameBuilder(5, 5)
            .with_open_maze()
            .with_corner_positions()
            .with_custom_cheese([(1, 1), (2, 2), (3, 3)])
            .build()
        )
        game = config.create()
        assert len(game.cheese_positions()) == 3

    def test_custom_cheese_empty_raises(self):
        with pytest.raises(ValueError, match="at least one cheese"):
            GameBuilder(
                5, 5
            ).with_open_maze().with_corner_positions().with_custom_cheese([])

    def test_custom_cheese_duplicate_raises(self):
        with pytest.raises(ValueError, match="Duplicate cheese"):
            (
                GameBuilder(5, 5)
                .with_open_maze()
                .with_corner_positions()
                .with_custom_cheese([(1, 1), (1, 1)])
            )

    def test_custom_cheese_out_of_bounds_raises(self):
        with pytest.raises(ValueError, match="outside board bounds"):
            (
                GameBuilder(5, 5)
                .with_open_maze()
                .with_corner_positions()
                .with_custom_cheese([(10, 10)])
            )


class TestGameBuilderValidation:
    """Test that build() fails when strategies are missing."""

    def test_missing_maze(self):
        with pytest.raises(ValueError, match="Maze strategy not set"):
            (GameBuilder(5, 5).with_corner_positions().with_random_cheese(5).build())

    def test_missing_players(self):
        with pytest.raises(ValueError, match="Player strategy not set"):
            GameBuilder(5, 5).with_open_maze().with_random_cheese(5).build()

    def test_missing_cheese(self):
        with pytest.raises(ValueError, match="Cheese strategy not set"):
            GameBuilder(5, 5).with_open_maze().with_corner_positions().build()

    def test_max_turns_zero_raises(self):
        with pytest.raises(ValueError, match="max_turns must be greater than 0"):
            GameBuilder(5, 5).with_max_turns(0)

    def test_zero_dimensions_raises(self):
        with pytest.raises(ValueError, match="width must be >= 2"):
            GameBuilder(0, 5)
        with pytest.raises(ValueError, match="height must be >= 2"):
            GameBuilder(5, 0)

    def test_one_by_one_raises(self):
        with pytest.raises(ValueError, match="width must be >= 2"):
            GameBuilder(1, 5)
        with pytest.raises(ValueError, match="height must be >= 2"):
            GameBuilder(5, 1)


class TestGameBuilderMaxTurns:
    """Test max_turns override."""

    def test_default_max_turns(self):
        config = (
            GameBuilder(11, 9)
            .with_open_maze()
            .with_corner_positions()
            .with_random_cheese(5)
            .build()
        )
        assert config.max_turns == 300

    def test_custom_max_turns(self):
        config = (
            GameBuilder(11, 9)
            .with_max_turns(500)
            .with_open_maze()
            .with_corner_positions()
            .with_random_cheese(5)
            .build()
        )
        assert config.max_turns == 500


class TestResetPreservesConfig:
    """Test that reset() uses the stored config."""

    def test_reset_preserves_dimensions(self):
        config = GameConfig.classic(11, 9, 13)
        game = config.create(seed=42)
        game.reset(seed=99)
        assert game.width == 11
        assert game.height == 9
        assert len(game.cheese_positions()) == 13

    def test_reset_with_custom_positions(self):
        """After reset, a game built with custom positions gets new random state
        but same player positions (since they're fixed in the config)."""
        config = (
            GameBuilder(11, 9)
            .with_open_maze()
            .with_custom_positions((3, 3), (7, 5))
            .with_random_cheese(5, symmetric=False)
            .build()
        )
        game = config.create(seed=42)
        game.reset(seed=99)
        # Fixed positions survive reset
        assert game.player1_position.x == 3
        assert game.player1_position.y == 3
        assert game.player2_position.x == 7
        assert game.player2_position.y == 5

    def test_reset_with_custom_cheese(self):
        config = (
            GameBuilder(5, 5)
            .with_open_maze()
            .with_corner_positions()
            .with_custom_cheese([(1, 1), (2, 2), (3, 3)])
            .build()
        )
        game = config.create()
        # Play a bit
        game.step(0, 0)
        # Reset should restore cheese
        game.reset()
        cheese = {(c.x, c.y) for c in game.cheese_positions()}
        assert cheese == {(1, 1), (2, 2), (3, 3)}

    def test_reset_preserves_max_turns(self):
        config = (
            GameBuilder(11, 9)
            .with_max_turns(500)
            .with_open_maze()
            .with_corner_positions()
            .with_random_cheese(5)
            .build()
        )
        game = config.create(seed=42)
        game.reset(seed=99)
        assert game.max_turns == 500

    def test_preset_game_reset_preserves_symmetry(self):
        """Resetting a symmetric preset game should produce symmetric cheese."""
        game = GameConfig.preset("medium").create(seed=42)
        game.reset(seed=123)

        cheese = game.cheese_positions()
        cheese_set = {(c.x, c.y) for c in cheese}
        width, height = game.width, game.height

        for c in cheese:
            sym_x = width - 1 - c.x
            sym_y = height - 1 - c.y
            assert (sym_x, sym_y) in cheese_set or (c.x == sym_x and c.y == sym_y)


class TestBuilderChaining:
    """Test that the full builder chain works end-to-end."""

    def test_full_chain(self):
        config = (
            GameBuilder(21, 15)
            .with_classic_maze()
            .with_corner_positions()
            .with_random_cheese(41)
            .build()
        )
        game = config.create(seed=42)
        assert game.width == 21
        assert game.height == 15
        assert len(game.cheese_positions()) == 41

    def test_mixed_strategies(self):
        """Random maze + custom positions + random cheese."""
        config = (
            GameBuilder(11, 9)
            .with_random_maze(wall_density=0.5, mud_density=0.05)
            .with_custom_positions((2, 2), (8, 6))
            .with_random_cheese(10, symmetric=False)
            .build()
        )
        game = config.create(seed=42)
        assert game.player1_position.x == 2
        assert game.player1_position.y == 2
        assert game.player2_position.x == 8
        assert game.player2_position.y == 6
        assert len(game.cheese_positions()) == 10


class TestMazeSymmetry:
    """Test that symmetric maze generation produces 180°-rotationally symmetric walls."""

    def test_symmetric_maze_has_symmetric_walls(self):
        config = GameConfig.classic(21, 15, 41)
        game = config.create(seed=42)
        width, height = game.width, game.height

        walls = game.wall_entries()
        wall_set = {((w.pos1.x, w.pos1.y), (w.pos2.x, w.pos2.y)) for w in walls}

        for w in walls:
            # 180° rotation: (x, y) → (width-1-x, height-1-y)
            sym_p1 = (width - 1 - w.pos1.x, height - 1 - w.pos1.y)
            sym_p2 = (width - 1 - w.pos2.x, height - 1 - w.pos2.y)
            # Normalize order (smaller first) to match wall_set
            sym_wall = (min(sym_p1, sym_p2), max(sym_p1, sym_p2))
            assert sym_wall in wall_set, (
                f"Wall {w.pos1}↔{w.pos2} has no symmetric counterpart "
                f"at {sym_wall[0]}↔{sym_wall[1]}"
            )
