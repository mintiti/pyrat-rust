"""Tests for the PettingZoo environment wrapper."""

import random

import numpy as np
from pyrat_engine import Direction, GameBuilder, GameConfig
from pyrat_engine.env import PyRatEnv

TEST_GAME_WIDTH = 5
TEST_GAME_HEIGHT = 5
TEST_CHEESE_COUNT = 3


def _test_config():
    return GameConfig.classic(TEST_GAME_WIDTH, TEST_GAME_HEIGHT, TEST_CHEESE_COUNT)


def test_env_initialization() -> None:
    """Test environment initialization."""
    env = PyRatEnv(_test_config())

    # Check spaces
    assert len(env.possible_agents) == 2  # noqa: PLR2004
    assert "player_1" in env.possible_agents
    assert "player_2" in env.possible_agents

    # Check action spaces
    assert env.action_space["player_1"].n == 5  # noqa: PLR2004
    assert env.action_space["player_2"].n == 5  # noqa: PLR2004

    # Check observation spaces
    for agent in env.possible_agents:
        obs_space = env.observation_space[agent]
        assert "player_position" in obs_space.spaces
        assert "cheese_matrix" in obs_space.spaces
        assert "movement_matrix" in obs_space.spaces


def test_env_reset() -> None:
    """Test environment reset."""
    env = PyRatEnv(_test_config())

    observations, infos = env.reset(seed=42)

    # Check observation structure
    for agent in env.possible_agents:
        obs = observations[agent]
        # Now returns Coordinates object
        assert hasattr(obs.player_position, "x")
        assert hasattr(obs.player_position, "y")
        assert isinstance(obs.cheese_matrix, np.ndarray)
        assert isinstance(obs.movement_matrix, np.ndarray)

        # Check array shapes
        assert obs.cheese_matrix.shape == (TEST_GAME_HEIGHT, TEST_GAME_WIDTH)
        assert obs.movement_matrix.shape == (TEST_GAME_HEIGHT, TEST_GAME_WIDTH, 4)


def test_env_step() -> None:
    """Test environment step."""
    env = PyRatEnv(_test_config())
    env.reset(seed=42)

    actions = {
        "player_1": Direction.RIGHT,
        "player_2": Direction.LEFT,
    }

    observations, rewards, terminations, truncations, infos = env.step(actions)

    # Check return types
    assert isinstance(observations, dict)
    assert isinstance(rewards, dict)
    assert isinstance(terminations, dict)
    assert isinstance(truncations, dict)
    assert isinstance(infos, dict)

    # Check observation updates
    for agent in env.possible_agents:
        obs = observations[agent]
        assert 0 <= obs.player_position.x < TEST_GAME_WIDTH
        assert 0 <= obs.player_position.y < TEST_GAME_HEIGHT


def test_env_reset_uses_the_game_owned_observation_state() -> None:
    """Reset observations match the game's regenerated topology and cheese."""
    config = GameConfig.classic(7, 5, 5)
    env = PyRatEnv(config, seed=42)

    observations, _ = env.reset(seed=123)

    for agent, is_player_one in (("player_1", True), ("player_2", False)):
        expected = env.game.get_observation(is_player_one)
        actual = observations[agent]
        np.testing.assert_array_equal(actual.cheese_matrix, expected.cheese_matrix)
        np.testing.assert_array_equal(actual.movement_matrix, expected.movement_matrix)
        assert actual.cheese_matrix.shape == (7, 5)
        assert actual.movement_matrix.shape == (7, 5, 4)
        assert actual.cheese_matrix.dtype == np.uint8
        assert actual.movement_matrix.dtype == np.int8

    np.testing.assert_array_equal(
        observations["player_1"].cheese_matrix,
        observations["player_2"].cheese_matrix,
    )
    np.testing.assert_array_equal(
        observations["player_1"].movement_matrix,
        observations["player_2"].movement_matrix,
    )


def test_env_can_reuse_one_maze_across_resets() -> None:
    """Supplying a maze fixes topology while seeds still select dynamic state."""
    config = GameConfig.classic(7, 5, 5)
    maze = config.generate_maze(seed=500)
    env = PyRatEnv(config, seed=71, maze=maze)
    initial_movement = env.game.get_observation(True).movement_matrix.copy()

    observations, _ = env.reset(seed=72)
    expected = config.create_with_maze(maze, seed=72)

    np.testing.assert_array_equal(
        observations["player_1"].movement_matrix,
        initial_movement,
    )
    assert env.game.state_hash == expected.state_hash
    assert env.game.player1_position == expected.player1_position
    assert env.game.player2_position == expected.player2_position
    assert env.game.cheese_positions() == expected.cheese_positions()


def test_env_step_updates_collected_cheese() -> None:
    """The environment observes cheese removed by its game step."""
    config = (
        GameBuilder(3, 3)
        .with_open_maze()
        .with_custom_positions((0, 0), (2, 2))
        .with_custom_cheese([(1, 0), (0, 2)])
        .build()
    )
    env = PyRatEnv(config)
    env.reset()

    observations, rewards, *_ = env.step(
        {
            "player_1": Direction.RIGHT,
            "player_2": Direction.STAY,
        }
    )

    assert observations["player_1"].cheese_matrix[1, 0] == 0
    assert observations["player_2"].cheese_matrix[1, 0] == 0
    assert rewards == {"player_1": 1.0, "player_2": -1.0}


def test_env_step_keeps_zero_sum_policy_for_shared_cheese() -> None:
    """Equal simultaneous score changes produce zero reward for both players."""
    config = (
        GameBuilder(3, 2)
        .with_open_maze()
        .with_custom_positions((0, 0), (2, 0))
        .with_custom_cheese([(1, 0)])
        .build()
    )
    env = PyRatEnv(config)
    env.reset()

    observations, rewards, terminations, truncations, infos = env.step(
        {
            "player_1": Direction.RIGHT,
            "player_2": Direction.LEFT,
        }
    )

    assert rewards == {"player_1": 0.0, "player_2": 0.0}
    assert terminations == {"player_1": True, "player_2": True}
    assert truncations == {"player_1": False, "player_2": False}
    assert infos == {}
    shared_score = 0.5
    assert observations["player_1"].player_score == shared_score
    assert observations["player_2"].player_score == shared_score
    assert not observations["player_1"].cheese_matrix.any()


def test_env_symmetry() -> None:
    """Test symmetric observations between players."""
    env = PyRatEnv(_test_config())
    obs, _ = env.reset(seed=42)

    # Player 2's view should be symmetric to player 1's
    p1_cheese = obs["player_1"].cheese_matrix
    p2_cheese = obs["player_2"].cheese_matrix

    # Matrices should be symmetric around center
    assert np.array_equal(p1_cheese, np.flip(np.flip(p2_cheese, 0), 1))


def test_random_gameplay() -> None:
    """Test environment with random moves until termination."""
    env = PyRatEnv(_test_config())
    obs, _ = env.reset(seed=42)

    terminated = truncated = False
    while not (terminated or truncated):
        # Make random moves for both players
        actions = {
            "player_1": random.randint(
                0, 4
            ),  # 0-4 covers all directions including STAY
            "player_2": random.randint(0, 4),
        }

        obs, rewards, terminations, truncations, infos = env.step(actions)
        terminated = any(terminations.values())
        truncated = any(truncations.values())

    # Basic assertions to ensure game ended properly
    assert terminated or truncated


def test_custom_maze() -> None:
    """Test environment with custom maze configuration."""
    game_width = 3
    game_height = 3
    config = (
        GameBuilder(game_width, game_height)
        .with_custom_maze(
            walls=[((0, 0), (0, 1)), ((1, 1), (2, 1))],
            mud=[((1, 0), (1, 1), 2)],
        )
        .with_custom_positions((0, 0), (2, 2))
        .with_custom_cheese([(1, 1)])
        .build()
    )
    game = config.create()

    # Verify maze configuration
    assert game.width == game_width
    assert game.height == game_height
    assert len(game.cheese_positions()) == 1
    assert game.player1_position.x == 0
    assert game.player1_position.y == 0
    assert game.player2_position.x == 2  # noqa: PLR2004
    assert game.player2_position.y == 2  # noqa: PLR2004

    # Verify movement matrix values
    # Matrix shape: (width, height, 4), dir: UP=0, RIGHT=1, DOWN=2, LEFT=3
    # Values: -1 = wall/boundary, 0 = open, N>0 = mud turns
    obs = game.get_observation(is_player_one=True)
    mm = np.array(obs.movement_matrix)

    # (0,0): UP→wall(-1), RIGHT→open(0), DOWN→boundary(-1), LEFT→boundary(-1)
    assert mm[0, 0, 0] == -1, "wall between (0,0) and (0,1)"
    assert mm[0, 0, 1] == 0, "open path (0,0)→(1,0)"
    assert mm[0, 0, 2] == -1, "boundary below (0,0)"
    assert mm[0, 0, 3] == -1, "boundary left of (0,0)"

    # (1,0): UP→mud(2), RIGHT→open(0), DOWN→boundary(-1), LEFT→open(0)
    assert mm[1, 0, 0] == 2, "mud between (1,0) and (1,1)"  # noqa: PLR2004
    assert mm[1, 0, 2] == -1, "boundary below (1,0)"

    # (1,1): UP→open(0), RIGHT→wall(-1), DOWN→mud(2), LEFT→open(0)
    assert mm[1, 1, 1] == -1, "wall between (1,1) and (2,1)"
    assert mm[1, 1, 2] == 2, "mud between (1,1) and (1,0)"  # noqa: PLR2004
