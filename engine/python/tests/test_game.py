"""Tests for PyRat from the Rust bindings.

This tests the game state implementation including:
- Game creation via GameConfig and GameBuilder
- Preset configurations
- Custom game creation
- Valid moves and effective actions
- Copy protocol
"""
# ruff: noqa: PLR2004

import pytest
from pyrat_engine import GameBuilder, GameConfig


class TestGameCreation:
    """Test game creation via GameConfig and GameBuilder."""

    def test_classic_config(self) -> None:
        """Test basic game creation with GameConfig.classic."""
        game = GameConfig.classic(5, 5, 3).create()
        assert game.width == 5
        assert game.height == 5
        assert len(game.cheese_positions()) == 3

    def test_classic_defaults(self) -> None:
        """Test game creation with standard defaults."""
        game = GameConfig.classic(21, 15, 41).create()
        assert game.width == 21
        assert game.height == 15
        assert game.max_turns == 300
        assert len(game.cheese_positions()) == 41

    def test_builder_with_max_turns(self):
        """Test that max_turns can be set via builder."""
        config = (
            GameBuilder(21, 15)
            .with_max_turns(500)
            .with_classic_maze()
            .with_corner_positions()
            .with_random_cheese(41)
            .build()
        )
        game = config.create()
        assert game.max_turns == 500

    def test_builder_all_parameters(self):
        """Test builder with all parameters."""
        config = (
            GameBuilder(15, 11)
            .with_max_turns(200)
            .with_classic_maze()
            .with_corner_positions()
            .with_random_cheese(21)
            .build()
        )
        game = config.create(seed=42)
        assert game.width == 15
        assert game.height == 11
        assert game.max_turns == 200
        assert len(game.cheese_positions()) == 21

    def test_seed_reproducibility(self):
        """Test that same config + seed produces same game."""
        config = GameConfig.classic(10, 10, 10)
        game1 = config.create(seed=42)
        game2 = config.create(seed=42)
        assert game1.cheese_positions() == game2.cheese_positions()


class TestDensityParameters:
    """Test wall_density and mud_density parameters via builder."""

    def test_zero_wall_density_creates_open_maze(self):
        """wall_density=0.0 should create a maze with no walls."""
        config = (
            GameBuilder(11, 11)
            .with_random_maze(wall_density=0.0, mud_density=0.1)
            .with_corner_positions()
            .with_random_cheese(10)
            .build()
        )
        game = config.create(seed=42)
        walls = game.wall_entries()
        assert len(walls) == 0

    def test_zero_mud_density_creates_no_mud(self):
        """mud_density=0.0 should create a maze with no mud."""
        config = (
            GameBuilder(11, 11)
            .with_random_maze(wall_density=0.7, mud_density=0.0)
            .with_corner_positions()
            .with_random_cheese(10)
            .build()
        )
        game = config.create(seed=42)
        mud = game.mud_entries()
        assert len(mud) == 0

    def test_open_maze_with_random_cheese(self):
        """Open maze with random symmetric cheese."""
        config = (
            GameBuilder(5, 5)
            .with_open_maze()
            .with_corner_positions()
            .with_random_cheese(5)
            .build()
        )
        game = config.create(seed=42)
        assert len(game.wall_entries()) == 0
        assert len(game.mud_entries()) == 0
        assert len(game.cheese_positions()) == 5

    def test_high_wall_density_creates_more_walls(self):
        """Higher wall_density should create more walls."""
        config_low = (
            GameBuilder(15, 15)
            .with_random_maze(wall_density=0.3)
            .with_corner_positions()
            .with_random_cheese(10)
            .build()
        )
        config_high = (
            GameBuilder(15, 15)
            .with_random_maze(wall_density=0.9)
            .with_corner_positions()
            .with_random_cheese(10)
            .build()
        )

        walls_low = len(config_low.create(seed=42).wall_entries())
        walls_high = len(config_high.create(seed=42).wall_entries())

        assert walls_high > walls_low

    def test_high_mud_density_creates_more_mud(self):
        """Higher mud_density should create more mud passages."""
        config_low = (
            GameBuilder(15, 15)
            .with_random_maze(mud_density=0.1)
            .with_corner_positions()
            .with_random_cheese(10)
            .build()
        )
        config_high = (
            GameBuilder(15, 15)
            .with_random_maze(mud_density=0.8)
            .with_corner_positions()
            .with_random_cheese(10)
            .build()
        )

        mud_low = len(config_low.create(seed=42).mud_entries())
        mud_high = len(config_high.create(seed=42).mud_entries())

        assert mud_high > mud_low

    def test_asymmetric_maze(self):
        """Density parameters should work with asymmetric games."""
        config = (
            GameBuilder(11, 11)
            .with_random_maze(wall_density=0.0, mud_density=0.0, symmetric=False)
            .with_corner_positions()
            .with_random_cheese(10, symmetric=False)
            .build()
        )
        game = config.create(seed=42)
        assert len(game.wall_entries()) == 0
        assert len(game.mud_entries()) == 0


class TestPresets:
    """Test the preset system."""

    def test_all_presets_exist(self):
        """Test that all presets can be created."""
        presets = ["tiny", "small", "medium", "large", "huge", "open", "asymmetric"]

        for preset in presets:
            game = GameConfig.preset(preset).create()
            assert game is not None

    def test_preset_dimensions(self):
        """Test that presets have correct dimensions."""
        expected = {
            "tiny": (11, 9, 13, 150),
            "small": (15, 11, 21, 200),
            "medium": (21, 15, 41, 300),
            "large": (31, 21, 85, 400),
            "huge": (41, 31, 165, 500),
            "open": (21, 15, 41, 300),
            "asymmetric": (21, 15, 41, 300),
        }

        for preset, (width, height, cheese, turns) in expected.items():
            game = GameConfig.preset(preset).create()
            assert game.width == width
            assert game.height == height
            assert game.max_turns == turns
            # Cheese count might vary slightly due to generation
            assert abs(len(game.cheese_positions()) - cheese) <= 2

    def test_preset_with_seed(self):
        """Test that presets with same seed are reproducible."""
        config = GameConfig.preset("medium")
        game1 = config.create(seed=42)
        game2 = config.create(seed=42)

        cheese1 = set(game1.cheese_positions())
        cheese2 = set(game2.cheese_positions())
        assert cheese1 == cheese2

    def test_open_preset_has_no_walls(self):
        """Test that open preset has no walls or mud."""
        game = GameConfig.preset("open").create()

        obs = game.get_observation(True)
        movement_matrix = obs.movement_matrix

        # Check a center position (should have all 4 moves valid)
        center_x, center_y = game.width // 2, game.height // 2
        for direction in range(4):
            assert movement_matrix[center_x][center_y][direction] == 0

    def test_invalid_preset_name(self):
        """Test that invalid preset names raise an error."""
        with pytest.raises(ValueError, match="Unknown preset"):
            GameConfig.preset("invalid_preset")


class TestCustomCreationMethods:
    """Test custom game creation via builder."""

    def test_custom_maze_with_random_cheese(self):
        """Test creating a game from a specific maze layout with random cheese."""
        config = (
            GameBuilder(5, 5)
            .with_max_turns(100)
            .with_custom_maze(walls=[((0, 0), (0, 1)), ((1, 1), (2, 1))])
            .with_corner_positions()
            .with_random_cheese(3, symmetric=False)
            .build()
        )
        game = config.create(seed=42)

        assert game.width == 5
        assert game.height == 5
        assert game.max_turns == 100
        assert 2 <= len(game.cheese_positions()) <= 4

    def test_custom_positions(self):
        """Test creating a game with custom starting positions."""
        config = (
            GameBuilder(15, 11)
            .with_max_turns(200)
            .with_classic_maze()
            .with_custom_positions((3, 3), (11, 7))
            .with_random_cheese(21)
            .build()
        )
        game = config.create(seed=42)

        assert game.width == 15
        assert game.height == 11
        assert game.player1_position.x == 3
        assert game.player1_position.y == 3
        assert game.player2_position.x == 11
        assert game.player2_position.y == 7
        assert game.max_turns == 200


class TestResetSymmetry:
    """Test that reset() respects the config."""

    def test_symmetric_game_reset_stays_symmetric(self):
        """Test that resetting a symmetric game generates symmetric maze."""
        config = GameConfig.classic(11, 9, 13)
        game = config.create(seed=42)
        game.reset(seed=123)

        # Check cheese positions are symmetric
        cheese = game.cheese_positions()
        cheese_set = {(c.x, c.y) for c in cheese}
        width, height = game.width, game.height

        for c in cheese:
            sym_x = width - 1 - c.x
            sym_y = height - 1 - c.y
            assert (sym_x, sym_y) in cheese_set or (c.x == sym_x and c.y == sym_y)

    def test_asymmetric_game_reset(self):
        """Test that resetting an asymmetric game works."""
        config = (
            GameBuilder(21, 15)
            .with_random_maze(symmetric=False)
            .with_corner_positions()
            .with_random_cheese(41, symmetric=False)
            .build()
        )
        game = config.create(seed=42)
        game.reset(seed=123)

        assert game.width == 21
        assert game.height == 15
        assert len(game.cheese_positions()) > 0

    def test_preset_symmetric_reset(self):
        """Test that preset games reset correctly."""
        game = GameConfig.preset("medium").create(seed=42)
        game.reset(seed=123)

        # Medium preset is symmetric - check cheese
        cheese = game.cheese_positions()
        cheese_set = {(c.x, c.y) for c in cheese}
        width, height = game.width, game.height

        for c in cheese:
            sym_x = width - 1 - c.x
            sym_y = height - 1 - c.y
            assert (sym_x, sym_y) in cheese_set or (c.x == sym_x and c.y == sym_y)

    def test_preset_asymmetric_reset(self):
        """Test that asymmetric preset resets correctly."""
        game = GameConfig.preset("asymmetric").create(seed=42)
        game.reset(seed=123)

        assert game.width == 21
        assert game.height == 15
        assert len(game.cheese_positions()) > 0


class TestGetValidMoves:
    """Test the get_valid_moves() method.

    Note: Returns list of integers matching Direction enum values:
    UP=0, RIGHT=1, DOWN=2, LEFT=3
    """

    def test_corner_position_bottom_left(self):
        """Test that corner positions have limited valid moves."""
        from pyrat_engine import Direction

        game = GameConfig.preset("open").create(seed=42)
        valid = game.get_valid_moves((0, 0))

        assert Direction.UP in valid
        assert Direction.RIGHT in valid
        assert Direction.DOWN not in valid
        assert Direction.LEFT not in valid

    def test_corner_position_top_right(self):
        """Test top-right corner has limited valid moves."""
        from pyrat_engine import Direction

        game = GameConfig.preset("open").create(seed=42)
        valid = game.get_valid_moves((game.width - 1, game.height - 1))

        assert Direction.DOWN in valid
        assert Direction.LEFT in valid
        assert Direction.UP not in valid
        assert Direction.RIGHT not in valid

    def test_center_position_no_walls(self):
        """Test that center position in empty maze has all 4 moves."""
        from pyrat_engine import Direction

        game = GameConfig.preset("open").create(seed=42)
        center_x = game.width // 2
        center_y = game.height // 2
        valid = game.get_valid_moves((center_x, center_y))

        assert len(valid) == 4
        assert Direction.UP in valid
        assert Direction.DOWN in valid
        assert Direction.LEFT in valid
        assert Direction.RIGHT in valid

    def test_position_with_wall(self):
        """Test that walls block moves."""
        from pyrat_engine import Direction

        config = (
            GameBuilder(5, 5)
            .with_custom_maze(walls=[((0, 0), (1, 0)), ((4, 4), (3, 4))])
            .with_corner_positions()
            .with_custom_cheese([(2, 2)])
            .build()
        )
        game = config.create()

        valid = game.get_valid_moves((0, 0))

        assert Direction.UP in valid
        assert Direction.RIGHT not in valid
        assert Direction.DOWN not in valid
        assert Direction.LEFT not in valid

    def test_out_of_bounds_raises_error(self):
        """Test that out-of-bounds positions raise ValueError."""
        game = GameConfig.preset("tiny").create(seed=42)

        with pytest.raises(ValueError, match="outside board bounds"):
            game.get_valid_moves((100, 100))

    def test_accepts_coordinates_object(self):
        """Test that get_valid_moves accepts Coordinates objects."""
        from pyrat_engine import Coordinates

        game = GameConfig.preset("open").create(seed=42)
        pos = Coordinates(0, 0)
        valid = game.get_valid_moves(pos)

        valid_tuple = game.get_valid_moves((0, 0))
        assert set(valid) == set(valid_tuple)

    def test_returns_direction_compatible_values(self):
        """Test that returned values can be used as Direction enum."""
        from pyrat_engine import Direction

        game = GameConfig.preset("open").create(seed=42)
        valid = game.get_valid_moves((5, 5))

        for v in valid:
            direction = Direction(v)
            assert direction in [
                Direction.UP,
                Direction.RIGHT,
                Direction.DOWN,
                Direction.LEFT,
            ]


class TestEffectiveActions:
    """Test the effective_actions() methods for MCTS action equivalence.

    These methods return [u8; 5] where result[action] = effective_action.
    Direction values: UP=0, RIGHT=1, DOWN=2, LEFT=3, STAY=4
    """

    def test_corner_position_bottom_left(self):
        """Test corner position where DOWN and LEFT are blocked."""
        from pyrat_engine import Direction

        game = GameConfig.preset("open").create(seed=42)
        result = game.effective_actions((0, 0))

        assert result[Direction.UP] == Direction.UP
        assert result[Direction.RIGHT] == Direction.RIGHT
        assert result[Direction.DOWN] == Direction.STAY
        assert result[Direction.LEFT] == Direction.STAY
        assert result[Direction.STAY] == Direction.STAY

    def test_corner_position_top_right(self):
        """Test corner position where UP and RIGHT are blocked."""
        from pyrat_engine import Direction

        game = GameConfig.preset("open").create(seed=42)
        result = game.effective_actions((game.width - 1, game.height - 1))

        assert result[Direction.UP] == Direction.STAY
        assert result[Direction.RIGHT] == Direction.STAY
        assert result[Direction.DOWN] == Direction.DOWN
        assert result[Direction.LEFT] == Direction.LEFT
        assert result[Direction.STAY] == Direction.STAY

    def test_center_position_all_valid(self):
        """Test center position in empty maze has all moves valid."""
        from pyrat_engine import Direction

        game = GameConfig.preset("open").create(seed=42)
        center_x = game.width // 2
        center_y = game.height // 2
        result = game.effective_actions((center_x, center_y))

        assert result[Direction.UP] == Direction.UP
        assert result[Direction.RIGHT] == Direction.RIGHT
        assert result[Direction.DOWN] == Direction.DOWN
        assert result[Direction.LEFT] == Direction.LEFT
        assert result[Direction.STAY] == Direction.STAY

    def test_position_with_wall(self):
        """Test that walls cause actions to map to STAY."""
        from pyrat_engine import Direction

        config = (
            GameBuilder(5, 5)
            .with_custom_maze(walls=[((0, 0), (1, 0)), ((4, 4), (3, 4))])
            .with_corner_positions()
            .with_custom_cheese([(2, 2)])
            .build()
        )
        game = config.create()

        result = game.effective_actions((0, 0))

        assert result[Direction.UP] == Direction.UP
        assert result[Direction.RIGHT] == Direction.STAY
        assert result[Direction.DOWN] == Direction.STAY
        assert result[Direction.LEFT] == Direction.STAY
        assert result[Direction.STAY] == Direction.STAY

    def test_player_in_mud_all_stay(self):
        """Test that player in mud has all actions map to STAY."""
        from pyrat_engine import Direction

        config = (
            GameBuilder(5, 5)
            .with_custom_maze(walls=[], mud=[((1, 0), (1, 1), 3)])
            .with_custom_positions((1, 0), (3, 4))
            .with_custom_cheese([(2, 2)])
            .build()
        )
        game = config.create()

        # Before entering mud, normal behavior
        result_before = game.effective_actions_p1()
        assert result_before[Direction.UP] == Direction.UP

        # Move player 1 into mud
        game.step(Direction.UP, Direction.STAY)

        # Now in mud - all actions should map to STAY
        result_in_mud = game.effective_actions_p1()
        assert list(result_in_mud) == [
            Direction.STAY,
            Direction.STAY,
            Direction.STAY,
            Direction.STAY,
            Direction.STAY,
        ]

        # Player 2 is not in mud, should have normal behavior
        result_p2 = game.effective_actions_p2()
        assert result_p2[Direction.DOWN] == Direction.DOWN

    def test_player_not_in_mud_normal_behavior(self):
        """Test that player not in mud uses position-based computation."""
        from pyrat_engine import Direction

        game = GameConfig.preset("open").create(seed=42)

        # Player 1 starts at (0, 0)
        result_p1 = game.effective_actions_p1()
        assert result_p1[Direction.UP] == Direction.UP
        assert result_p1[Direction.RIGHT] == Direction.RIGHT
        assert result_p1[Direction.DOWN] == Direction.STAY
        assert result_p1[Direction.LEFT] == Direction.STAY

        # Player 2 starts at (width-1, height-1)
        result_p2 = game.effective_actions_p2()
        assert result_p2[Direction.UP] == Direction.STAY
        assert result_p2[Direction.RIGHT] == Direction.STAY
        assert result_p2[Direction.DOWN] == Direction.DOWN
        assert result_p2[Direction.LEFT] == Direction.LEFT

    def test_out_of_bounds_raises_error(self):
        """Test that out-of-bounds position raises ValueError."""
        game = GameConfig.preset("tiny").create(seed=42)

        with pytest.raises(ValueError, match="outside board bounds"):
            game.effective_actions((100, 100))

    def test_accepts_coordinates_object(self):
        """Test that effective_actions accepts Coordinates objects."""
        from pyrat_engine import Coordinates

        game = GameConfig.preset("open").create(seed=42)
        pos = Coordinates(0, 0)
        result = game.effective_actions(pos)

        result_tuple = game.effective_actions((0, 0))
        assert list(result) == list(result_tuple)

    def test_return_type_is_list(self):
        """Test that the return type is a list of 5 integers."""
        game = GameConfig.preset("open").create(seed=42)
        result = game.effective_actions((0, 0))

        assert len(result) == 5
        assert all(isinstance(v, int) for v in result)
        assert all(0 <= v <= 4 for v in result)

    def test_consistency_with_get_valid_moves(self):
        """Test that effective_actions is consistent with get_valid_moves."""
        from pyrat_engine import Direction

        game = GameConfig.preset("open").create(seed=42)

        for x in range(game.width):
            for y in range(game.height):
                valid_moves = game.get_valid_moves((x, y))
                effective = game.effective_actions((x, y))

                for move in valid_moves:
                    assert effective[move] == move

                for move in [
                    Direction.UP,
                    Direction.RIGHT,
                    Direction.DOWN,
                    Direction.LEFT,
                ]:
                    if move not in valid_moves:
                        assert effective[move] == Direction.STAY


class TestCopyProtocol:
    """Test Python copy protocol support (copy.copy and copy.deepcopy)."""

    def test_copy_creates_independent_state(self):
        """Test that copy.copy() creates an independent game state."""
        import copy

        game = GameConfig.classic(11, 9, 13).create(seed=42)
        game_copy = copy.copy(game)

        game_copy.step(0, 0)

        assert game.turn == 0
        assert game_copy.turn == 1

    def test_deepcopy_creates_independent_state(self):
        """Test that copy.deepcopy() creates an independent game state."""
        import copy

        game = GameConfig.classic(11, 9, 13).create(seed=42)
        game_copy = copy.deepcopy(game)

        game_copy.step(0, 0)

        assert game.turn == 0
        assert game_copy.turn == 1

    def test_copy_preserves_game_state(self):
        """Test that copy preserves all game state attributes."""
        import copy

        from pyrat_engine import Direction

        game = GameConfig.classic(11, 9, 13).create(seed=42)

        game.step(Direction.UP, Direction.DOWN)
        game.step(Direction.RIGHT, Direction.LEFT)

        game_copy = copy.copy(game)

        assert game_copy.width == game.width
        assert game_copy.height == game.height
        assert game_copy.turn == game.turn
        assert game_copy.max_turns == game.max_turns
        assert game_copy.player1_position == game.player1_position
        assert game_copy.player2_position == game.player2_position
        assert game_copy.player1_score == game.player1_score
        assert game_copy.player2_score == game.player2_score
        assert game_copy.player1_mud_turns == game.player1_mud_turns
        assert game_copy.player2_mud_turns == game.player2_mud_turns
        assert game_copy.cheese_positions() == game.cheese_positions()

    def test_mutations_on_copy_dont_affect_original(self):
        """Test that mutations on copy don't affect the original."""
        import copy

        from pyrat_engine import Direction

        game = GameConfig.classic(11, 9, 13).create(seed=42)
        original_turn = game.turn
        original_p1_pos = game.player1_position
        original_cheese = game.cheese_positions()

        game_copy = copy.copy(game)

        for _ in range(5):
            game_copy.step(Direction.UP, Direction.DOWN)

        assert game.turn == original_turn
        assert game.player1_position == original_p1_pos
        assert game.cheese_positions() == original_cheese

    def test_copy_for_mcts_simulation(self):
        """Test the MCTS use case: simulate on copy, original unchanged."""
        import copy

        from pyrat_engine import Direction

        game = GameConfig.classic(11, 9, 13).create(seed=42)

        simulator = copy.deepcopy(game)

        while not simulator.step(Direction.UP, Direction.DOWN)[0]:
            if simulator.turn >= simulator.max_turns:
                break

        assert game.turn == 0
        assert simulator.turn > 0

    def test_copy_with_preset(self):
        """Test copy works with preset-created games."""
        import copy

        game = GameConfig.preset("small").create(seed=42)
        game_copy = copy.copy(game)

        assert game_copy.width == game.width
        assert game_copy.height == game.height
        assert game_copy.max_turns == game.max_turns
