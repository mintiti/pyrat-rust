"""Tests for the new module structure of pyrat_engine.core.

This tests that the module reorganization works correctly:
- The compiled _core module exists and has submodules
- The Python wrapper modules provide the expected API
- All classes can be imported from their new locations
- The submodule structure works as expected
"""


class TestModuleStructure:
    """Test the module structure and organization."""

    def test_core_module_exists(self):
        """Test that the core module can be imported."""
        import pyrat_engine.core

        assert pyrat_engine.core is not None

    def test_compiled_module_exists(self):
        """Test that the compiled _core module exists."""
        import pyrat_engine._core

        assert pyrat_engine._core is not None

    def test_core_has_submodules(self):
        """Test that core module has the expected submodules as attributes."""
        from pyrat_engine import core

        assert hasattr(core, "types")
        assert hasattr(core, "game")
        assert hasattr(core, "observation")
        assert hasattr(core, "builder")

    def test_types_submodule_imports(self):
        """Test importing from the types submodule."""
        from pyrat_engine.core.types import Coordinates, Direction, Mud, Wall

        # Test that classes exist
        assert Coordinates is not None
        assert Direction is not None
        assert Wall is not None
        assert Mud is not None

    def test_game_submodule_imports(self):
        """Test importing from the game submodule."""
        from pyrat_engine.core.game import GameState, MoveUndo

        assert GameState is not None
        assert MoveUndo is not None

    def test_observation_submodule_imports(self):
        """Test importing from the observation submodule."""
        from pyrat_engine.core.observation import GameObservation, ObservationHandler

        assert GameObservation is not None
        assert ObservationHandler is not None

    def test_builder_submodule_imports(self):
        """Test importing from the builder submodule."""
        from pyrat_engine.core.builder import GameConfigBuilder

        assert GameConfigBuilder is not None

    def test_core_level_imports(self):
        """Test that commonly used classes are available at the core level."""
        from pyrat_engine.core import (
            Coordinates,
            Direction,
            GameConfigBuilder,
            GameObservation,
            GameState,
            MoveUndo,
            Mud,
            ObservationHandler,
            Wall,
        )

        # All should be importable
        assert all(
            [
                Coordinates,
                Direction,
                Wall,
                Mud,
                GameState,
                MoveUndo,
                GameObservation,
                ObservationHandler,
                GameConfigBuilder,
            ]
        )

    def test_backward_compatibility_names(self):
        """Test that the Py-prefixed names are still available."""
        from pyrat_engine.core.builder import GameConfigBuilder, PyGameConfigBuilder

        # These should be aliases to the cleaner names
        from pyrat_engine.core.game import GameState, MoveUndo, PyGameState, PyMoveUndo
        from pyrat_engine.core.observation import (
            GameObservation,
            ObservationHandler,
            PyGameObservation,
            PyObservationHandler,
        )

        assert PyGameState is GameState
        assert PyMoveUndo is MoveUndo
        assert PyGameObservation is GameObservation
        assert PyObservationHandler is ObservationHandler
        assert PyGameConfigBuilder is GameConfigBuilder


class TestTypesFunctionality:
    """Test that the types module classes work correctly."""

    def test_coordinates_creation(self):
        """Test creating Coordinates objects."""
        from pyrat_engine.core.types import Coordinates

        coord = Coordinates(5, 10)
        assert coord.x == 5  # noqa: PLR2004
        assert coord.y == 10  # noqa: PLR2004

    def test_coordinates_methods(self):
        """Test Coordinates methods."""
        from pyrat_engine.core.types import Coordinates

        coord = Coordinates(5, 5)

        # Test get_neighbor with numeric direction values
        up_neighbor = coord.get_neighbor(0)  # UP = 0
        assert up_neighbor.x == 5  # noqa: PLR2004
        assert up_neighbor.y == 6  # noqa: PLR2004

        # Test manhattan_distance
        other = Coordinates(8, 9)
        distance = coord.manhattan_distance(other)
        assert distance == 7  # noqa: PLR2004  |8-5| + |9-5| = 3 + 4 = 7

    def test_wall_creation(self):
        """Test creating Wall objects."""
        from pyrat_engine.core.types import Coordinates, Wall

        # With Coordinates only (tuples not supported in current implementation)
        wall1 = Wall(Coordinates(0, 0), Coordinates(0, 1))
        assert wall1.pos1.x == 0
        assert wall1.pos1.y == 0
        assert wall1.pos2.x == 0
        assert wall1.pos2.y == 1

    def test_mud_creation(self):
        """Test creating Mud objects."""
        from pyrat_engine.core.types import Coordinates, Mud

        # With Coordinates only (tuples not supported in current implementation)
        mud1 = Mud(Coordinates(0, 0), Coordinates(0, 1), 3)
        assert mud1.value == 3  # noqa: PLR2004

    def test_direction_values(self):
        """Test Direction enum values."""
        from pyrat_engine.core.types import Direction

        # Direction is a Rust enum, check it exists and can be used
        assert Direction is not None
        # In Rust, the enum values are accessed differently
        # For now, just check the type exists


class TestGameStateFunctionality:
    """Test that game module classes work correctly."""

    def test_game_state_creation(self):
        """Test creating GameState objects."""
        from pyrat_engine.core.game import GameState

        # Use odd dimensions to avoid the symmetric maze issue
        game = GameState(width=11, height=11)
        assert game.width == 11  # noqa: PLR2004
        assert game.height == 11  # noqa: PLR2004

    def test_game_state_preset(self):
        """Test creating GameState from preset."""
        from pyrat_engine.core.game import GameState

        game = GameState.create_preset("tiny", seed=42)
        assert game.width == 11  # noqa: PLR2004
        assert game.height == 9  # noqa: PLR2004
        assert game.max_turns == 150  # noqa: PLR2004

    def test_game_state_properties(self):
        """Test GameState properties return correct types."""
        from pyrat_engine.core.game import GameState
        from pyrat_engine.core.types import Coordinates

        # Use odd dimensions to avoid the symmetric maze issue
        game = GameState(width=11, height=11, seed=42)

        # Position properties should return Coordinates
        pos1 = game.player1_position
        assert isinstance(pos1, Coordinates)
        assert hasattr(pos1, "x")
        assert hasattr(pos1, "y")

        pos2 = game.player2_position
        assert isinstance(pos2, Coordinates)

        # Cheese positions should be a list of Coordinates
        cheese = game.cheese_positions()
        assert isinstance(cheese, list)
        if cheese:  # If there's any cheese
            assert isinstance(cheese[0], Coordinates)


class TestHighLevelAPI:
    """Test that the high-level API still works with the new structure."""

    def test_pyrat_import(self):
        """Test that PyRat can still be imported and used."""
        from pyrat_engine import PyRat

        game = PyRat(width=15, height=15)
        width, height = game.dimensions
        assert width == 15  # noqa: PLR2004
        assert height == 15  # noqa: PLR2004

    def test_env_import(self):
        """Test that the PettingZoo environment still works."""
        from pyrat_engine.env import PyRatEnv

        # Use odd dimensions to avoid the symmetric maze issue
        env = PyRatEnv(width=11, height=11)
        assert env is not None

        # Test reset works
        obs, info = env.reset(seed=42)
        assert obs is not None
        assert info is not None
