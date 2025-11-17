"""Test data builders for PyRat protocol tests."""

from typing import List, Optional, Tuple

from pyrat_engine.core import Direction

from pyrat_base.enums import Player
from pyrat_base.protocol import DIRECTION_INT_TO_NAME

# Note: We now use PyGameConfigBuilder directly instead of a custom MazeBuilder
# Example usage:
#   game = (PyGameConfigBuilder(5, 5)
#           .with_walls([((0, 0), (1, 0))])
#           .with_mud([((1, 1), (1, 2), 2)])
#           .with_cheese([(2, 2)])
#           .build())


class CommandSequenceBuilder:
    """Build sequences of protocol commands for testing."""

    def __init__(self):
        self.commands = []

    def add(self, command: str) -> "CommandSequenceBuilder":
        """Add a command to the sequence."""
        self.commands.append(command)
        return self

    def handshake(
        self, ai_name: str = "TestAI", author: str = "Tester"
    ) -> "CommandSequenceBuilder":
        """Add handshake sequence."""
        self.add("pyrat")
        # Note: AI would respond here, we're just building engine commands
        return self

    def game_init(self, width: int = 5, height: int = 5) -> "CommandSequenceBuilder":
        """Add basic game initialization sequence."""
        self.add("newgame")
        self.add(f"maze width:{width} height:{height}")
        self.add("walls")  # No walls by default
        self.add("mud")  # No mud by default
        self.add("cheese (2,2)")  # Single cheese in center
        self.add("player1 rat (0,0)")
        self.add(f"player2 python ({width-1},{height-1})")
        self.add("youare rat")
        return self

    def from_game_config(
        self,
        width: int,
        height: int,
        walls: Optional[List[Tuple[Tuple[int, int], Tuple[int, int]]]] = None,
        mud: Optional[List[Tuple[Tuple[int, int], Tuple[int, int], int]]] = None,
        cheese: Optional[List[Tuple[int, int]]] = None,
        player1_pos: Tuple[int, int] = (0, 0),
        player2_pos: Optional[Tuple[int, int]] = None,
        player: Player = Player.RAT,
    ) -> "CommandSequenceBuilder":
        """Build game init sequence from game configuration."""
        if player2_pos is None:
            player2_pos = (width - 1, height - 1)

        self.add("newgame")
        self.add(f"maze width:{width} height:{height}")

        # Format walls
        if walls:
            wall_strs = []
            for (x1, y1), (x2, y2) in walls:
                wall_strs.append(f"({x1},{y1})-({x2},{y2})")
            self.add(f"walls {' '.join(wall_strs)}")
        else:
            self.add("walls")

        # Format mud
        if mud:
            mud_strs = []
            for (x1, y1), (x2, y2), value in mud:
                mud_strs.append(f"({x1},{y1})-({x2},{y2}):{value}")
            self.add(f"mud {' '.join(mud_strs)}")
        else:
            self.add("mud")

        # Format cheese
        if cheese:
            cheese_strs = [f"({x},{y})" for x, y in cheese]
            self.add(f"cheese {' '.join(cheese_strs)}")
        else:
            self.add("cheese")

        # Players
        self.add(f"player1 rat ({player1_pos[0]},{player1_pos[1]})")
        self.add(f"player2 python ({player2_pos[0]},{player2_pos[1]})")
        self.add(f"youare {player.value}")
        return self

    def preprocessing(self, time_ms: int = 3000) -> "CommandSequenceBuilder":
        """Add preprocessing phase."""
        self.add(f"startpreprocessing {time_ms}")
        return self

    def turn(self, turn_time_ms: Optional[int] = None) -> "CommandSequenceBuilder":
        """Add turn command."""
        if turn_time_ms:
            self.add(f"go {turn_time_ms}")
        else:
            self.add("go")
        return self

    def move_broadcast(
        self, rat_move: Direction, python_move: Direction
    ) -> "CommandSequenceBuilder":
        """Add move broadcast."""
        self.add(
            f"moves rat:{DIRECTION_INT_TO_NAME[rat_move]} python:{DIRECTION_INT_TO_NAME[python_move]}"
        )
        return self

    def position_update(
        self, player: Player, x: int, y: int
    ) -> "CommandSequenceBuilder":
        """Add position update."""
        self.add(f"current_position {player.value} ({x},{y})")
        return self

    def build(self) -> List[str]:
        """Get the command sequence."""
        return self.commands.copy()


class ProtocolExchangeBuilder:
    """Build complete protocol exchanges including AI responses."""

    def __init__(self):
        self.exchanges = []  # List of (sender, message) tuples

    def engine(self, command: str) -> "ProtocolExchangeBuilder":
        """Add an engine command."""
        self.exchanges.append(("engine", command))
        return self

    def ai(self, response: str) -> "ProtocolExchangeBuilder":
        """Add an AI response."""
        self.exchanges.append(("ai", response))
        return self

    def handshake_exchange(
        self, ai_name: str = "TestAI", author: str = "Tester"
    ) -> "ProtocolExchangeBuilder":
        """Add complete handshake exchange."""
        self.engine("pyrat")
        self.ai(f"pyratai {ai_name}")
        self.ai(f"id author {author}")
        self.ai("pyratready")
        return self

    def isready_exchange(self) -> "ProtocolExchangeBuilder":
        """Add isready/readyok exchange."""
        self.engine("isready")
        self.ai("readyok")
        return self

    def move_exchange(
        self, ai_move: Direction, rat_move: Direction, python_move: Direction
    ) -> "ProtocolExchangeBuilder":
        """Add complete move exchange."""
        self.engine("go")
        self.ai(f"move {DIRECTION_INT_TO_NAME[ai_move]}")
        self.engine(
            f"moves rat:{DIRECTION_INT_TO_NAME[rat_move]} python:{DIRECTION_INT_TO_NAME[python_move]}"
        )
        return self

    def get_engine_commands(self) -> List[str]:
        """Get only engine commands from the exchange."""
        return [msg for sender, msg in self.exchanges if sender == "engine"]

    def get_ai_responses(self) -> List[str]:
        """Get only AI responses from the exchange."""
        return [msg for sender, msg in self.exchanges if sender == "ai"]

    def build(self) -> List[Tuple[str, str]]:
        """Get the complete exchange."""
        return self.exchanges.copy()
