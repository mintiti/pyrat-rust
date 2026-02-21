from pyrat_engine import Direction, GameConfig


def test_import() -> None:
    # Create game
    game = GameConfig.classic(15, 15, 21).create()
    print(game.cheese_positions())

    # Make some moves with undo capability
    undo1 = game.make_move(Direction.RIGHT, Direction.LEFT)
    print(f"After move 1: P1 at {game.player1_position}, P2 at {game.player2_position}")

    undo2 = game.make_move(Direction.UP, Direction.DOWN)
    print(f"After move 2: P1 at {game.player1_position}, P2 at {game.player2_position}")

    # Undo moves in reverse order
    game.unmake_move(undo2)
    print(
        f"Undid move 2: P1 back to {game.player1_position}, P2 back to {game.player2_position}"
    )

    game.unmake_move(undo1)
    print(
        f"Undid move 1: P1 back to {game.player1_position}, P2 back to {game.player2_position}"
    )
