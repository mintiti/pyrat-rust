# PyRat Python SDK

Python SDK for writing PyRat bots. Your bot connects to the host over FlatBuffers, receives game state each turn, and returns a direction.

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

## The `Bot` contract

1. Subclass `Bot`
2. Override `think(state, ctx) -> Direction` (required)
3. Optionally override `preprocess(state, ctx)` for one-time setup
4. Call `.run()` from `__main__`

`think()` is called every turn. Return a `Direction`. If it raises, the SDK catches it and defaults to `STAY`.

`preprocess()` runs once before the game starts, with a separate time budget.

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

`player1_position`, `player2_position`, `player1_score`, `player2_score`, `player1_mud_turns`, `player2_mud_turns`

### Properties — shared

| Property | Type | Description |
|---|---|---|
| `cheese` | `list[(int, int)]` | Current cheese positions |
| `cheese_matrix` | `ndarray (w, h)` | uint8 matrix, 1 where cheese exists |
| `movement_matrix` | `ndarray (w, h, 4)` | int8: -1=wall, 0=free, N=mud cost per direction |
| `turn` | `int` | Current turn number |
| `width`, `height` | `int` | Maze dimensions |
| `max_turns` | `int` | Turn limit |

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
- `send_info(target=, depth=, nodes=, score=, path=, message=)` — send debug data to the host/GUI

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
