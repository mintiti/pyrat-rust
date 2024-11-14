from pyrat import PyRatEnv


def test_import():
    env = PyRatEnv()
    obs = env.reset()
    actions = [0, 1]  # Up for player 1, Right for player 2
    obs, rewards, done, truncated, info = env.step(actions)
    print(obs)