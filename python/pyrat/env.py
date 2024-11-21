from __future__ import annotations

from typing import TYPE_CHECKING, Any, ClassVar

import numpy as np
from gymnasium.spaces import Box, Discrete
from gymnasium.spaces import Dict as SpaceDict
from pettingzoo.utils.env import AgentID, ParallelEnv

from pyrat._rust import PyGameState, PyObservationHandler

if TYPE_CHECKING:
    from pyrat.game import Direction


class PyRatEnv(ParallelEnv):  # type: ignore[misc]
    """PyRat PettingZoo Environment

    A PettingZoo parallel environment wrapper for the PyRat game. This environment
    implements the standard PettingZoo interface for multi-agent reinforcement learning.

    The observation space includes:
    - player_position: (x,y) coordinates of the current player
    - player_mud_turns: remaining turns stuck in mud
    - player_score: current score
    - opponent_position: (x,y) coordinates of the opponent
    - opponent_mud_turns: opponent's remaining mud turns
    - opponent_score: opponent's current score
    - cheese_matrix: 2D binary array showing cheese locations
    - movement_matrix: 3D array encoding valid moves and mud costs

    The action space is discrete with 5 possible actions:
    - UP (0)
    - RIGHT (1)
    - DOWN (2)
    - LEFT (3)
    - STAY (4)

    Example:
        >>> env = PyRatEnv(width=15, height=15)
        >>> obs, info = env.reset(seed=42)
        >>> actions = {"player_1": Direction.RIGHT, "player_2": Direction.LEFT}
        >>> obs, rewards, terminations, truncations, infos = env.step(actions)
    """

    metadata: ClassVar[dict[str, Any]] = {
        "render_modes": ["human", "rgb_array"],
        "name": "pyrat_v0",
    }

    def __init__(
        self,
        width: int = 21,
        height: int = 15,
        cheese_count: int = 41,
        symmetric: bool = True,
        seed: int | None = None,
    ):
        super().__init__()

        self.possible_agents = ["player_1", "player_2"]

        # Create game state and observation handler
        self.game = PyGameState(width, height, cheese_count, symmetric, seed)
        self.obs_handler = PyObservationHandler(self.game)

        # Define spaces
        self.action_space = {agent: Discrete(5) for agent in self.possible_agents}

        # Observation space matches our observation structure
        obs_space = SpaceDict(
            {
                "player_position": Box(
                    low=0, high=max(width, height), shape=(2,), dtype=np.uint8
                ),
                "player_mud_turns": Box(low=0, high=3, shape=(1,), dtype=np.uint8),
                "player_score": Box(
                    low=0, high=cheese_count, shape=(1,), dtype=np.float32
                ),
                "opponent_position": Box(
                    low=0, high=max(width, height), shape=(2,), dtype=np.uint8
                ),
                "opponent_mud_turns": Box(low=0, high=3, shape=(1,), dtype=np.uint8),
                "opponent_score": Box(
                    low=0, high=cheese_count, shape=(1,), dtype=np.float32
                ),
                "current_turn": Box(low=0, high=300, shape=(1,), dtype=np.uint16),
                "max_turns": Box(low=0, high=300, shape=(1,), dtype=np.uint16),
                "cheese_matrix": Box(
                    low=0, high=1, shape=(width, height), dtype=np.uint8
                ),
                "movement_matrix": Box(
                    low=-1, high=3, shape=(width, height, 4), dtype=np.int8
                ),
            }
        )
        self.observation_space: dict[AgentID, SpaceDict] = {
            agent: obs_space for agent in self.possible_agents
        }

    def reset(
        self, seed: int | None = None, options: dict[str, Any] | None = None
    ) -> tuple[dict[str, Any], dict[str, Any]]:
        self.agents = self.possible_agents[:]
        self.game.reset(seed)

        observations = {
            "player_1": self.obs_handler.get_observation(self.game, True),
            "player_2": self.obs_handler.get_observation(self.game, False),
        }
        infos: dict[str, Any] = {agent: {} for agent in self.agents}

        return observations, infos

    def step(
        self, actions: dict[str, Direction]
    ) -> tuple[
        dict[str, Any],
        dict[str, float],
        dict[str, bool],
        dict[str, bool],
        dict[str, Any],
    ]:
        # Process moves
        game_over, collected = self.game.step(
            actions["player_1"].value, actions["player_2"].value
        )

        # Update observation handler with collected cheese
        if collected:
            self.obs_handler.update_collected_cheese(collected)

        # Get observations
        observations = {
            "player_1": self.obs_handler.get_observation(self.game, True),
            "player_2": self.obs_handler.get_observation(self.game, False),
        }

        # Calculate rewards (just score changes for now)
        rewards = {
            "player_1": self.game.player1_score,
            "player_2": self.game.player2_score,
        }

        # Game termination
        terminations = {agent: game_over for agent in self.agents}
        truncations: dict[str, bool] = {agent: False for agent in self.agents}

        # No additional info for now
        infos: dict[str, Any] = {agent: {} for agent in self.agents}

        return observations, rewards, terminations, truncations, infos
