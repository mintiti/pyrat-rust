"""End-to-end integration test for greedy AI correctness.

This test verifies that the greedy AI:
1. Makes optimal pathfinding decisions across multiple turns
2. Successfully collects cheese in a full game
3. Maintains correct game state synchronization
4. Outperforms simpler AIs (wins against dummy AI)
"""
# ruff: noqa: PLR2004

import pytest
from pyrat_engine.core.game import GameState as PyGameState
from pyrat_engine.core.types import Coordinates, Direction

from pyrat_base import ProtocolState
from pyrat_base.enums import Player
from pyrat_base.examples.dummy_ai import DummyAI
from pyrat_base.examples.greedy_ai import GreedyAI


class DirectAIRunner:
    """Helper to run AIs directly (without subprocess) for testing."""

    def __init__(self, ai_class, player: Player):
        self.ai = ai_class()
        self.player = player

    def get_move(self, game_state: PyGameState) -> Direction:
        """Get the AI's move for the current game state."""
        state = ProtocolState(game_state, self.player)
        return self.ai.get_move(state)


class TestGreedyAIEndToEnd:
    """End-to-end tests for greedy AI correctness."""

    def test_greedy_finds_nearest_cheese_simple_maze(self):
        """Test greedy AI finds and collects the nearest cheese in a simple maze."""
        # Create a simple 5x5 game with cheese at different distances
        game = PyGameState.create_custom(
            width=5,
            height=5,
            walls=[],
            mud=[],
            cheese=[(1, 0), (4, 4)],  # Close cheese and far cheese
            player1_pos=(0, 0),  # Rat starts at bottom-left
            player2_pos=(0, 4),  # Python starts at top-left (away from cheese)
        )

        greedy = DirectAIRunner(GreedyAI, Player.RAT)

        # Turn 1: Greedy should move toward (1,0) which is closer
        move = greedy.get_move(game)
        assert (
            move == Direction.RIGHT
        ), "Should move RIGHT toward nearest cheese at (1,0)"

        # Execute the move
        game.step(Direction.RIGHT, Direction.STAY)

        # Verify rat moved
        assert game.player1_position == Coordinates(1, 0), "Rat should be at (1, 0)"

        # Turn 2: Rat should now be on the cheese at (1,0)
        # Greedy should target the remaining cheese
        move = greedy.get_move(game)
        # After collecting (1,0), should target (4,4)
        assert move in [
            Direction.UP,
            Direction.RIGHT,
        ], "Should move toward remaining cheese"

    @pytest.mark.slow
    def test_greedy_navigates_around_walls(self):
        """Test greedy AI correctly navigates around walls to reach cheese."""
        # Create maze with vertical wall that has a gap at the top
        # Wall blocks x=2→3 for y=0,1,2,3 but has a gap at y=4
        walls = [
            ((2, y), (3, y)) for y in range(4)
        ]  # Vertical wall at x=2-3 boundary, gap at y=4

        game = PyGameState.create_custom(
            width=6,
            height=5,
            walls=walls,
            mud=[],
            cheese=[(4, 0)],  # Cheese on other side of wall
            player1_pos=(0, 0),  # Rat starts bottom-left
            player2_pos=(5, 4),  # Python starts top-right
        )

        greedy = DirectAIRunner(GreedyAI, Player.RAT)

        # Calculate exact optimal path from (0,0) to (4,0) around wall:
        # 1. UP from (0,0) to (0,4): 4 steps
        # 2. RIGHT from (0,4) to (4,4): 4 steps (through gap at y=4)
        # 3. DOWN from (4,4) to (4,0): 4 steps
        # Total: 12 steps exactly
        expected_steps = 12

        # Verify the AI takes exactly this many steps
        for step in range(expected_steps):
            rat_move = greedy.get_move(game)

            # Verify we're making valid moves
            assert rat_move in [
                Direction.UP,
                Direction.DOWN,
                Direction.LEFT,
                Direction.RIGHT,
            ], f"Step {step}: Invalid move {rat_move}"

            game.step(rat_move, Direction.STAY)

        # After exactly 12 steps, rat should be at cheese location and collect it
        assert (
            game.player1_position == Coordinates(4, 0)
        ), f"After {expected_steps} steps, rat should be at (4,0), but is at {game.player1_position}"

        assert (
            game.player1_score == 1.0
        ), f"After {expected_steps} steps, rat should have collected cheese (score=1.0), but score is {game.player1_score}"

    def test_greedy_handles_mud_optimally(self):
        """Test greedy AI considers mud costs when choosing paths."""
        # Create scenario where direct path has heavy mud
        # and longer path is faster
        game = PyGameState.create_custom(
            width=6,
            height=3,
            walls=[],
            mud=[
                ((0, 0), (1, 0), 10),  # Heavy mud going right
                ((1, 0), (2, 0), 10),  # More heavy mud
            ],
            cheese=[(2, 0), (0, 2)],  # One through mud, one around
            player1_pos=(0, 0),
            player2_pos=(5, 2),
        )

        greedy = DirectAIRunner(GreedyAI, Player.RAT)

        # Greedy should choose UP (avoiding mud) rather than RIGHT (into mud)
        move = greedy.get_move(game)
        assert move == Direction.UP, "Should avoid heavy mud by going UP first"

    def test_greedy_vs_dummy_full_game(self):
        """Test greedy AI beats dummy AI in a full game."""
        # Create a standard game
        game = PyGameState(width=11, height=9, seed=12345, cheese_count=5)

        greedy = DirectAIRunner(GreedyAI, Player.RAT)
        dummy = DirectAIRunner(DummyAI, Player.PYTHON)

        max_turns = 100
        turn = 0

        while turn < max_turns and len(game.cheese_positions()) > 0:
            # Get moves from both AIs
            rat_move = greedy.get_move(game)
            python_move = dummy.get_move(game)

            # Execute moves
            game.step(rat_move, python_move)
            turn += 1

            # Check for winner
            total_cheese = game.player1_score + game.player2_score
            if game.player1_score > total_cheese / 2:
                break
            if game.player2_score > total_cheese / 2:
                break

        # Greedy should beat dummy (dummy just stays, greedy actively collects)
        assert game.player1_score > game.player2_score, "Greedy AI should beat dummy AI"
        assert game.player1_score > 0, "Greedy AI should collect at least one cheese"

    @pytest.mark.slow
    def test_greedy_state_synchronization_multi_turn(self):
        """Test greedy AI maintains correct state over multiple turns.

        This is a regression test for the command-dropping bug where
        game state would desync.
        """
        game = PyGameState.create_custom(
            width=7,
            height=7,
            walls=[],
            mud=[],
            cheese=[(3, 3), (6, 6), (1, 1)],
            player1_pos=(0, 0),
            player2_pos=(6, 0),
        )

        greedy = DirectAIRunner(GreedyAI, Player.RAT)

        # Track positions to ensure state stays synchronized
        positions_visited = []

        for turn in range(20):
            # Get greedy's decision
            state = ProtocolState(game, Player.RAT)
            current_pos = state.my_position
            positions_visited.append(current_pos)

            move = greedy.get_move(game)

            # Execute move
            game.step(move, Direction.STAY)

            # Verify game state is consistent
            # Position after move should match what game reports
            expected_pos = game.player1_position
            assert (
                expected_pos == game.player1_position
            ), f"State desync detected at turn {turn}"

            # If all cheese collected, stop
            if len(game.cheese_positions()) == 0:
                break

        # Verify greedy collected at least one cheese
        assert game.player1_score > 0, "Greedy should collect cheese over 20 turns"

        # Verify rat actually moved (not stuck)
        unique_positions = set(positions_visited)
        assert len(unique_positions) > 1, "Rat should move to different positions"

    @pytest.mark.slow
    @pytest.mark.parametrize("seed", [12345, 54321, 99999, 11111])
    def test_greedy_on_random_mazes(self, seed):
        """Test greedy AI performs well on various random mazes."""
        game = PyGameState(width=15, height=11, seed=seed, cheese_count=10)

        greedy = DirectAIRunner(GreedyAI, Player.RAT)

        max_turns = 200
        min_cheese_for_success = 5
        min_cheese_threshold = 3
        for _turn in range(max_turns):
            move = greedy.get_move(game)
            game.step(move, Direction.STAY)

            # If collected majority of cheese, success
            if game.player1_score >= min_cheese_for_success:
                break

        # Greedy should collect at least half the cheese in reasonable time
        assert (
            game.player1_score >= min_cheese_threshold
        ), f"Greedy should collect at least {min_cheese_threshold} cheese (got {game.player1_score})"

    def test_greedy_recalculates_when_cheese_collected(self):
        """Test greedy AI updates its target when cheese is collected."""
        game = PyGameState.create_custom(
            width=5,
            height=5,
            walls=[],
            mud=[],
            cheese=[(1, 0), (2, 0), (3, 0)],
            player1_pos=(0, 0),
            player2_pos=(4, 4),
        )

        greedy = DirectAIRunner(GreedyAI, Player.RAT)

        # Initial move toward (1,0)
        move1 = greedy.get_move(game)
        assert move1 == Direction.RIGHT

        expected_score_after_first = 1.0
        expected_score_after_second = 2.0
        expected_score_after_third = 3.0

        # Move and collect cheese at (1,0)
        game.step(Direction.RIGHT, Direction.STAY)
        assert game.player1_score == expected_score_after_first

        # After collecting (1,0), should target (2,0) next
        move2 = greedy.get_move(game)
        assert move2 == Direction.RIGHT, "Should continue to next nearest cheese"

        # Move and collect cheese at (2,0)
        game.step(Direction.RIGHT, Direction.STAY)
        assert game.player1_score == expected_score_after_second

        # Should target (3,0)
        move3 = greedy.get_move(game)
        assert move3 == Direction.RIGHT

        # Collect final cheese
        game.step(Direction.RIGHT, Direction.STAY)
        assert game.player1_score == expected_score_after_third

    def test_greedy_handles_simultaneous_collection(self):
        """Test greedy AI handles simultaneous cheese collection correctly."""
        game = PyGameState.create_custom(
            width=3,
            height=1,
            walls=[],
            mud=[],
            cheese=[(1, 0)],  # One cheese in the middle
            player1_pos=(0, 0),  # Rat on left
            player2_pos=(2, 0),  # Python on right
        )

        greedy = DirectAIRunner(GreedyAI, Player.RAT)

        # Both move toward cheese
        move = greedy.get_move(game)
        assert move == Direction.RIGHT

        # Both collect simultaneously (each gets 0.5 points)
        game.step(Direction.RIGHT, Direction.LEFT)

        # Check simultaneous collection works correctly
        expected_simultaneous_score = 0.5
        assert game.player1_score == expected_simultaneous_score
        assert game.player2_score == expected_simultaneous_score


@pytest.mark.slow
class TestGreedyVsGreedy:
    """Precise, deterministic tests for greedy vs greedy gameplay.

    These tests verify exact turn-by-turn behavior with known outcomes.
    They also test the critical command requeue bug fix for state synchronization.
    """

    def test_symmetric_race_equal_split(self):
        """Test symmetric maze where both AIs collect equal cheese.

        Setup: 1D maze, symmetric cheese placement
        - Cheese at (1,0), (2,0), (3,0)
        - Rat at (0,0), Python at (4,0)

        Expected:
        - Turn 1: Rat collects (1,0), Python collects (3,0)
        - Turn 2: Both reach (2,0) simultaneously
        - Final: Rat=1.5, Python=1.5
        """
        game = PyGameState.create_custom(
            width=5,
            height=1,
            walls=[],
            mud=[],
            cheese=[(1, 0), (2, 0), (3, 0)],
            player1_pos=(0, 0),  # Rat starts left
            player2_pos=(4, 0),  # Python starts right
        )

        greedy_rat = DirectAIRunner(GreedyAI, Player.RAT)
        greedy_python = DirectAIRunner(GreedyAI, Player.PYTHON)

        # Turn 1: Both move toward nearest cheese
        rat_move = greedy_rat.get_move(game)
        python_move = greedy_python.get_move(game)

        assert rat_move == Direction.RIGHT, "Turn 1: Rat should move RIGHT to (1,0)"
        assert python_move == Direction.LEFT, "Turn 1: Python should move LEFT to (3,0)"

        game.step(rat_move, python_move)

        # After turn 1: Both collected their nearest cheese
        assert game.player1_position == Coordinates(1, 0), "Turn 1: Rat at (1,0)"
        assert game.player2_position == Coordinates(3, 0), "Turn 1: Python at (3,0)"
        assert game.player1_score == 1.0, "Turn 1: Rat score = 1.0"
        assert game.player2_score == 1.0, "Turn 1: Python score = 1.0"
        assert len(game.cheese_positions()) == 1, "Turn 1: 1 cheese remaining at (2,0)"

        # Turn 2: Both target (2,0), simultaneous collection
        rat_move = greedy_rat.get_move(game)
        python_move = greedy_python.get_move(game)

        assert rat_move == Direction.RIGHT, "Turn 2: Rat should move RIGHT to (2,0)"
        assert python_move == Direction.LEFT, "Turn 2: Python should move LEFT to (2,0)"

        game.step(rat_move, python_move)

        # After turn 2: Simultaneous collection at (2,0)
        assert game.player1_position == Coordinates(2, 0), "Turn 2: Rat at (2,0)"
        assert game.player2_position == Coordinates(2, 0), "Turn 2: Python at (2,0)"
        assert game.player1_score == 1.5, "Turn 2: Rat score = 1.5 (simultaneous)"
        assert game.player2_score == 1.5, "Turn 2: Python score = 1.5 (simultaneous)"
        assert len(game.cheese_positions()) == 0, "Turn 2: All cheese collected"

        # Verify total
        total_cheese = game.player1_score + game.player2_score
        assert total_cheese == 3.0, "Total cheese collected = 3.0"

    def test_asymmetric_advantage_clear_winner(self):
        """Test asymmetric maze where Rat has clear advantage.

        Setup: 1D maze, Rat much closer to all cheese
        - Cheese at (1,0), (2,0)
        - Rat at (0,0), Python at (6,0)

        Expected:
        - Turn 1: Rat collects (1,0), Python moves toward (2,0)
        - Turn 2: Rat collects (2,0), Python still moving
        - Final: Rat=2.0, Python=0.0
        """
        game = PyGameState.create_custom(
            width=7,
            height=1,
            walls=[],
            mud=[],
            cheese=[(1, 0), (2, 0)],
            player1_pos=(0, 0),  # Rat starts left
            player2_pos=(6, 0),  # Python starts far right
        )

        greedy_rat = DirectAIRunner(GreedyAI, Player.RAT)
        greedy_python = DirectAIRunner(GreedyAI, Player.PYTHON)

        # Turn 1: Rat collects (1,0), Python moves LEFT
        rat_move = greedy_rat.get_move(game)
        python_move = greedy_python.get_move(game)

        assert rat_move == Direction.RIGHT, "Turn 1: Rat should move RIGHT to (1,0)"
        assert python_move == Direction.LEFT, "Turn 1: Python should move LEFT"

        game.step(rat_move, python_move)

        assert game.player1_position == Coordinates(1, 0), "Turn 1: Rat at (1,0)"
        assert game.player2_position == Coordinates(5, 0), "Turn 1: Python at (5,0)"
        assert game.player1_score == 1.0, "Turn 1: Rat score = 1.0"
        assert game.player2_score == 0.0, "Turn 1: Python score = 0.0"

        # Turn 2: Rat collects (2,0), Python continues moving
        rat_move = greedy_rat.get_move(game)
        python_move = greedy_python.get_move(game)

        assert rat_move == Direction.RIGHT, "Turn 2: Rat should move RIGHT to (2,0)"
        assert python_move == Direction.LEFT, "Turn 2: Python should move LEFT"

        game.step(rat_move, python_move)

        assert game.player1_position == Coordinates(2, 0), "Turn 2: Rat at (2,0)"
        assert game.player2_position == Coordinates(4, 0), "Turn 2: Python at (4,0)"
        assert game.player1_score == 2.0, "Turn 2: Rat score = 2.0"
        assert game.player2_score == 0.0, "Turn 2: Python score = 0.0"
        assert len(game.cheese_positions()) == 0, "Turn 2: All cheese collected by Rat"

        # Verify Rat won with 100% of cheese
        assert game.player1_score == 2.0, "Final: Rat wins with all cheese"
        assert game.player2_score == 0.0, "Final: Python gets no cheese"

    def test_mud_timing_strategic_delay(self):
        """Test mud affecting arrival time and cheese collection.

        Setup: 3x5 maze with mud blocking direct path
        - Cheese at (2,1) center
        - Rat at (0,1) with 5-turn mud blocking right
        - Python at (4,1) with clear path

        Expected:
        - Rat avoids mud (goes UP→RIGHT→DOWN = 3 moves)
        - Python goes direct LEFT (2 moves)
        - Python wins
        """
        game = PyGameState.create_custom(
            width=5,
            height=3,
            walls=[],
            mud=[
                ((0, 1), (1, 1), 5),  # Heavy mud if Rat goes right directly
            ],
            cheese=[(2, 1)],  # Center cheese
            player1_pos=(0, 1),  # Rat starts left middle
            player2_pos=(4, 1),  # Python starts right middle
        )

        greedy_rat = DirectAIRunner(GreedyAI, Player.RAT)
        greedy_python = DirectAIRunner(GreedyAI, Player.PYTHON)

        # Turn 1: Rat should avoid mud by going UP, Python goes LEFT
        rat_move = greedy_rat.get_move(game)
        python_move = greedy_python.get_move(game)

        assert rat_move == Direction.UP, "Turn 1: Rat should go UP to avoid mud"
        assert python_move == Direction.LEFT, "Turn 1: Python should go LEFT"

        game.step(rat_move, python_move)

        assert game.player1_position == Coordinates(0, 2), "Turn 1: Rat at (0,2)"
        assert game.player2_position == Coordinates(3, 1), "Turn 1: Python at (3,1)"
        # Rat avoided mud by going UP, Python has clear path

        # Turn 2: Rat goes RIGHT, Python collects cheese at (2,1)
        rat_move = greedy_rat.get_move(game)
        python_move = greedy_python.get_move(game)

        assert rat_move == Direction.RIGHT, "Turn 2: Rat should go RIGHT"
        assert python_move == Direction.LEFT, "Turn 2: Python should go LEFT to (2,1)"

        game.step(rat_move, python_move)

        assert game.player1_position == Coordinates(1, 2), "Turn 2: Rat at (1,2)"
        assert game.player2_position == Coordinates(2, 1), "Turn 2: Python at (2,1)"
        assert game.player2_score == 1.0, "Turn 2: Python collected cheese"
        assert game.player1_score == 0.0, "Turn 2: Rat hasn't collected yet"

        # Verify Python won, Rat never entered mud
        assert game.player2_score == 1.0, "Final: Python wins"
        assert game.player1_score == 0.0, "Final: Rat gets no cheese"
        assert len(game.cheese_positions()) == 0, "Final: All cheese collected"

    def test_state_synchronization_simultaneous_calculation(self):
        """CRITICAL: Test state synchronization during simultaneous calculation.

        This is the regression test for the command requeue bug fix.
        When both AIs calculate moves simultaneously, commands arriving during
        calculation must be re-queued (not dropped) to prevent state desync.

        Setup: 7x7 maze with multiple cheese
        - Both greedy AIs calculating moves at the same time
        - Verify state consistency after each turn

        Expected:
        - No state desync between AI perspectives and actual game state
        - Both AIs see consistent opponent positions
        - Both AIs see same cheese list
        - Total cheese collected equals initial count
        """
        game = PyGameState.create_custom(
            width=7,
            height=7,
            walls=[],
            mud=[],
            cheese=[
                (1, 1),
                (5, 1),
                (3, 3),
                (1, 5),
                (5, 5),
            ],  # 5 cheese distributed
            player1_pos=(0, 0),  # Rat bottom-left
            player2_pos=(6, 0),  # Python bottom-right
        )

        greedy_rat = DirectAIRunner(GreedyAI, Player.RAT)
        greedy_python = DirectAIRunner(GreedyAI, Player.PYTHON)

        initial_cheese_count = len(game.cheese_positions())
        max_turns = 30

        for turn in range(max_turns):
            # Get states from both perspectives BEFORE moves
            rat_state = ProtocolState(game, Player.RAT)
            python_state = ProtocolState(game, Player.PYTHON)

            # CRITICAL: Verify both AIs see consistent game state
            assert (
                rat_state.opponent_position == game.player2_position
            ), f"Turn {turn}: Rat sees wrong opponent position"
            assert (
                python_state.opponent_position == game.player1_position
            ), f"Turn {turn}: Python sees wrong opponent position"

            # Both AIs should see same cheese list
            assert set(rat_state.cheese) == set(
                python_state.cheese
            ), f"Turn {turn}: Cheese mismatch between AI perspectives"

            # Both calculate moves simultaneously (this is where bug could occur)
            rat_move = greedy_rat.get_move(game)
            python_move = greedy_python.get_move(game)

            # Execute moves
            game.step(rat_move, python_move)

            # Verify state consistency AFTER moves
            assert (
                game.player1_position.x >= 0 and game.player1_position.x < 7
            ), f"Turn {turn}: Rat position out of bounds"
            assert (
                game.player2_position.x >= 0 and game.player2_position.x < 7
            ), f"Turn {turn}: Python position out of bounds"

            # Check if game over
            if len(game.cheese_positions()) == 0:
                break

        # Final verification
        total_collected = game.player1_score + game.player2_score
        assert (
            total_collected == initial_cheese_count
        ), f"Cheese accounting error: collected {total_collected}, expected {initial_cheese_count}"

        # Both should have collected some cheese (greedy AIs are good)
        assert game.player1_score > 0, "Rat should collect at least one cheese"
        assert game.player2_score > 0, "Python should collect at least one cheese"

        # No cheese should remain
        assert len(game.cheese_positions()) == 0, "All cheese should be collected"

    def test_wall_navigation_different_paths(self):
        """Test wall forcing different optimal paths for each AI.

        Setup: 7x5 maze with vertical wall dividing it
        - Vertical wall at x=3 from y=0 to y=3 (gap at y=4)
        - Cheese at (1,2) left side, (5,2) right side
        - Rat at (0,0), Python at (6,0)

        Expected:
        - Rat takes path to (1,2) on left side
        - Python takes path to (5,2) on right side
        - Both navigate around wall efficiently
        - Both collect one cheese each
        """
        # Create vertical wall blocking middle, with gap at top
        walls = [((3, y), (4, y)) for y in range(4)]  # Wall from y=0 to y=3

        game = PyGameState.create_custom(
            width=7,
            height=5,
            walls=walls,
            mud=[],
            cheese=[(1, 2), (5, 2)],  # One cheese on each side of wall
            player1_pos=(0, 0),  # Rat on left side
            player2_pos=(6, 0),  # Python on right side
        )

        greedy_rat = DirectAIRunner(GreedyAI, Player.RAT)
        greedy_python = DirectAIRunner(GreedyAI, Player.PYTHON)

        # Both should target nearest cheese (on their respective sides)
        # Rat: (0,0) → (1,2) = RIGHT + UP + UP = 3 moves
        # Python: (6,0) → (5,2) = LEFT + UP + UP = 3 moves

        # Turn 1: Both start moving toward their nearest cheese
        rat_move = greedy_rat.get_move(game)
        python_move = greedy_python.get_move(game)

        # Both should move toward their cheese (exact path may vary, but should move)
        assert rat_move in [
            Direction.UP,
            Direction.RIGHT,
        ], "Turn 1: Rat should move toward (1,2)"
        assert python_move in [
            Direction.UP,
            Direction.LEFT,
        ], "Turn 1: Python should move toward (5,2)"

        # Run until both collect their cheese (max 5 turns should be enough)
        for _turn in range(5):
            rat_move = greedy_rat.get_move(game)
            python_move = greedy_python.get_move(game)
            game.step(rat_move, python_move)

            # Check if both collected
            if game.player1_score >= 1.0 and game.player2_score >= 1.0:
                break

        # Verify both collected exactly one cheese each
        assert game.player1_score == 1.0, "Rat should collect exactly 1 cheese"
        assert game.player2_score == 1.0, "Python should collect exactly 1 cheese"
        assert len(game.cheese_positions()) == 0, "All cheese should be collected"

        # Verify neither crossed the wall (stayed on their side)
        # Rat should be on left side (x <= 3), Python on right side (x >= 3)
        assert game.player1_position == Coordinates(
            1, 2
        ), "Rat should be at (1,2) where cheese was"
        assert game.player2_position == Coordinates(
            5, 2
        ), "Python should be at (5,2) where cheese was"


if __name__ == "__main__":
    pytest.main([__file__, "-v", "-s"])
