# PyRat Bot SDKs

Write a bot, plug it into a match, and watch it play. You implement one method, `think()`, and the SDK handles everything else: connecting to the host, receiving game state, sending your moves back.

Available in [Python](python/) and [Rust](rust/).

## Pick a language

| | |
|---|---|
| **[Python](python/)** | Quick to start, numpy arrays for matrix-based reasoning |
| **[Rust](rust/)** | 🔥 *blazingly fast* 🔥 |

## The bot contract

One required method: **`think(state, ctx) → Direction`**, called every turn.

**Python**

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

**Rust**

```rust
use pyrat_sdk::{Bot, Context, Direction, GameState, Options};

struct MyBot;
impl Options for MyBot {}

impl Bot for MyBot {
    fn think(&mut self, state: &GameState, _ctx: &Context) -> Direction {
        match state.nearest_cheese(None) {
            Some(r) if !r.path.is_empty() => r.path[0],
            _ => Direction::Stay,
        }
    }
}

fn main() {
    pyrat_sdk::run(MyBot, "MyBot", "Me");
}
```

That's a working bot: it connects, finds the nearest cheese, and walks toward it.

### Lifecycle

1. **Connect**: SDK reads `PYRAT_HOST_PORT` and connects
2. **Configure**: host sends match config and option overrides
3. **Preprocess**: `preprocess(state, ctx)` runs once before the game with a longer timeout, for precomputing distance tables, loading models, whatever you need *(optional)*
4. **Turn loop**: `think(state, ctx)` each turn, with a time budget
5. **Game over**: `on_game_over(result, scores)` when the match ends *(optional)*

If `think()` crashes, the SDK catches it and defaults to STAY — see [Crashes and debugging](#crashes-and-debugging).

## What GameState gives you

`GameState` is the single object passed to `think()`, and everything you need is on it.

### Raw state: what's happening right now

The game snapshot, updated every turn. It's **perspective-mapped**, meaning you always use `my_position` / `opponent_position` and your code works regardless of which player you are:

```python
pos = state.my_position           # your (x, y)
opp = state.opponent_position     # opponent's (x, y)
score = state.my_score
mud = state.my_mud_turns          # 0 = free to move
cheese = state.cheese             # list of (x, y) positions
```

Also available: `my_last_move`, `opponent_last_move`, `opponent_score`, `opponent_mud_turns`, `turn`.

For querying the maze itself:

```python
moves = state.effective_moves()      # passable directions from your position
cost = state.move_cost(Direction.UP)  # None=wall, 1=free, N=mud turns
```

Python also provides **`movement_matrix`** (numpy `int8`, `[x, y, direction]`) and **`cheese_matrix`** (numpy `uint8`) for array-based reasoning.

### Pathfinding: where should I go

Pathfinding ships built-in so you don't have to write Dijkstra yourself. All methods account for mud costs and are backed by the Rust engine.

```python
result = state.nearest_cheese()            # closest cheese with full path
if result:
    first_step = result.directions[0]      # Direction to move
    cost_in_turns = result.cost

path = state.shortest_path(start, goal)    # between any two cells
dists = state.distances_from()             # cost to every reachable cell
```

This is what the greedy example bot does: find the nearest cheese and follow the path.

### Simulation: what happens if I go *there*?

This is where it gets interesting. `to_sim()` gives you a mutable copy of the whole game, backed by the Rust engine, and you can explore the game tree with `make_move` / `unmake_move` without cloning anything.

```python
sim = state.to_sim()

undo = sim.make_move(Direction.RIGHT, Direction.LEFT)
print(sim.player1_score, sim.is_game_over)

sim.unmake_move(undo)  # back to where we were
```

Undo tokens are LIFO. In Rust, `GameSim` also implements `Clone` if you want parallel search.

This is minimax and MCTS territory. The `search` example bot in both SDKs shows a working tree search with iterative deepening if you want to see what that looks like.

The [Python](python/) and [Rust](rust/) READMEs have the full GameState reference — every property, method signature, and return type.

## Thinking out loud

Your bot can tell the GUI what it's thinking: target cell, planned path, search depth, debug messages. Really useful for watching your bot reason in real time during development.

```python
ctx.send_info(
    target=result.position,
    path=[result.position],
    depth=4,
    message=f"heading to cheese at {result.position}",
)
```

| Parameter | Type | What it does |
|-----------|------|-------------|
| `target` | coordinate | Cell the bot is heading toward (shown in GUI) |
| `path` | list of coordinates | Planned route (drawn on the maze) |
| `depth` | int | Search depth reached |
| `nodes` | int | Nodes evaluated |
| `score` | float | Evaluation score |
| `message` | str | Free-form debug text |

Available as `ctx.send_info(...)` in both SDKs.

## Crashes and debugging

- **If `think()` crashes**, the SDK catches it, defaults to STAY, and the game continues. Traceback prints to stderr.
- **`print()` just works.** The SDK talks to the host over TCP, not stdin/stdout, so print whatever you want.

## Options

Bots can declare tunable parameters (search depth, strategy, model path, that kind of thing) that the host or GUI can override before a match starts, without you having to change any code.

**Python**

```python
from pyrat_sdk import Bot, Spin, Check, Combo, Str

class MyBot(Bot):
    depth = Spin(default=3, min=1, max=10)
    avoid_mud = Check(default=True)
    strategy = Combo(default="greedy", choices=["greedy", "defensive"])
    model_path = Str(default="")
```

**Rust**

```rust
#[derive(DeriveOptions)]
struct MyBot {
    #[spin(default = 3, min = 1, max = 10)]
    depth: i32,

    #[check(default = true)]
    avoid_mud: bool,

    #[combo(default = "greedy", choices = ["greedy", "defensive"])]
    strategy: String,

    #[str_opt(default = "")]
    model_path: String,
}
```

Four types: **Spin** (bounded int), **Check** (bool), **Combo** (constrained string), **Str** (free-form). Full details in the [Python](python/) and [Rust](rust/) READMEs.

## Running matches

The headless runner launches two bots as subprocesses and runs them against each other:

```bash
# Rust vs Rust
cargo run -p pyrat-headless -- \
  "cargo run -p pyrat-sdk --example greedy" \
  "cargo run -p pyrat-sdk --example search"

# Python vs Python
cargo run -p pyrat-headless -- \
  "uv run python sdk/python/examples/greedy.py" \
  "uv run python sdk/python/examples/search.py"
```

For visual matches, use the [GUI](../gui/). To make your bot show up in the GUI, add a `bot.toml` to its directory. See the [botpack README](../botpack/#bottoml) for the format.
