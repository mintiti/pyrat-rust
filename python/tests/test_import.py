from pyrat import PyRatEnv


def test_import() -> None:
    """Tests that the PyRatEnv imports correctly."""
    env = PyRatEnv()
    obs = env.reset()
    actions = [0, 1]  # Up for player 1, Right for player 2
    done = False
    while not done:
        obs, rewards, done, truncated, info = env.step(actions)
