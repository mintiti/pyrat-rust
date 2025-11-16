"""Demonstrate the new PyRat API with presets."""

from pyrat_engine._rust import PyGameState
from pyrat_engine.game import Direction, PyRat


def demo_presets():
    """Show how to use different game presets."""
    print("PyRat API Demo - Presets\n")

    # 1. Using the enhanced main constructor
    print("1. Main constructor with max_turns:")
    game = PyRat(width=15, height=11, max_turns=200)
    print(
        f"   Created {game.dimensions[0]}x{game.dimensions[1]} game with {game.max_turns} max turns"
    )

    # 2. Using presets
    print("\n2. Available presets:")
    presets = ["tiny", "small", "default", "large", "huge", "empty", "asymmetric"]

    for preset in presets:
        game_state = PyGameState.create_preset(preset, seed=42)
        print(
            f"   - {preset:12} {game_state.width}x{game_state.height}, "
            f"{len(game_state.cheese_positions()):3} cheese, "
            f"{game_state.max_turns:3} turns"
        )

    # 3. Preset with seed for reproducibility
    print("\n3. Reproducible games with seeds:")
    game1 = PyGameState.create_preset("small", seed=123)
    game2 = PyGameState.create_preset("small", seed=123)
    game3 = PyGameState.create_preset("small", seed=456)

    print(
        f"   Same seed produces same cheese: {game1.cheese_positions() == game2.cheese_positions()}"
    )
    print(
        f"   Different seed produces different cheese: {game1.cheese_positions() == game3.cheese_positions()}"
    )

    # 4. Empty preset for testing
    print("\n4. Empty preset (no walls/mud):")
    empty_game = PyGameState.create_preset("empty")
    print(f"   Mud entries: {len(empty_game.mud_entries())}")
    print("   Can move freely in all directions")

    # 5. Quick game with tiny preset
    print("\n5. Quick game simulation:")
    tiny = PyGameState.create_preset("tiny", seed=42)
    print(f"   Starting game on {tiny.width}x{tiny.height} board")

    # Simulate a few moves
    for i in range(5):
        game_over, collected = tiny.step(Direction.RIGHT.value, Direction.LEFT.value)
        if collected:
            print(f"   Turn {i+1}: Cheese collected at {collected}")
        if game_over:
            print(
                f"   Game over! Final scores: {tiny.player1_score} - {tiny.player2_score}"
            )
            break


if __name__ == "__main__":
    demo_presets()
