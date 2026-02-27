# PyRat Python SDK

Python SDK for writing PyRat bots. Your bot connects to the host over FlatBuffers, receives game state each turn, and returns a direction.

Requires **Python 3.10+**.

## Quick start

```bash
# From the repo root
uv sync --all-extras
cd sdk-python && uv run maturin develop --release && cd ..

# Run a match (the host sets PYRAT_HOST_PORT for you)
pyrat-game sdk-python/examples/greedy.py sdk-python/examples/smart_random.py
```

### Minimal bot

```python
from pyrat_sdk import Bot, Context, Direction, GameState


class MyBot(Bot):
    name = "MyBot"
    author = "Me"

    def think(self, state: GameState, ctx: Context) -> Direction:
        result = state.nearest_cheese()
        if result is not None and result.directions:
            return result.directions[0]
        return Direction.STAY


if __name__ == "__main__":
    MyBot().run()
```

You can use `print()` for debugging — the SDK communicates over TCP, not stdin/stdout.

## The `Bot` contract

1. Subclass `Bot`
2. Override `think(state, ctx) -> Direction` (required)
3. Optionally override `preprocess(state, ctx)` for one-time setup
4. Call `.run()` from `__main__`

`think()` is called every turn. Return a `Direction`. If it raises, the SDK catches it and defaults to `STAY`.

`preprocess()` runs once before the game starts, with a separate time budget. Use it to precompute distance tables or other data expensive to rebuild each turn.

## Coordinate system

Coordinates are `(x, y)` with `(0, 0)` at the bottom-left. `UP` increases `y`, `RIGHT` increases `x`.

## `GameState` reference

### Properties — perspective-aware (use these)

| Property | Type | Description |
|---|---|---|
| `my_position` | `(int, int)` | This bot's (x, y) position |
| `opponent_position` | `(int, int)` | Opponent's (x, y) position |
| `my_score` | `float` | This bot's score |
| `opponent_score` | `float` | Opponent's score |
| `my_mud_turns` | `int` | Turns stuck in mud (0 = free) |
| `opponent_mud_turns` | `int` | Opponent's mud turns |
| `my_last_move` | `Direction` | This bot's last move |
| `opponent_last_move` | `Direction` | Opponent's last move |
| `my_player` | `Player` | Which player this bot is (PLAYER1/PLAYER2) |

### Properties — raw (for HivemindBot)

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

### Properties — shared

| Property | Type | Description |
|---|---|---|
| `cheese` | `list[(int, int)]` | Current cheese positions |
| `cheese_matrix` | `ndarray (w, h)` | uint8 matrix, 1 where cheese exists |
| `movement_matrix` | `ndarray (w, h, 4)` | int8 encoding of moves per direction (see below) |
| `turn` | `int` | Current turn number |
| `width`, `height` | `int` | Maze dimensions |
| `max_turns` | `int` | Turn limit |

#### `movement_matrix` encoding

The matrix is indexed `[x, y, direction]` where direction is 0=UP, 1=RIGHT, 2=DOWN, 3=LEFT.

- `-1` — wall (can't move)
- `0` — free passage (costs 1 turn)
- `N > 0` — mud passage (costs N turns)

```python
matrix = state.movement_matrix
# Check if you can move UP from (x, y):
if matrix[x, y, 0] != -1:
    cost = max(1, matrix[x, y, 0])  # free passages are 0 in the matrix, 1 turn actual cost
```

### Methods

| Method | Returns | Description |
|---|---|---|
| `get_effective_moves(pos=None)` | `list[Direction]` | Non-wall directions from pos |
| `get_move_cost(direction, pos=None)` | `int \| None` | None=wall, 1=free, N=mud turns |
| `shortest_path(start, goal)` | `PathResult \| None` | Full path with cost in turns |
| `nearest_cheese(pos=None)` | `NearestCheeseResult \| None` | Closest cheese with path and cost |
| `distances_from(pos=None)` | `dict[(int,int), int]` | Cost to every reachable cell |

`PathResult` is a NamedTuple: `(directions: list[Direction], cost: int)`

`NearestCheeseResult` is a NamedTuple: `(position: (int, int), directions: list[Direction], cost: int)`

## Direction values

| Name | Value |
|---|---|
| `UP` | 0 |
| `RIGHT` | 1 |
| `DOWN` | 2 |
| `LEFT` | 3 |
| `STAY` | 4 |

## `Context`

Passed to `think()` and `preprocess()`. Provides:

- `time_remaining_ms()` — milliseconds left in this phase
- `should_stop()` — True when time is up (for iterative deepening)
- `send_info(...)` — send debug data to the host/GUI

### `send_info()` parameters

All keyword-only, all optional:

| Parameter | Type | Description |
|---|---|---|
| `target` | `(int, int)` | Cell the bot is heading toward (shown in GUI) |
| `depth` | `int` | Search depth reached |
| `nodes` | `int` | Nodes evaluated |
| `score` | `float` | Evaluation score |
| `path` | `list[(int, int)]` | Planned path (shown in GUI) |
| `message` | `str` | Free-form debug message |

## `HivemindBot`

Controls both players. Override `think()` to return a dict:

```python
def think(self, state, ctx) -> dict[Player, Direction]:
    return {
        Player.PLAYER1: Direction.UP,
        Player.PLAYER2: Direction.DOWN,
    }
```

Uses the raw `player1_*`/`player2_*` properties on `GameState` for per-player data.

## Options

Declare tunable parameters as class attributes. The host can override them via `SetOption` before the game starts.

```python
from pyrat_sdk import Bot, Direction, Spin, Check, Combo, Str

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

| Type | Description | Example |
|---|---|---|
| `Spin(default, min, max)` | Integer in a range | `Spin(default=3, min=1, max=10)` |
| `Check(default)` | Boolean | `Check(default=True)` |
| `Combo(default, choices)` | String from fixed set | `Combo(default="greedy", choices=[...])` |
| `Str(default)` | Free-form string | `Str(default="")` |
