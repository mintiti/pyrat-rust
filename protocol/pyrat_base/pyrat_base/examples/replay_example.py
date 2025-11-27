#!/usr/bin/env python3
"""Example of using the PyRat replay system."""

from pyrat_engine.core.types import Direction

from pyrat_base.replay import (
    InitialState,
    Move,
    Replay,
    ReplayMetadata,
    ReplayPlayer,
    ReplayReader,
    ReplayWriter,
    StreamingReplayWriter,
)


def create_replay_example() -> None:
    """Create a simple replay and save it."""
    print("Creating a simple replay...")

    # Create metadata
    metadata = ReplayMetadata(
        event="Example Game",
        site="PyRat Lab",
        date="2025.01.15",
        rat="GreedyBot",
        python="RandomBot",
        maze_height=10,
        maze_width=10,
        time_control="100+3000+1000",
    )

    # Create initial state
    initial_state = InitialState(
        width=10,
        height=10,
        walls=[((5, 5), (5, 6)), ((6, 5), (6, 6))],  # Small box of walls
        mud=[(((3, 3), (4, 3)), 2)],  # 2-turn mud
        cheese=[(1, 1), (8, 8), (5, 5)],
        rat_position=(9, 9),
        python_position=(0, 0),
    )

    # Create some moves
    moves = [
        Move(1, Direction.LEFT, Direction.RIGHT, 50, 60, "Opening moves"),
        Move(2, Direction.DOWN, Direction.UP, 45, 55),
        Move(3, Direction.LEFT, Direction.RIGHT, 48, 52, "Approaching cheese"),
    ]

    # Create replay
    replay = Replay(
        metadata=metadata,
        initial_state=initial_state,
        moves=moves,
        preprocessing_done=True,
    )

    # Write to file
    writer = ReplayWriter()
    writer.write_file(replay, "example_game.pyrat")
    print("Saved replay to example_game.pyrat")


def read_replay_example() -> None:
    """Read and analyze a replay."""
    print("\nReading replay...")

    reader = ReplayReader()
    replay = reader.read_file("example_game.pyrat")

    print(f"Event: {replay.metadata.event}")
    print(f"Players: {replay.metadata.rat} vs {replay.metadata.python}")
    print(f"Board size: {replay.metadata.maze_width}x{replay.metadata.maze_height}")
    print(f"Number of moves: {len(replay.moves)}")

    # Show moves
    print("\nMoves:")
    for move in replay.moves:
        rat_name = (
            Direction(move.rat_move).name
            if isinstance(move.rat_move, int)
            else move.rat_move
        )
        python_name = (
            Direction(move.python_move).name
            if isinstance(move.python_move, int)
            else move.python_move
        )
        print(f"  Turn {move.turn}: Rat {rat_name}, Python {python_name}")
        if move.comment:
            print(f"    Comment: {move.comment}")


def streaming_example() -> None:
    """Example of writing a replay during gameplay."""
    print("\nStreaming replay example...")

    with StreamingReplayWriter("streaming_game.pyrat") as writer:
        # Write metadata at game start
        metadata = ReplayMetadata(
            event="Live Game", rat="Bot1", python="Bot2", maze_height=5, maze_width=5
        )
        writer.write_metadata(metadata)

        # Write initial state
        initial_state = InitialState(
            width=5,
            height=5,
            cheese=[(2, 2)],
            rat_position=(4, 4),
            python_position=(0, 0),
        )
        writer.write_initial_state(initial_state)

        # During game, write moves as they happen
        writer.write_preprocessing_done()

        # Simulate some moves
        for turn in range(1, 4):
            move = Move(
                turn=turn,
                rat_move=Direction.LEFT if turn % 2 else Direction.UP,
                python_move=Direction.RIGHT if turn % 2 else Direction.DOWN,
                rat_time_ms=50 + turn,
                python_time_ms=60 + turn,
            )
            writer.write_move(move)
            print(f"  Wrote move {turn}")

    print("Saved streaming replay to streaming_game.pyrat")


def replay_player_example() -> None:
    """Example of using ReplayPlayer to step through a game."""
    print("\nReplay player example...")

    # Read the replay we created
    reader = ReplayReader()
    replay = reader.read_file("example_game.pyrat")

    # Create player
    player = ReplayPlayer(replay)

    print("Stepping through the game:")
    while not player.is_finished():
        result = player.step_forward()
        if result:
            move = player.get_move_at_current_turn()
            if move:
                print(
                    f"  Turn {move.turn}: Scores {result.p1_score:.1f}-{result.p2_score:.1f}"
                )
                if move.comment:
                    print(f"    {move.comment}")

    print(
        f"Final scores: {player.game.player1_score:.1f}-{player.game.player2_score:.1f}"
    )


def main() -> None:
    """Run all examples."""
    create_replay_example()
    read_replay_example()
    streaming_example()
    replay_player_example()


if __name__ == "__main__":
    main()
