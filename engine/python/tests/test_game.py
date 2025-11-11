"""Tests for PyGameState from the Rust bindings.

This tests the low-level game state implementation including:
- Basic game creation
- Constructor parameters
- Preset configurations
- Custom game creation methods
"""
# ruff: noqa: PLR2004

import pytest
from pyrat_engine.core.game import GameState as PyGameState
from pyrat_engine.game import PyRat


class TestBasicGameCreation:
    """Test basic game creation."""

    def test_game_creation(self) -> None:
        """Test basic game creation with minimal parameters."""
        game = PyGameState(width=5, height=5, cheese_count=3)
        assert game.width == 5
        assert game.height == 5
        assert len(game.cheese_positions()) == 3

    def test_default_values(self) -> None:
        """Test game creation with default values."""
        game = PyGameState()
        assert game.width == 21
        assert game.height == 15
        assert game.max_turns == 300
        # Default cheese count is 41
        assert len(game.cheese_positions()) == 41


class TestEnhancedConstructor:
    """Test the enhanced main constructor with new parameters."""

    def test_max_turns_parameter(self):
        """Test that max_turns can be set in main constructor."""
        game = PyGameState(max_turns=500)
        assert game.max_turns == 500

    def test_default_max_turns(self):
        """Test that default max_turns is still 300."""
        game = PyGameState()
        assert game.max_turns == 300

    def test_all_parameters(self):
        """Test all parameters work together."""
        game = PyGameState(
            width=15, height=11, cheese_count=21, symmetric=True, seed=42, max_turns=200
        )
        assert game.width == 15
        assert game.height == 11
        assert game.max_turns == 200
        assert len(game.cheese_positions()) == 21


class TestPresets:
    """Test the preset system."""

    def test_all_presets_exist(self):
        """Test that all presets can be created."""
        presets = ["tiny", "small", "default", "large", "huge", "empty", "asymmetric"]

        for preset in presets:
            game = PyGameState.create_preset(preset)
            assert game is not None

    def test_preset_dimensions(self):
        """Test that presets have correct dimensions."""
        expected = {
            "tiny": (11, 9, 13, 150),
            "small": (15, 11, 21, 200),
            "default": (21, 15, 41, 300),
            "large": (31, 21, 85, 400),
            "huge": (41, 31, 165, 500),
            "empty": (21, 15, 41, 300),
            "asymmetric": (21, 15, 41, 300),
        }

        for preset, (width, height, cheese, turns) in expected.items():
            game = PyGameState.create_preset(preset)
            assert game.width == width
            assert game.height == height
            assert game.max_turns == turns
            # Cheese count might vary slightly due to generation
            assert abs(len(game.cheese_positions()) - cheese) <= 2

    def test_preset_with_seed(self):
        """Test that presets with same seed are reproducible."""
        game1 = PyGameState.create_preset("default", seed=42)
        game2 = PyGameState.create_preset("default", seed=42)

        # Check that cheese positions are the same
        cheese1 = set(game1.cheese_positions())
        cheese2 = set(game2.cheese_positions())
        assert cheese1 == cheese2

    def test_empty_preset_has_no_walls(self):
        """Test that empty preset has no walls or mud."""
        game = PyGameState.create_preset("empty")

        # Check walls by seeing if all moves are valid
        # In a maze with no walls, you can move in all 4 directions
        # from any non-edge position
        obs = game.get_observation(True)  # True for player 1
        movement_matrix = obs.movement_matrix

        # Check a center position (should have all 4 moves valid)
        center_x, center_y = game.width // 2, game.height // 2
        # Movement matrix: [x, y, direction]
        # Directions: 0=UP, 1=RIGHT, 2=DOWN, 3=LEFT
        # Values: -1=wall/invalid, 0=free movement, >0=mud
        for direction in range(4):
            assert movement_matrix[center_x][center_y][direction] == 0

    def test_invalid_preset_name(self):
        """Test that invalid preset names raise an error."""
        with pytest.raises(ValueError, match="Unknown preset"):
            PyGameState.create_preset("invalid_preset")


class TestCustomCreationMethods:
    """Test the new custom creation methods."""

    def test_create_from_maze(self):
        """Test creating a game from a specific maze layout."""
        walls = [
            ((0, 0), (0, 1)),  # Wall between (0,0) and (0,1)
            ((1, 1), (2, 1)),  # Wall between (1,1) and (2,1)
        ]

        game = PyGameState.create_from_maze(
            width=5, height=5, walls=walls, seed=42, max_turns=100
        )

        assert game.width == 5
        assert game.height == 5
        assert game.max_turns == 100
        # Should have cheese (13% of 25 = ~3 pieces)
        assert 2 <= len(game.cheese_positions()) <= 4

    def test_create_with_starts(self):
        """Test creating a game with custom starting positions."""
        game = PyGameState.create_with_starts(
            width=15,
            height=11,
            player1_start=(3, 3),
            player2_start=(11, 7),
            preset="small",
            seed=42,
        )

        assert game.width == 15
        assert game.height == 11
        assert game.player1_position.x == 3
        assert game.player1_position.y == 3
        assert game.player2_position.x == 11
        assert game.player2_position.y == 7
        # Should use the preset's max_turns
        assert game.max_turns == 200


class TestPyRatIntegration:
    """Test that the new API works with the high-level PyRat class."""

    def test_pyrat_with_max_turns(self):
        """Test that PyRat class accepts max_turns parameter."""
        game = PyRat(max_turns=500)
        assert game.max_turns == 500

    def test_pyrat_defaults(self):
        """Test that PyRat defaults still work."""
        game = PyRat()
        assert game.max_turns == 300
        assert game.dimensions == (21, 15)

    def test_pyrat_all_parameters(self):
        """Test PyRat with all parameters."""
        game = PyRat(
            width=25,
            height=17,
            cheese_count=50,
            symmetric=False,
            seed=123,
            max_turns=400,
        )
        assert game.dimensions == (25, 17)
        assert game.max_turns == 400
        # Cheese count might vary slightly
        assert 48 <= len(game.cheese_positions) <= 52


class TestBackwardCompatibility:
    """Test that the new API maintains backward compatibility."""

    def test_old_constructor_still_works(self):
        """Test that the old constructor signature still works."""
        # Old way without max_turns
        game = PyGameState(
            width=10, height=10, cheese_count=10, symmetric=True, seed=42
        )
        assert game.width == 10
        assert game.height == 10
        assert game.max_turns == 300  # Default value

    def test_positional_arguments_work(self):
        """Test that positional arguments still work for backward compatibility."""
        # This is how some old code might call it
        game = PyGameState(15, 15, 20, True, 42)
        assert game.width == 15
        assert game.height == 15
        assert len(game.cheese_positions()) == 20
