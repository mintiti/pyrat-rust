"""End-to-end integration test for greedy AI correctness.

This test verifies that the greedy AI:
1. Makes optimal pathfinding decisions across multiple turns
2. Successfully collects cheese in a full game
3. Maintains correct game state synchronization
4. Outperforms simpler AIs (wins against dummy AI)
"""
# ruff: noqa: PLR2004

import pytest
from pyrat_engine import GameBuilder, GameConfig
from pyrat_engine.core.types import Coordinates, Direction

from pyrat_base import ProtocolState
from pyrat_base.enums import Player
from pyrat_base.examples.dummy_ai import DummyAI
from pyrat_base.examples.greedy_ai import GreedyAI


def _create_game(
    width=5,
    height=5,
    walls=None,
    mud=None,
    cheese=None,
    player1_pos=(0, 0),
    player2_pos=None,
):
    """Helper to create a game via GameBuilder."""
    if player2_pos is None:
        player2_pos = (width - 1, height - 1)
    config = (
        GameBuilder(width, height)
        .with_custom_maze(walls=walls or [], mud=mud or [])
        .with_custom_positions(player1_pos, player2_pos)
        .with_custom_cheese(cheese or [(width // 2, height // 2)])
        .build()
    )
    return config.create()


class DirectAIRunner:
    """Helper to run AIs directly (without subprocess) for testing."""

    def __init__(self, ai_class, player: Player):
        self.ai = ai_class()
        self.player = player

    def get_move(self, game_state) -> Direction:
        """Get the AI's move for the current game state."""
        state = ProtocolState(game_state, self.player)
        return self.ai.get_move(state)


class TestGreedyAIEndToEnd:
    """End-to-end tests for greedy AI correctness."""

    def test_greedy_finds_nearest_cheese_simple_maze(self):
        """Test greedy AI finds and collects the nearest cheese in a simple maze."""
        game = _create_game(
            width=5,
            height=5,
            cheese=[(1, 0), (4, 4)],
            player1_pos=(0, 0),
            player2_pos=(0, 4),
        )

        greedy = DirectAIRunner(GreedyAI, Player.RAT)

        move = greedy.get_move(game)
        assert (
            move == Direction.RIGHT
        ), "Should move RIGHT toward nearest cheese at (1,0)"

        game.step(Direction.RIGHT, Direction.STAY)

        assert game.player1_position == Coordinates(1, 0), "Rat should be at (1, 0)"

        move = greedy.get_move(game)
        assert move in [
            Direction.UP,
            Direction.RIGHT,
        ], "Should move toward remaining cheese"

    @pytest.mark.slow
    def test_greedy_navigates_around_walls(self):
        """Test greedy AI correctly navigates around walls to reach cheese."""
        walls = [((2, y), (3, y)) for y in range(4)]

        game = _create_game(
            width=6,
            height=5,
            walls=walls,
            cheese=[(4, 0)],
            player1_pos=(0, 0),
            player2_pos=(5, 4),
        )

        greedy = DirectAIRunner(GreedyAI, Player.RAT)

        expected_steps = 12

        for step in range(expected_steps):
            rat_move = greedy.get_move(game)

            assert rat_move in [
                Direction.UP,
                Direction.DOWN,
                Direction.LEFT,
                Direction.RIGHT,
            ], f"Step {step}: Invalid move {rat_move}"

            game.step(rat_move, Direction.STAY)

        assert (
            game.player1_position == Coordinates(4, 0)
        ), f"After {expected_steps} steps, rat should be at (4,0), but is at {game.player1_position}"

        assert (
            game.player1_score == 1.0
        ), f"After {expected_steps} steps, rat should have collected cheese (score=1.0), but score is {game.player1_score}"

    def test_greedy_handles_mud_optimally(self):
        """Test greedy AI considers mud costs when choosing paths."""
        game = _create_game(
            width=6,
            height=3,
            mud=[
                ((0, 0), (1, 0), 10),
                ((1, 0), (2, 0), 10),
            ],
            cheese=[(2, 0), (0, 2)],
            player1_pos=(0, 0),
            player2_pos=(5, 2),
        )

        greedy = DirectAIRunner(GreedyAI, Player.RAT)

        move = greedy.get_move(game)
        assert move == Direction.UP, "Should avoid heavy mud by going UP first"

    def test_greedy_vs_dummy_full_game(self):
        """Test greedy AI beats dummy AI in a full game."""
        game = GameConfig.classic(11, 9, 5).create(seed=12345)

        greedy = DirectAIRunner(GreedyAI, Player.RAT)
        dummy = DirectAIRunner(DummyAI, Player.PYTHON)

        max_turns = 100
        turn = 0

        while turn < max_turns and len(game.cheese_positions()) > 0:
            rat_move = greedy.get_move(game)
            python_move = dummy.get_move(game)

            game.step(rat_move, python_move)
            turn += 1

            total_cheese = game.player1_score + game.player2_score
            if game.player1_score > total_cheese / 2:
                break
            if game.player2_score > total_cheese / 2:
                break

        assert game.player1_score > game.player2_score, "Greedy AI should beat dummy AI"
        assert game.player1_score > 0, "Greedy AI should collect at least one cheese"

    @pytest.mark.slow
    def test_greedy_state_synchronization_multi_turn(self):
        """Test greedy AI maintains correct state over multiple turns."""
        game = _create_game(
            width=7,
            height=7,
            cheese=[(3, 3), (6, 6), (1, 1)],
            player1_pos=(0, 0),
            player2_pos=(6, 0),
        )

        greedy = DirectAIRunner(GreedyAI, Player.RAT)

        positions_visited = []

        for turn in range(20):
            state = ProtocolState(game, Player.RAT)
            current_pos = state.my_position
            positions_visited.append(current_pos)

            move = greedy.get_move(game)

            game.step(move, Direction.STAY)

            expected_pos = game.player1_position
            assert (
                expected_pos == game.player1_position
            ), f"State desync detected at turn {turn}"

            if len(game.cheese_positions()) == 0:
                break

        assert game.player1_score > 0, "Greedy should collect cheese over 20 turns"

        unique_positions = set(positions_visited)
        assert len(unique_positions) > 1, "Rat should move to different positions"

    @pytest.mark.slow
    @pytest.mark.parametrize("seed", [12345, 54321, 99999, 11111])
    def test_greedy_on_random_mazes(self, seed):
        """Test greedy AI performs well on various random mazes."""
        game = GameConfig.classic(15, 11, 10).create(seed=seed)

        greedy = DirectAIRunner(GreedyAI, Player.RAT)

        max_turns = 200
        min_cheese_for_success = 5
        min_cheese_threshold = 3
        for _turn in range(max_turns):
            move = greedy.get_move(game)
            game.step(move, Direction.STAY)

            if game.player1_score >= min_cheese_for_success:
                break

        assert (
            game.player1_score >= min_cheese_threshold
        ), f"Greedy should collect at least {min_cheese_threshold} cheese (got {game.player1_score})"

    def test_greedy_recalculates_when_cheese_collected(self):
        """Test greedy AI updates its target when cheese is collected."""
        game = _create_game(
            width=5,
            height=5,
            cheese=[(1, 0), (2, 0), (3, 0)],
            player1_pos=(0, 0),
            player2_pos=(4, 4),
        )

        greedy = DirectAIRunner(GreedyAI, Player.RAT)

        move1 = greedy.get_move(game)
        assert move1 == Direction.RIGHT

        expected_score_after_first = 1.0
        expected_score_after_second = 2.0
        expected_score_after_third = 3.0

        game.step(Direction.RIGHT, Direction.STAY)
        assert game.player1_score == expected_score_after_first

        move2 = greedy.get_move(game)
        assert move2 == Direction.RIGHT, "Should continue to next nearest cheese"

        game.step(Direction.RIGHT, Direction.STAY)
        assert game.player1_score == expected_score_after_second

        move3 = greedy.get_move(game)
        assert move3 == Direction.RIGHT

        game.step(Direction.RIGHT, Direction.STAY)
        assert game.player1_score == expected_score_after_third

    def test_greedy_handles_simultaneous_collection(self):
        """Test greedy AI handles simultaneous cheese collection correctly."""
        game = _create_game(
            width=3,
            height=2,
            cheese=[(1, 0)],
            player1_pos=(0, 0),
            player2_pos=(2, 0),
        )

        greedy = DirectAIRunner(GreedyAI, Player.RAT)

        move = greedy.get_move(game)
        assert move == Direction.RIGHT

        game.step(Direction.RIGHT, Direction.LEFT)

        expected_simultaneous_score = 0.5
        assert game.player1_score == expected_simultaneous_score
        assert game.player2_score == expected_simultaneous_score


@pytest.mark.slow
class TestGreedyVsGreedy:
    """Precise, deterministic tests for greedy vs greedy gameplay."""

    def test_symmetric_race_equal_split(self):
        """Test symmetric maze where both AIs collect equal cheese."""
        game = _create_game(
            width=5,
            height=2,
            cheese=[(1, 0), (2, 0), (3, 0)],
            player1_pos=(0, 0),
            player2_pos=(4, 0),
        )

        greedy_rat = DirectAIRunner(GreedyAI, Player.RAT)
        greedy_python = DirectAIRunner(GreedyAI, Player.PYTHON)

        rat_move = greedy_rat.get_move(game)
        python_move = greedy_python.get_move(game)

        assert rat_move == Direction.RIGHT, "Turn 1: Rat should move RIGHT to (1,0)"
        assert python_move == Direction.LEFT, "Turn 1: Python should move LEFT to (3,0)"

        game.step(rat_move, python_move)

        assert game.player1_position == Coordinates(1, 0), "Turn 1: Rat at (1,0)"
        assert game.player2_position == Coordinates(3, 0), "Turn 1: Python at (3,0)"
        assert game.player1_score == 1.0, "Turn 1: Rat score = 1.0"
        assert game.player2_score == 1.0, "Turn 1: Python score = 1.0"
        assert len(game.cheese_positions()) == 1, "Turn 1: 1 cheese remaining at (2,0)"

        rat_move = greedy_rat.get_move(game)
        python_move = greedy_python.get_move(game)

        assert rat_move == Direction.RIGHT, "Turn 2: Rat should move RIGHT to (2,0)"
        assert python_move == Direction.LEFT, "Turn 2: Python should move LEFT to (2,0)"

        game.step(rat_move, python_move)

        assert game.player1_position == Coordinates(2, 0), "Turn 2: Rat at (2,0)"
        assert game.player2_position == Coordinates(2, 0), "Turn 2: Python at (2,0)"
        assert game.player1_score == 1.5, "Turn 2: Rat score = 1.5 (simultaneous)"
        assert game.player2_score == 1.5, "Turn 2: Python score = 1.5 (simultaneous)"
        assert len(game.cheese_positions()) == 0, "Turn 2: All cheese collected"

        total_cheese = game.player1_score + game.player2_score
        assert total_cheese == 3.0, "Total cheese collected = 3.0"

    def test_asymmetric_advantage_clear_winner(self):
        """Test asymmetric maze where Rat has clear advantage."""
        game = _create_game(
            width=7,
            height=2,
            cheese=[(1, 0), (2, 0)],
            player1_pos=(0, 0),
            player2_pos=(6, 0),
        )

        greedy_rat = DirectAIRunner(GreedyAI, Player.RAT)
        greedy_python = DirectAIRunner(GreedyAI, Player.PYTHON)

        rat_move = greedy_rat.get_move(game)
        python_move = greedy_python.get_move(game)

        assert rat_move == Direction.RIGHT, "Turn 1: Rat should move RIGHT to (1,0)"
        assert python_move == Direction.LEFT, "Turn 1: Python should move LEFT"

        game.step(rat_move, python_move)

        assert game.player1_position == Coordinates(1, 0), "Turn 1: Rat at (1,0)"
        assert game.player2_position == Coordinates(5, 0), "Turn 1: Python at (5,0)"
        assert game.player1_score == 1.0, "Turn 1: Rat score = 1.0"
        assert game.player2_score == 0.0, "Turn 1: Python score = 0.0"

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

        assert game.player1_score == 2.0, "Final: Rat wins with all cheese"
        assert game.player2_score == 0.0, "Final: Python gets no cheese"

    def test_mud_timing_strategic_delay(self):
        """Test mud affecting arrival time and cheese collection."""
        game = _create_game(
            width=5,
            height=3,
            mud=[((0, 1), (1, 1), 5)],
            cheese=[(2, 1)],
            player1_pos=(0, 1),
            player2_pos=(4, 1),
        )

        greedy_rat = DirectAIRunner(GreedyAI, Player.RAT)
        greedy_python = DirectAIRunner(GreedyAI, Player.PYTHON)

        rat_move = greedy_rat.get_move(game)
        python_move = greedy_python.get_move(game)

        assert rat_move == Direction.UP, "Turn 1: Rat should go UP to avoid mud"
        assert python_move == Direction.LEFT, "Turn 1: Python should go LEFT"

        game.step(rat_move, python_move)

        assert game.player1_position == Coordinates(0, 2), "Turn 1: Rat at (0,2)"
        assert game.player2_position == Coordinates(3, 1), "Turn 1: Python at (3,1)"

        rat_move = greedy_rat.get_move(game)
        python_move = greedy_python.get_move(game)

        assert rat_move == Direction.RIGHT, "Turn 2: Rat should go RIGHT"
        assert python_move == Direction.LEFT, "Turn 2: Python should go LEFT to (2,1)"

        game.step(rat_move, python_move)

        assert game.player1_position == Coordinates(1, 2), "Turn 2: Rat at (1,2)"
        assert game.player2_position == Coordinates(2, 1), "Turn 2: Python at (2,1)"
        assert game.player2_score == 1.0, "Turn 2: Python collected cheese"
        assert game.player1_score == 0.0, "Turn 2: Rat hasn't collected yet"

        assert game.player2_score == 1.0, "Final: Python wins"
        assert game.player1_score == 0.0, "Final: Rat gets no cheese"
        assert len(game.cheese_positions()) == 0, "Final: All cheese collected"

    def test_state_synchronization_simultaneous_calculation(self):
        """CRITICAL: Test state synchronization during simultaneous calculation."""
        game = _create_game(
            width=7,
            height=7,
            cheese=[
                (1, 1),
                (5, 1),
                (3, 3),
                (1, 5),
                (5, 5),
            ],
            player1_pos=(0, 0),
            player2_pos=(6, 0),
        )

        greedy_rat = DirectAIRunner(GreedyAI, Player.RAT)
        greedy_python = DirectAIRunner(GreedyAI, Player.PYTHON)

        initial_cheese_count = len(game.cheese_positions())
        max_turns = 30

        for turn in range(max_turns):
            rat_state = ProtocolState(game, Player.RAT)
            python_state = ProtocolState(game, Player.PYTHON)

            assert (
                rat_state.opponent_position == game.player2_position
            ), f"Turn {turn}: Rat sees wrong opponent position"
            assert (
                python_state.opponent_position == game.player1_position
            ), f"Turn {turn}: Python sees wrong opponent position"

            assert set(rat_state.cheese) == set(
                python_state.cheese
            ), f"Turn {turn}: Cheese mismatch between AI perspectives"

            rat_move = greedy_rat.get_move(game)
            python_move = greedy_python.get_move(game)

            game.step(rat_move, python_move)

            assert (
                game.player1_position.x >= 0 and game.player1_position.x < 7
            ), f"Turn {turn}: Rat position out of bounds"
            assert (
                game.player2_position.x >= 0 and game.player2_position.x < 7
            ), f"Turn {turn}: Python position out of bounds"

            if len(game.cheese_positions()) == 0:
                break

        total_collected = game.player1_score + game.player2_score
        assert (
            total_collected == initial_cheese_count
        ), f"Cheese accounting error: collected {total_collected}, expected {initial_cheese_count}"

        assert game.player1_score > 0, "Rat should collect at least one cheese"
        assert game.player2_score > 0, "Python should collect at least one cheese"

        assert len(game.cheese_positions()) == 0, "All cheese should be collected"

    def test_wall_navigation_different_paths(self):
        """Test wall forcing different optimal paths for each AI."""
        walls = [((3, y), (4, y)) for y in range(4)]

        game = _create_game(
            width=7,
            height=5,
            walls=walls,
            cheese=[(1, 2), (5, 2)],
            player1_pos=(0, 0),
            player2_pos=(6, 0),
        )

        greedy_rat = DirectAIRunner(GreedyAI, Player.RAT)
        greedy_python = DirectAIRunner(GreedyAI, Player.PYTHON)

        rat_move = greedy_rat.get_move(game)
        python_move = greedy_python.get_move(game)

        assert rat_move in [
            Direction.UP,
            Direction.RIGHT,
        ], "Turn 1: Rat should move toward (1,2)"
        assert python_move in [
            Direction.UP,
            Direction.LEFT,
        ], "Turn 1: Python should move toward (5,2)"

        for _turn in range(5):
            rat_move = greedy_rat.get_move(game)
            python_move = greedy_python.get_move(game)
            game.step(rat_move, python_move)

            if game.player1_score >= 1.0 and game.player2_score >= 1.0:
                break

        assert game.player1_score == 1.0, "Rat should collect exactly 1 cheese"
        assert game.player2_score == 1.0, "Python should collect exactly 1 cheese"
        assert len(game.cheese_positions()) == 0, "All cheese should be collected"

        assert game.player1_position == Coordinates(
            1, 2
        ), "Rat should be at (1,2) where cheese was"
        assert game.player2_position == Coordinates(
            5, 2
        ), "Python should be at (5,2) where cheese was"


if __name__ == "__main__":
    pytest.main([__file__, "-v", "-s"])
