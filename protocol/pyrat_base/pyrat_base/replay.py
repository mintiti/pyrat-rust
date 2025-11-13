"""PyRat Replay Format (PRF) reader and writer implementation."""

import re
from dataclasses import dataclass, field
from pathlib import Path
from typing import Any, Dict, List, Optional, TextIO, Tuple, Union

from pyrat_engine.core.game import GameState as PyGameState
from pyrat_engine.core.types import Coordinates as Position
from pyrat_engine.core.types import Direction
from pyrat_engine.game import GameResult


@dataclass
class ReplayMetadata:
    """Metadata about a PyRat game."""

    event: str = "?"
    site: str = "?"
    date: str = "????.??.??"
    round_: str = "-"  # round is a Python keyword
    rat: str = "?"
    python: str = "?"
    result: str = "*"  # "1-0", "0-1", "1/2-1/2", or "*"
    maze_height: int = 10
    maze_width: int = 10
    time_control: str = "100+0+0"  # move+preprocessing+postprocessing

    # Optional tags
    rat_author: Optional[str] = None
    python_author: Optional[str] = None
    replay_id: Optional[str] = None
    termination: Optional[str] = None
    final_score: Optional[str] = None
    total_turns: Optional[int] = None

    # Additional custom tags
    custom_tags: Dict[str, str] = field(default_factory=dict)


@dataclass
class InitialState:
    """Initial game configuration in engine-compatible format."""

    width: int
    height: int
    walls: List[Tuple[Tuple[int, int], Tuple[int, int]]] = field(default_factory=list)
    mud: List[Tuple[Tuple[Tuple[int, int], Tuple[int, int]], int]] = field(
        default_factory=list
    )
    cheese: List[Tuple[int, int]] = field(default_factory=list)
    rat_position: Tuple[int, int] = (0, 0)
    python_position: Tuple[int, int] = (0, 0)
    max_turns: int = 300


@dataclass
class Move:
    """A single turn's moves and metadata."""

    turn: int
    rat_move: Union[int, str]  # Direction value (int) or "*" for timeout
    python_move: Union[int, str]  # Direction value (int) or "*" for timeout
    rat_time_ms: Optional[int] = None
    python_time_ms: Optional[int] = None
    comment: Optional[str] = None


@dataclass
class Replay:
    """Complete replay of a PyRat game."""

    metadata: ReplayMetadata
    initial_state: InitialState
    moves: List[Move] = field(default_factory=list)
    preprocessing_done: bool = False
    postprocessing_done: bool = False

    def to_prf(self) -> str:
        """Convert replay to PyRat Replay Format string."""
        writer = ReplayWriter()
        return writer.format_replay(self)


class ReplayReader:
    """Reads PyRat Replay Format files."""

    # Regex patterns for parsing
    TAG_PATTERN = re.compile(r'\[(\w+)\s+"([^"]+)"\]')
    WALL_PATTERN = re.compile(r"\((\d+),(\d+)\)-\((\d+),(\d+)\)")
    MUD_PATTERN = re.compile(r"\((\d+),(\d+)\)-\((\d+),(\d+)\):(\d+)")
    POSITION_PATTERN = re.compile(r"\((\d+),(\d+)\)")
    MOVE_PATTERN = re.compile(
        r"^(\d+)\.\s*([SUDLR*])/([SUDLR*])(?:\s*\((\d+)ms/(\d+)ms\))?(?:\s*\{([^}]*)\})?"
    )

    def read_file(self, path: Union[str, Path]) -> Replay:
        """Read a replay from a .pyrat file."""
        path = Path(path)
        with open(path, encoding="utf-8") as f:
            content = f.read()
        return self.parse(content)

    def parse(self, content: str) -> Replay:  # noqa: C901, PLR0912, PLR0915
        """Parse PRF content into a Replay object."""
        # Handle UTF-8 BOM if present (some Windows editors add this)
        if content.startswith("\ufeff"):
            content = content[1:]

        # Validate input
        content = content.strip()
        if not content:
            raise ValueError("Empty replay file")

        # Handle different line endings (Unix \n, Windows \r\n, old Mac \r)
        # Replace all line endings with \n for consistent processing
        content = content.replace("\r\n", "\n").replace("\r", "\n")
        lines = content.split("\n")

        # Check if file has any valid content (metadata tags or moves)
        has_metadata = any(
            line.strip().startswith("[") and line.strip().endswith("]")
            for line in lines
        )
        has_moves = any(
            line.strip() and not line.strip().startswith("[") for line in lines
        )

        if not has_metadata and not has_moves:
            raise ValueError("Invalid replay file: no metadata tags or moves found")

        metadata = ReplayMetadata()
        initial_state = None
        moves = []
        preprocessing_done = False
        postprocessing_done = False

        i = 0

        # Parse tag pairs
        while i < len(lines):
            line = lines[i].strip()
            if not line or line.startswith((";", "#")):
                i += 1
                continue

            match = self.TAG_PATTERN.match(line)
            if match:
                tag, value = match.groups()
                self._set_metadata_field(metadata, tag, value)
                i += 1
            else:
                break

        # Create initial state with dimensions from metadata
        initial_state = InitialState(
            width=metadata.maze_width,
            height=metadata.maze_height,
            max_turns=300,  # Default, may be overridden
        )

        # Parse initial state
        while i < len(lines):
            line = lines[i].strip()
            if not line or line.startswith((";", "#", "{")):
                i += 1
                continue

            if line.startswith("W:"):
                initial_state.walls = self._parse_walls(line[2:])
            elif line.startswith("M:"):
                initial_state.mud = self._parse_mud(line[2:])
            elif line.startswith("C:"):
                initial_state.cheese = self._parse_positions(line[2:])
            elif line.startswith("R:"):
                positions = self._parse_positions(line[2:])
                if positions:
                    initial_state.rat_position = positions[0]
            elif line.startswith("P:"):
                positions = self._parse_positions(line[2:])
                if positions:
                    initial_state.python_position = positions[0]
            elif line.startswith("!"):
                preprocessing_done = True
            elif line.startswith("?"):
                postprocessing_done = True
            else:
                # Probably hit moves section
                break
            i += 1

        # Parse moves
        while i < len(lines):
            line = lines[i].strip()
            if not line or line.startswith((";", "#")):
                i += 1
                continue

            if line.startswith("!"):
                preprocessing_done = True
            elif line.startswith("?"):
                postprocessing_done = True
            else:
                match = self.MOVE_PATTERN.match(line)
                if match:
                    turn = int(match.group(1))
                    rat_move = self._parse_move(match.group(2))
                    python_move = self._parse_move(match.group(3))
                    rat_time = int(match.group(4)) if match.group(4) else None
                    python_time = int(match.group(5)) if match.group(5) else None
                    comment = match.group(6) if match.group(6) else None

                    moves.append(
                        Move(
                            turn=turn,
                            rat_move=rat_move,
                            python_move=python_move,
                            rat_time_ms=rat_time,
                            python_time_ms=python_time,
                            comment=comment,
                        )
                    )
            i += 1

        return Replay(
            metadata=metadata,
            initial_state=initial_state,
            moves=moves,
            preprocessing_done=preprocessing_done,
            postprocessing_done=postprocessing_done,
        )

    def _set_metadata_field(
        self, metadata: ReplayMetadata, tag: str, value: str
    ) -> None:
        """Set metadata field from tag name and value."""
        # Map tag names to metadata fields
        tag_map = {
            "Event": "event",
            "Site": "site",
            "Date": "date",
            "Round": "round_",
            "Rat": "rat",
            "Python": "python",
            "Result": "result",
            "MazeHeight": "maze_height",
            "MazeWidth": "maze_width",
            "TimeControl": "time_control",
            "RatAuthor": "rat_author",
            "PythonAuthor": "python_author",
            "ReplayID": "replay_id",
            "Termination": "termination",
            "FinalScore": "final_score",
            "TotalTurns": "total_turns",
        }

        field_name = tag_map.get(tag)
        if field_name:
            if field_name in ("maze_height", "maze_width", "total_turns"):
                setattr(metadata, field_name, int(value))
            else:
                setattr(metadata, field_name, value)
        else:
            # Store unknown tags in custom_tags
            metadata.custom_tags[tag] = value

    def _parse_walls(self, text: str) -> List[Tuple[Tuple[int, int], Tuple[int, int]]]:
        """Parse wall list from format: (x1,y1)-(x2,y2) ..."""
        walls = []
        for match in self.WALL_PATTERN.finditer(text):
            x1, y1, x2, y2 = map(int, match.groups())
            walls.append(((x1, y1), (x2, y2)))
        return walls

    def _parse_mud(
        self, text: str
    ) -> List[Tuple[Tuple[Tuple[int, int], Tuple[int, int]], int]]:
        """Parse mud list from format: (x1,y1)-(x2,y2):N ..."""
        mud_zones = []
        for match in self.MUD_PATTERN.finditer(text):
            x1, y1, x2, y2, value = map(int, match.groups())
            mud_zones.append((((x1, y1), (x2, y2)), value))
        return mud_zones

    def _parse_positions(self, text: str) -> List[Tuple[int, int]]:
        """Parse position list from format: (x,y) ..."""
        positions = []
        for match in self.POSITION_PATTERN.finditer(text):
            x, y = map(int, match.groups())
            positions.append((x, y))
        return positions

    def _parse_move(self, text: str) -> Union[int, str]:
        """Parse move notation to Direction or special string."""
        if text == "*":
            return "*"  # Timeout

        move_map = {
            "S": Direction.STAY,
            "U": Direction.UP,
            "D": Direction.DOWN,
            "L": Direction.LEFT,
            "R": Direction.RIGHT,
        }
        return move_map.get(text, Direction.STAY)


class ReplayWriter:
    """Writes PyRat Replay Format files."""

    def write_file(self, replay: Replay, path: Union[str, Path]) -> None:
        """Write a replay to a .pyrat file."""
        path = Path(path)
        content = self.format_replay(replay)
        with open(path, "w", encoding="utf-8") as f:
            f.write(content)

    def format_replay(self, replay: Replay) -> str:  # noqa: C901, PLR0915
        """Format a Replay object as PRF content."""
        lines = []

        # Write required tag pairs
        meta = replay.metadata
        lines.append(f'[Event "{meta.event}"]')
        lines.append(f'[Site "{meta.site}"]')
        lines.append(f'[Date "{meta.date}"]')
        lines.append(f'[Round "{meta.round_}"]')
        lines.append(f'[Rat "{meta.rat}"]')
        lines.append(f'[Python "{meta.python}"]')
        lines.append(f'[Result "{meta.result}"]')
        lines.append(f'[MazeHeight "{meta.maze_height}"]')
        lines.append(f'[MazeWidth "{meta.maze_width}"]')
        lines.append(f'[TimeControl "{meta.time_control}"]')

        # Write optional tags
        if meta.rat_author:
            lines.append(f'[RatAuthor "{meta.rat_author}"]')
        if meta.python_author:
            lines.append(f'[PythonAuthor "{meta.python_author}"]')
        if meta.replay_id:
            lines.append(f'[ReplayID "{meta.replay_id}"]')
        if meta.termination:
            lines.append(f'[Termination "{meta.termination}"]')
        if meta.final_score:
            lines.append(f'[FinalScore "{meta.final_score}"]')
        if meta.total_turns is not None:
            lines.append(f'[TotalTurns "{meta.total_turns}"]')

        # Write custom tags
        for tag, value in meta.custom_tags.items():
            lines.append(f'[{tag} "{value}"]')

        lines.append("")  # Empty line before initial state

        # Write initial state
        state = replay.initial_state

        # Walls
        wall_strs = [
            f"({w[0][0]},{w[0][1]})-({w[1][0]},{w[1][1]})" for w in state.walls
        ]
        lines.append(f"W:{' '.join(wall_strs)}")

        # Mud
        mud_strs = [
            f"({m[0][0][0]},{m[0][0][1]})-({m[0][1][0]},{m[0][1][1]}):{m[1]}"
            for m in state.mud
        ]
        lines.append(f"M:{' '.join(mud_strs)}")

        # Cheese
        cheese_strs = [f"({c[0]},{c[1]})" for c in state.cheese]
        lines.append(f"C:{' '.join(cheese_strs)}")

        # Starting positions
        lines.append(f"R:({state.rat_position[0]},{state.rat_position[1]})")
        lines.append(f"P:({state.python_position[0]},{state.python_position[1]})")

        lines.append("")  # Empty line before moves

        # Preprocessing marker
        if replay.preprocessing_done:
            lines.append("! {Both players completed preprocessing}")
            lines.append("")

        # Write moves
        for move in replay.moves:
            line = f"{move.turn}. {self._format_move(move.rat_move)}/{self._format_move(move.python_move)}"

            # Add times if available
            if move.rat_time_ms is not None or move.python_time_ms is not None:
                rat_time = (
                    f"{move.rat_time_ms}ms" if move.rat_time_ms is not None else "?"
                )
                python_time = (
                    f"{move.python_time_ms}ms"
                    if move.python_time_ms is not None
                    else "?"
                )
                line += f" ({rat_time}/{python_time})"

            # Add comment if available
            if move.comment:
                line += f" {{{move.comment}}}"

            lines.append(line)

        # Postprocessing marker
        if replay.postprocessing_done:
            lines.append("")
            lines.append("? {Both players completed postprocessing}")

        return "\n".join(lines)

    def _format_move(self, move: Union[int, str]) -> str:
        """Format a move as PRF notation."""
        if isinstance(move, str):
            return move  # Special notation like '*'

        move_map = {
            Direction.STAY: "S",
            Direction.UP: "U",
            Direction.DOWN: "D",
            Direction.LEFT: "L",
            Direction.RIGHT: "R",
        }
        return move_map.get(move, "S")


class StreamingReplayWriter:
    """Writes replays incrementally during gameplay."""

    def __init__(self, path: Union[str, Path]):
        self.path = Path(path)
        self.file: Optional[TextIO] = None
        self.metadata_written = False
        self.initial_state_written = False

    def __enter__(self) -> "StreamingReplayWriter":
        self.file = open(self.path, "w", encoding="utf-8")
        return self

    def __exit__(self, exc_type: Any, exc_val: Any, exc_tb: Any) -> None:
        _ = exc_type, exc_val, exc_tb  # Unused parameters
        if self.file:
            self.file.close()

    def write_metadata(self, metadata: ReplayMetadata) -> None:
        """Write the metadata section."""
        if self.metadata_written:
            raise RuntimeError("Metadata already written")
        if self.file is None:
            raise RuntimeError("File not opened")

        writer = ReplayWriter()
        replay = Replay(metadata=metadata, initial_state=InitialState(0, 0))
        lines = writer.format_replay(replay).split("\n")

        # Write only metadata lines
        for line in lines:
            if line.strip() == "":
                break
            self.file.write(line + "\n")

        self.file.write("\n")
        self.metadata_written = True

    def write_initial_state(self, state: InitialState) -> None:
        """Write the initial state section."""
        if not self.metadata_written:
            raise RuntimeError("Must write metadata first")
        if self.initial_state_written:
            raise RuntimeError("Initial state already written")
        if self.file is None:
            raise RuntimeError("File not opened")

        writer = ReplayWriter()
        replay = Replay(metadata=ReplayMetadata(), initial_state=state)
        lines = writer.format_replay(replay).split("\n")

        # Find and write initial state lines
        in_state = False
        for line in lines:
            if line.startswith(("W:", "M:", "C:", "R:", "P:")):
                in_state = True
            if in_state:
                if line.strip() == "":
                    break
                self.file.write(line + "\n")

        self.file.write("\n")
        self.initial_state_written = True

    def write_preprocessing_done(self) -> None:
        """Write preprocessing completion marker."""
        if self.file is None:
            raise RuntimeError("File not opened")
        self.file.write("! {Both players completed preprocessing}\n\n")
        self.file.flush()

    def write_move(self, move: Move) -> None:
        """Write a single move."""
        if self.file is None:
            raise RuntimeError("File not opened")
        writer = ReplayWriter()
        line = f"{move.turn}. {writer._format_move(move.rat_move)}/{writer._format_move(move.python_move)}"

        if move.rat_time_ms is not None or move.python_time_ms is not None:
            rat_time = f"{move.rat_time_ms}ms" if move.rat_time_ms is not None else "?"
            python_time = (
                f"{move.python_time_ms}ms" if move.python_time_ms is not None else "?"
            )
            line += f" ({rat_time}/{python_time})"

        if move.comment:
            line += f" {{{move.comment}}}"

        self.file.write(line + "\n")
        self.file.flush()

    def write_postprocessing_done(self) -> None:
        """Write postprocessing completion marker."""
        if self.file is None:
            raise RuntimeError("File not opened")
        self.file.write("\n? {Both players completed postprocessing}\n")
        self.file.flush()


class ReplayPlayer:
    """Reconstructs and plays through a replay."""

    def __init__(self, replay: Replay):
        self.replay = replay
        self.game = self._create_game()
        self.current_turn = 0
        self._move_index = 0

    def _create_game(self) -> PyGameState:
        """Create game from initial state using PyGameState.create_custom()."""
        state = self.replay.initial_state

        # Convert walls and mud to the format expected by PyGameState
        walls = state.walls
        mud = [(cells[0], cells[1], value) for cells, value in state.mud]

        # Ensure at least one cheese exists (required by engine)
        cheese = (
            state.cheese if state.cheese else [(state.width // 2, state.height // 2)]
        )

        return PyGameState.create_custom(
            width=state.width,
            height=state.height,
            walls=walls,
            mud=mud,
            cheese=cheese,
            player1_pos=state.rat_position,
            player2_pos=state.python_position,
            max_turns=state.max_turns,
        )

    def step_forward(self) -> Optional[GameResult]:
        """Execute next move in replay."""
        if self._move_index >= len(self.replay.moves):
            return None

        move = self.replay.moves[self._move_index]

        # Handle timeout moves
        rat_move = Direction.STAY if isinstance(move.rat_move, str) else move.rat_move
        python_move = (
            Direction.STAY if isinstance(move.python_move, str) else move.python_move
        )

        # Execute the move
        # Direction is exposed as plain int constants, not enum with .value
        game_over, collected = self.game.step(rat_move, python_move)

        self._move_index += 1
        self.current_turn = move.turn

        return GameResult(
            game_over=game_over,
            collected_cheese=[Position(coord.x, coord.y) for coord in collected],
            p1_score=self.game.player1_score,
            p2_score=self.game.player2_score,
        )

    def jump_to_turn(self, turn: int) -> None:
        """Jump directly to a specific turn."""
        turn = max(turn, 0)

        # If going backwards, reset and replay
        if turn < self.current_turn:
            self.game = self._create_game()
            self._move_index = 0
            self.current_turn = 0

        # Play forward to the desired turn
        while self._move_index < len(self.replay.moves) and self.current_turn < turn:
            move = self.replay.moves[self._move_index]
            if move.turn > turn:
                break
            self.step_forward()

    def get_state(self) -> PyGameState:
        """Get current game state."""
        return self.game

    def is_finished(self) -> bool:
        """Check if replay has finished."""
        return self._move_index >= len(self.replay.moves)

    def get_move_at_current_turn(self) -> Optional[Move]:
        """Get the move that was just executed."""
        if self._move_index > 0 and self._move_index <= len(self.replay.moves):
            return self.replay.moves[self._move_index - 1]
        return None
