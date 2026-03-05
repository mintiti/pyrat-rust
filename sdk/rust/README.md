# PyRat Rust SDK

Rust SDK for writing PyRat bots. Your bot connects to the host over FlatBuffers, receives game state each turn, and returns a direction.

## Quick start

Add the dependency (path-based within the monorepo):

```toml
[dependencies]
pyrat-sdk = { path = "../sdk/rust" }
```

### Minimal bot

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

### Running a match

```bash
# From the repo root
cargo run -p pyrat-headless -- \
  "cargo run -p pyrat-sdk --example greedy" \
  "cargo run -p pyrat-sdk --example smart_random"
```

## The `Bot` contract

1. Implement `Options` + `Bot` on your struct
2. Call `pyrat_sdk::run(bot, name, author)` from `main()`

Lifecycle order:
1. `Options::apply_option()` — called for each host-set option
2. `Bot::preprocess(state, ctx)` — one-time setup with a longer timeout
3. `Bot::think(state, ctx)` — called every turn, return a `Direction`
4. `Bot::on_game_over(result, scores)` — optional cleanup

If `think()` panics, the SDK catches it and defaults to `Stay`.

## Coordinate system

Coordinates are `(x, y)` with `(0, 0)` at the bottom-left. `Up` increases `y`, `Right` increases `x`.

## `GameState` reference

### Methods — perspective-aware (use these)

| Method | Returns | Description |
|---|---|---|
| `my_position()` | `Coordinates` | This bot's position |
| `opponent_position()` | `Coordinates` | Opponent's position |
| `my_score()` | `f32` | This bot's score |
| `opponent_score()` | `f32` | Opponent's score |
| `my_mud_turns()` | `u8` | Turns stuck in mud (0 = free) |
| `opponent_mud_turns()` | `u8` | Opponent's mud turns |
| `my_last_move()` | `Direction` | This bot's last move |
| `opponent_last_move()` | `Direction` | Opponent's last move |
| `my_player()` | `Player` | Which player this bot is |
| `controlled_players()` | `&[Player]` | All controlled players (usually one) |

### Methods — raw (for hivemind bots)

| Method | Returns | Description |
|---|---|---|
| `player1_position()` | `Coordinates` | Player 1's position |
| `player2_position()` | `Coordinates` | Player 2's position |
| `player1_score()` | `f32` | Player 1's score |
| `player2_score()` | `f32` | Player 2's score |
| `player1_mud_turns()` | `u8` | Player 1's mud turns |
| `player2_mud_turns()` | `u8` | Player 2's mud turns |
| `player1_last_move()` | `Direction` | Player 1's last move |
| `player2_last_move()` | `Direction` | Player 2's last move |

### Methods — shared

| Method | Returns | Description |
|---|---|---|
| `cheese()` | `&[Coordinates]` | Current cheese positions |
| `turn()` | `u16` | Current turn number |
| `width()` | `u8` | Maze width |
| `height()` | `u8` | Maze height |
| `max_turns()` | `u16` | Turn limit |
| `move_timeout_ms()` | `u32` | Per-turn time budget (ms) |
| `preprocessing_timeout_ms()` | `u32` | Preprocessing time budget (ms) |

### Convenience methods

| Method | Returns | Description |
|---|---|---|
| `effective_moves(pos)` | `Vec<Direction>` | Non-wall directions from `pos` |
| `move_cost(dir, pos)` | `Option<u8>` | `None`=wall, `Some(1)`=free, `Some(n)`=mud turns |
| `shortest_path(from, to)` | `Option<FullPathResult>` | Full path with cost in turns |
| `nearest_cheese(pos)` | `Option<FullPathResult>` | Closest cheese (first of ties) |
| `nearest_cheeses(pos)` | `Vec<FullPathResult>` | All cheeses tied at minimum distance |
| `distances_from(pos)` | `HashMap<Coordinates, u32>` | Cost to every reachable cell |
| `simulate()` | `GameSim` | Mutable snapshot for tree search |
| `view()` | `&GameView` | Read-only access to maze topology |

All `pos` parameters are `Option<Coordinates>` — pass `None` to default to `my_position()`.

`FullPathResult` fields: `target: Coordinates`, `path: Vec<Direction>`, `first_moves: Vec<Direction>`, `cost: u32`.

## Simulation (tree search)

`state.simulate()` returns a `GameSim` — a mutable game snapshot backed by the Rust engine. Use it for game-tree search (minimax, MCTS, etc.) via `make_move` / `unmake_move`.

```rust
let mut sim = state.simulate();

// Advance one turn: both players move simultaneously.
let undo = sim.make_move(Direction::Right, Direction::Left);
println!("{} {}", sim.player1_score(), sim.is_game_over());

// Revert to the previous state.
sim.unmake_move(undo);
```

`GameSim` implements `Clone` for parallel search.

**`GameSim` methods:** `player1_position()`, `player2_position()`, `player1_score()`, `player2_score()`, `player1_mud_turns()`, `player2_mud_turns()`, `cheese_positions()`, `turn()`, `max_turns()`, `is_game_over()`

`MoveUndo` is the undo token returned by `make_move`. Apply tokens in LIFO order (most recent first).

## `Direction` enum

| Variant | Meaning |
|---|---|
| `Up` | +y |
| `Right` | +x |
| `Down` | -y |
| `Left` | -x |
| `Stay` | No movement |

## `Context`

Passed to `think()` and `preprocess()`:

- `should_stop()` — `true` when time is up (for iterative deepening)
- `time_remaining_ms()` — milliseconds left in this phase

## `Hivemind`

Controls both players. Implement `Hivemind` instead of `Bot`:

```rust
use pyrat_sdk::{Context, Direction, GameState, Hivemind, Options, Player};

struct MyHivemind;
impl Options for MyHivemind {}

impl Hivemind for MyHivemind {
    fn think(&mut self, _state: &GameState, _ctx: &Context) -> [(Player, Direction); 2] {
        [
            (Player::Player1, Direction::Up),
            (Player::Player2, Direction::Down),
        ]
    }
}

fn main() {
    pyrat_sdk::run_hivemind(MyHivemind, "Hive", "Me");
}
```

Uses the raw `player1_*`/`player2_*` methods on `GameState` for per-player data.

## Options

Declare tunable parameters with the `DeriveOptions` macro. The host can override them via `SetOption` before the game starts. Fields without option attributes are ignored by the derive.

```rust
use pyrat_sdk::{Bot, Context, Direction, GameState, DeriveOptions};

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

    // No attribute — not an option, ignored by derive
    cache: Vec<u32>,
}
```

| Attribute | Field type | Description |
|---|---|---|
| `#[spin(default, min, max)]` | `i32` | Integer in a range |
| `#[check(default)]` | `bool` | Boolean toggle |
| `#[combo(default, choices)]` | `String` | String from a fixed set |
| `#[str_opt(default)]` | `String` | Free-form string |
