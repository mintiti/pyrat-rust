from pyrat_engine._rust import PyGameState

TEST_GAME_WIDTH = 5
TEST_GAME_HEIGHT = 5
TEST_CHEESE_COUNT = 3


def test_game_creation() -> None:
    game = PyGameState(
        width=TEST_GAME_WIDTH, height=TEST_GAME_HEIGHT, cheese_count=TEST_CHEESE_COUNT
    )
    assert game.width == TEST_GAME_WIDTH
    assert game.height == TEST_GAME_HEIGHT
    assert len(game.cheese_positions()) == TEST_CHEESE_COUNT
