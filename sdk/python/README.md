# PyRat Python SDK

Python library for writing PyRat bots. For the conceptual overview (bot lifecycle, GameState facets, how matches work), see the [SDK README](../). This document is the full Python API reference.

Requires **Python 3.10+**.

## Getting started

The SDK includes a Rust extension (pathfinding, simulation) built with [maturin](https://www.maturin.rs/). From the repo root:

```bash
uv sync --all-extras
cd sdk/python && uv run maturin develop --release && cd ../..
```

Run a match between two botpack bots:

```bash
cargo run -p pyrat-headless -- \
  "cd botpack/greedy-py && uv run python bot.py" \
  "cd botpack/smart-random-py && uv run python bot.py"
```

### Minimal bot

```python
from pyrat_sdk import Bot, Context, Direction, GameState


class MyBot(Bot):
    name = "MyBot"
    author = "Me"

    def think(self, state: GameState, ctx: Context) -> Direction:
        result = state.nearest_cheese()
        return result.directions[0] if result else Direction.STAY


if __name__ == "__main__":
    MyBot().run()
```

`print()` works for debugging: the SDK talks to the host over TCP, not stdin/stdout.

## The bot contract

Subclass `Bot`, implement `think()`, call `.run()`.

```python
class Bot:
    name: str   # shown to host/GUI
    author: str

    def think(self, state: GameState, ctx: Context) -> Direction: ...
    def preprocess(self, state: GameState, ctx: Context) -> None: ...
    def on_game_over(self, result: GameResult, scores: tuple[float, float]) -> None: ...
    def run(self) -> None: ...
```

**`think(state, ctx) → Direction`** is the only required method. Called every turn. If it raises, the SDK catches the exception, prints the traceback, and sends STAY.

**`preprocess(state, ctx)`** runs once before the game starts, with a separate (longer) time budget. Use it to precompute distance tables, load models, or anything too expensive to redo each turn.

**`on_game_over(result, scores)`** is called when the match ends. `result` is a `GameResult` (PLAYER1, PLAYER2, or DRAW). `scores` is `(player1_score, player2_score)`.

**`run()`** connects to the host, runs the full lifecycle, and exits. The [SDK README](../) covers the lifecycle in detail.

## Coordinate system

Coordinates are `(x, y)` tuples with `(0, 0)` at the bottom-left corner. `UP` increases `y`, `RIGHT` increases `x`.

## `GameState` reference

### Properties: perspective-aware (use these)

| Property | Type | Description |
|---|---|---|
| `my_position` | `(int, int)` | Your (x, y) position |
| `opponent_position` | `(int, int)` | Opponent's (x, y) position |
| `my_score` | `float` | Your score |
| `opponent_score` | `float` | Opponent's score |
| `my_mud_turns` | `int` | Turns stuck in mud (0 = free) |
| `opponent_mud_turns` | `int` | Opponent's mud turns |
| `my_last_move` | `Direction` | Your last move |
| `opponent_last_move` | `Direction` | Opponent's last move |
| `my_player` | `Player` | Which player you are (PLAYER1 or PLAYER2) |

### Properties: raw (for HivemindBot)

| Property | Type | Description |
|---|---|---|
| `player1_position` | `(int, int)` | Player 1's position |
| `player2_position` | `(int, int)` | Player 2's position |
| `player1_score` | `float` | Player 1's score |
| `player2_score` | `float` | Player 2's score |
| `player1_mud_turns` | `int` | Player 1's mud turns |
| `player2_mud_turns` | `int` | Player 2's mud turns |
| `player1_last_move` | `Direction` | Player 1's last move |
| `player2_last_move` | `Direction` | Player 2's last move |

### Properties: shared

| Property | Type | Description |
|---|---|---|
| `cheese` | `list[(int, int)]` | Remaining cheese positions |
| `cheese_matrix` | `ndarray (w, h)` | `uint8`, 1 where cheese exists |
| `movement_matrix` | `ndarray (w, h, 4)` | `int8`, encodes moves per direction (see below) |
| `turn` | `int` | Current turn number |
| `width`, `height` | `int` | Maze dimensions |
| `max_turns` | `int` | Turn limit |
| `move_timeout_ms` | `int` | Milliseconds allowed per `think()` call |
| `preprocessing_timeout_ms` | `int` | Milliseconds allowed for `preprocess()` |

#### `movement_matrix` encoding

Indexed as `[x, y, direction]` where direction is 0=UP, 1=RIGHT, 2=DOWN, 3=LEFT.

- `-1` : wall (can't move)
- `0` : free passage (1 turn to traverse)
- `N > 0` : mud passage (N turns to traverse)

```python
matrix = state.movement_matrix
# Check if you can move UP from (x, y):
if matrix[x, y, 0] != -1:
    cost = max(1, matrix[x, y, 0])  # free passages are 0 in the matrix, 1 turn actual cost
```

### Methods

| Method | Returns | Description |
|---|---|---|
| `effective_moves(pos=None)` | `list[Direction]` | Non-wall directions from pos (default: `my_position`) |
| `move_cost(direction, pos=None)` | `int \| None` | `None`=wall, `1`=free, `N`=mud turns |
| `shortest_path(start, goal)` | `PathResult \| None` | Dijkstra path with total cost |
| `nearest_cheese(pos=None)` | `NearestCheeseResult \| None` | Closest cheese with path and cost |
| `nearest_cheeses(pos=None)` | `list[NearestCheeseResult]` | All cheeses tied at minimum distance |
| `distances_from(pos=None)` | `dict[(int,int), int]` | Cost in turns to every reachable cell |
| `to_sim()` | `GameSim` | Mutable game copy for tree search |

All pathfinding methods account for mud costs and are backed by Dijkstra in Rust. Methods that take `pos` default to `my_position`.

**`PathResult`** is a NamedTuple: `(directions: list[Direction], cost: int)`

**`NearestCheeseResult`** is a NamedTuple: `(position: (int, int), directions: list[Direction], cost: int)`

### Simulation (tree search)

`state.to_sim()` returns a `GameSim`, a mutable snapshot of the game backed by the Rust engine. Use `make_move` / `unmake_move` for game-tree search (minimax, MCTS, etc.) without cloning state.

```python
sim = state.to_sim()

undo = sim.make_move(int(Direction.RIGHT), int(Direction.LEFT))
print(sim.player1_score, sim.is_game_over)

sim.unmake_move(undo)  # back to where we were
```

Directions must be passed as `int` (use `int(Direction.RIGHT)`). Undo tokens must be applied in LIFO order (most recent first).

`GameSim` supports `copy.copy()` and `copy.deepcopy()` for branching search trees.

**`GameSim` properties:**

| Property | Type | Description |
|---|---|---|
| `player1_position` | `(int, int)` | Player 1's current position |
| `player2_position` | `(int, int)` | Player 2's current position |
| `player1_score` | `float` | Player 1's score |
| `player2_score` | `float` | Player 2's score |
| `player1_mud_turns` | `int` | Player 1's remaining mud turns |
| `player2_mud_turns` | `int` | Player 2's remaining mud turns |
| `cheese_positions` | `list[(int, int)]` | Remaining cheese |
| `turn` | `int` | Current turn |
| `max_turns` | `int` | Turn limit |
| `is_game_over` | `bool` | True if the game has ended |

**`MoveUndo` properties** (read-only, returned by `make_move`):

| Property | Type | Description |
|---|---|---|
| `p1_pos` | `(int, int)` | Player 1's position before the move |
| `p2_pos` | `(int, int)` | Player 2's position before the move |
| `p1_score` | `float` | Player 1's score before the move |
| `p2_score` | `float` | Player 2's score before the move |
| `p1_mud` | `int` | Player 1's mud turns before the move |
| `p2_mud` | `int` | Player 2's mud turns before the move |
| `collected_cheese` | `list[(int, int)]` | Cheese collected during this move |
| `turn` | `int` | Turn number before the move |

Both `GameSim` and `MoveUndo` are importable from `pyrat_sdk`.

## `Direction` values

| Name | Value |
|---|---|
| `UP` | 0 |
| `RIGHT` | 1 |
| `DOWN` | 2 |
| `LEFT` | 3 |
| `STAY` | 4 |

`Direction` is an `IntEnum`, so you can use it anywhere an `int` is expected.

## `Context`

Passed to `think()` and `preprocess()`.

- **`time_remaining_ms()`** returns milliseconds left before the deadline (0.0 when expired)
- **`should_stop()`** returns `True` when the deadline has passed, useful for iterative deepening loops

### `send_info()`

Send debug and visualization data to the host/GUI. `player` is required; all other parameters are keyword-only with defaults:

| Parameter | Type | Default | Description |
|---|---|---|---|
| `player` | `Player` | — | Which player this info is about |
| `multipv` | `int` | `0` | Principal variation index (0 for single line) |
| `target` | `(int, int) \| None` | `None` | Cell the bot is heading toward |
| `depth` | `int` | `0` | Search depth reached |
| `nodes` | `int` | `0` | Nodes evaluated |
| `score` | `float` | `0.0` | Evaluation score |
| `pv` | `list[Direction] \| None` | `None` | Principal variation (sequence of moves) |
| `message` | `str` | `""` | Free-form debug text |

```python
ctx.send_info(
    player=state.my_player,
    target=result.position,
    depth=4,
    message=f"heading to cheese at {result.position}",
)
```

## `HivemindBot`

Controls both players. `think()` returns a dict mapping each player to a direction:

```python
from pyrat_sdk import HivemindBot, Direction, Player

class MyHivemind(HivemindBot):
    name = "My Hivemind"
    author = "Me"

    def think(self, state, ctx) -> dict[Player, Direction]:
        return {
            Player.PLAYER1: Direction.UP,
            Player.PLAYER2: Direction.DOWN,
        }

if __name__ == "__main__":
    MyHivemind().run()
```

Uses the raw `player1_*` / `player2_*` properties on `GameState` (no my/opponent mapping). Missing keys default to STAY.

## Options

Declare tunable parameters as class attributes. The host or GUI can override them before a match starts.

```python
from pyrat_sdk import Bot, Spin, Check, Combo, Str

class MyBot(Bot):
    name = "Greedy+"
    depth = Spin(default=3, min=1, max=10)
    avoid_mud = Check(default=True)
    strategy = Combo(default="greedy", choices=["greedy", "defensive"])
    model_path = Str(default="")

    def think(self, state, ctx):
        if self.depth > 5:  # resolves to int (default or host-set)
            ...
```

| Type | Constructor | Resolves to |
|---|---|---|
| `Spin` | `Spin(default, min, max)` | `int` |
| `Check` | `Check(default)` | `bool` |
| `Combo` | `Combo(default, choices)` | `str` |
| `Str` | `Str(default)` | `str` |
