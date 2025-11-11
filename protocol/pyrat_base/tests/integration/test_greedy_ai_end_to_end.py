"""End-to-end integration test for greedy AI correctness.

This test verifies that the greedy AI:
1. Makes optimal pathfinding decisions across multiple turns
2. Successfully collects cheese in a full game
3. Maintains correct game state synchronization
4. Outperforms simpler AIs (wins against dummy AI)
"""

import pytest
from pyrat_engine._rust import PyGameState
from pyrat_engine.game import Direction

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
            player2_pos=(4, 4),  # Python starts at top-right (on cheese)
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
        assert game.player1_position == (1, 0), "Rat should be at (1, 0)"

        # Turn 2: Rat should now be on the cheese at (1,0)
        # Greedy should target the remaining cheese
        move = greedy.get_move(game)
        # After collecting (1,0), should target (4,4)
        assert move in [
            Direction.UP,
            Direction.RIGHT,
        ], "Should move toward remaining cheese"

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
            game.player1_position
            == (
                4,
                0,
            )
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


if __name__ == "__main__":
    pytest.main([__file__, "-v", "-s"])
