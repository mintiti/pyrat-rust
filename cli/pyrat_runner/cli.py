"""Command-line interface for PyRat game runner."""

import argparse
import sys
from pathlib import Path

from .game_runner import GameRunner


def main():
    """Main entry point for pyrat-game command."""
    parser = argparse.ArgumentParser(
        description="Run and visualize PyRat games between two AI processes",
        formatter_class=argparse.RawDescriptionHelpFormatter,
        epilog="""
Examples:
  # Run game with default settings
  pyrat-game bot1.py bot2.py

  # Run with custom maze size
  pyrat-game --width 31 --height 21 bot1.py bot2.py

  # Run with custom timeouts and faster display
  pyrat-game --timeout 2.0 --delay 0.1 bot1.py bot2.py

  # Run with specific seed for reproducibility
  pyrat-game --seed 42 bot1.py bot2.py
        """,
    )

    # Required arguments
    parser.add_argument(
        "rat_ai",
        type=str,
        help="Path to Rat AI script (starts at top-right)",
    )

    parser.add_argument(
        "python_ai",
        type=str,
        help="Path to Python AI script (starts at bottom-left)",
    )

    # Maze configuration
    maze_group = parser.add_argument_group("maze configuration")
    maze_group.add_argument(
        "--width",
        type=int,
        default=21,
        help="Maze width (default: 21)",
    )
    maze_group.add_argument(
        "--height",
        type=int,
        default=15,
        help="Maze height (default: 15)",
    )
    maze_group.add_argument(
        "--cheese",
        type=int,
        default=41,
        help="Number of cheese pieces (default: 41)",
    )
    maze_group.add_argument(
        "--seed",
        type=int,
        default=None,
        help="Random seed for maze generation (default: random)",
    )

    # Time controls
    time_group = parser.add_argument_group("time controls")
    time_group.add_argument(
        "--timeout",
        type=float,
        default=1.0,
        help="AI response timeout in seconds (default: 1.0)",
    )
    time_group.add_argument(
        "--preprocessing",
        type=float,
        default=3.0,
        help="Preprocessing time in seconds (default: 3.0)",
    )

    # Display options
    display_group = parser.add_argument_group("display options")
    display_group.add_argument(
        "--delay",
        type=float,
        default=0.3,
        help="Delay between turns in seconds (default: 0.3)",
    )

    # Logging options
    logging_group = parser.add_argument_group("logging")
    logging_group.add_argument(
        "--log-dir",
        type=str,
        default=None,
        help=(
            "Directory to write logs (protocol, stderr, events). "
            "If unset, logging is disabled."
        ),
    )

    args = parser.parse_args()

    # Validate AI script paths
    rat_path = Path(args.rat_ai)
    python_path = Path(args.python_ai)

    if not rat_path.exists():
        print(f"Error: Rat AI script not found: {rat_path}", file=sys.stderr)
        return 1

    if not python_path.exists():
        print(f"Error: Python AI script not found: {python_path}", file=sys.stderr)
        return 1

    # Create and run game
    try:
        runner = GameRunner(
            rat_script=str(rat_path.absolute()),
            python_script=str(python_path.absolute()),
            width=args.width,
            height=args.height,
            cheese_count=args.cheese,
            seed=args.seed,
            turn_timeout=args.timeout,
            preprocessing_timeout=args.preprocessing,
            display_delay=args.delay,
            log_dir=args.log_dir,
        )

        success = runner.run()
        return 0 if success else 1

    except KeyboardInterrupt:
        print("\n\nGame interrupted by user", file=sys.stderr)
        return 130
    except Exception as e:
        print(f"\nUnexpected error: {e}", file=sys.stderr)
        import traceback

        traceback.print_exc()
        return 1


if __name__ == "__main__":
    sys.exit(main())
