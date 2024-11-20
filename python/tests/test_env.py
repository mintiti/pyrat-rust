"""Tests for the PettingZoo environment wrapper."""

import random

import numpy as np
from pyrat import Direction
from pyrat.env import PyRatEnv

TEST_GAME_WIDTH = 5
TEST_GAME_HEIGHT = 5
TEST_CHEESE_COUNT = 3


def test_env_initialization() -> None:
    """Test environment initialization."""
    env = PyRatEnv(
        width=TEST_GAME_WIDTH, height=TEST_GAME_HEIGHT, cheese_count=TEST_CHEESE_COUNT
    )

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
    env = PyRatEnv(
        width=TEST_GAME_WIDTH, height=TEST_GAME_HEIGHT, cheese_count=TEST_CHEESE_COUNT
    )

    observations, infos = env.reset(seed=42)

    # Check observation structure
    for agent in env.possible_agents:
        obs = observations[agent]
        assert isinstance(obs.player_position, tuple)
        assert isinstance(obs.cheese_matrix, np.ndarray)
        assert isinstance(obs.movement_matrix, np.ndarray)

        # Check array shapes
        assert obs.cheese_matrix.shape == (TEST_GAME_HEIGHT, TEST_GAME_WIDTH)
        assert obs.movement_matrix.shape == (TEST_GAME_HEIGHT, TEST_GAME_WIDTH, 4)


def test_env_step() -> None:
    """Test environment step."""
    env = PyRatEnv(
        width=TEST_GAME_WIDTH, height=TEST_GAME_HEIGHT, cheese_count=TEST_CHEESE_COUNT
    )
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
        assert 0 <= obs.player_position[0] < TEST_GAME_WIDTH
        assert 0 <= obs.player_position[1] < TEST_GAME_HEIGHT


def test_env_symmetry() -> None:
    """Test symmetric observations between players."""
    env = PyRatEnv(
        width=TEST_GAME_WIDTH,
        height=TEST_GAME_HEIGHT,
        cheese_count=TEST_CHEESE_COUNT,
        symmetric=True,
    )
    obs, _ = env.reset(seed=42)

    # Player 2's view should be symmetric to player 1's
    p1_cheese = obs["player_1"].cheese_matrix
    p2_cheese = obs["player_2"].cheese_matrix

    # Matrices should be symmetric around center
    assert np.array_equal(p1_cheese, np.flip(np.flip(p2_cheese, 0), 1))


def test_random_gameplay() -> None:
    """Test environment with random moves until termination."""
    env = PyRatEnv(
        width=TEST_GAME_WIDTH, height=TEST_GAME_HEIGHT, cheese_count=TEST_CHEESE_COUNT
    )
    obs, _ = env.reset(seed=42)

    terminated = truncated = False
    while not (terminated or truncated):
        # Make random moves for both players
        actions = {
            "player_1": Direction(
                random.randint(0, 4)
            ),  # 0-4 covers all directions including STAY
            "player_2": Direction(random.randint(0, 4)),
        }

        obs, rewards, terminations, truncations, infos = env.step(actions)
        terminated = any(terminations.values())
        truncated = any(truncations.values())

    # Basic assertions to ensure game ended properly
    assert terminated or truncated
