# PyRat Rust SDK

Rust library for writing PyRat bots. For the conceptual overview (bot lifecycle, GameState facets, how matches work), see the [SDK README](../).

## Getting started

Add the dependency (path-based within the monorepo):

```toml
[dependencies]
pyrat-sdk = { path = "../sdk/rust" }
```

Run a match between two botpack bots:

```bash
cargo run -p pyrat-headless -- \
  "cd botpack/greedy && cargo run --release" \
  "cd botpack/smart-random && cargo run --release"
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

## The bot contract

Implement `Options` + `Bot` on your struct, then call `pyrat_sdk::run(bot, name, author)` from `main()`.

```rust
pub trait Bot: Options {
    fn think(&mut self, state: &GameState, ctx: &Context) -> Direction;
    fn preprocess(&mut self, _state: &GameState, _ctx: &Context) {}
    fn on_game_over(&mut self, _result: GameResult, _scores: (f32, f32)) {}
}
```

**`think`** is the only required method. Called every turn. If it panics, the SDK catches it and defaults to `Stay`.

**`preprocess`** runs once before the game with a separate (longer) time budget. Use it to precompute distance tables, build caches, or anything too expensive to redo each turn.

**`on_game_over`** is called when the match ends. `GameResult` is `Player1`, `Player2`, or `Draw`. Scores are `(player1_score, player2_score)`.

## Coordinate system

Coordinates are `Coordinates` objects with `.x` and `.y` fields. `(0, 0)` is at the bottom-left. `Up` increases `y`, `Right` increases `x`.

## `GameState` reference

### Methods: perspective-aware (use these)

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

### Methods: raw (for Hivemind)

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

### Methods: shared

| Method | Returns | Description |
|---|---|---|
| `cheese()` | `&[Coordinates]` | Remaining cheese positions |
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
| `move_cost(dir, pos)` | `Option<u8>` | `None`=wall, `Some(1)`=free, `Some(n)`=mud |
| `shortest_path(from, to)` | `Option<FullPathResult>` | Dijkstra path with cost |
| `nearest_cheese(pos)` | `Option<FullPathResult>` | Closest cheese (first of ties) |
| `nearest_cheeses(pos)` | `Vec<FullPathResult>` | All cheeses tied at minimum distance |
| `distances_from(pos)` | `HashMap<Coordinates, u32>` | Cost to every reachable cell |
| `to_sim()` | `GameSim` | Mutable snapshot for tree search |
| `view()` | `&GameView` | Read-only access to maze topology |

All `pos` parameters are `Option<Coordinates>`. Pass `None` to default to `my_position()`.

`FullPathResult` fields: `target: Coordinates`, `path: Vec<Direction>`, `first_moves: Vec<Direction>`, `cost: u32`.

## Simulation (tree search)

`state.to_sim()` returns a `GameSim`, a mutable game snapshot backed by the Rust engine. Use `make_move` / `unmake_move` for game-tree search (minimax, MCTS, etc.) without cloning state.

```rust
let mut sim = state.to_sim();

let undo = sim.make_move(Direction::Right, Direction::Left);
println!("{} {}", sim.player1_score(), sim.check_game_over());

sim.unmake_move(undo);  // back to previous state
```

`GameSim` implements `Clone` for parallel search.

**`GameSim` methods:**

| Method | Returns |
|---|---|
| `player1_position()` | `Coordinates` |
| `player2_position()` | `Coordinates` |
| `player1_score()` | `f32` |
| `player2_score()` | `f32` |
| `player1_mud_turns()` | `u8` |
| `player2_mud_turns()` | `u8` |
| `cheese_positions()` | `Vec<Coordinates>` |
| `turn` | `u16` |
| `max_turns` | `u16` |
| `check_game_over()` | `bool` |

`MoveUndo` is the undo token returned by `make_move`. Apply in LIFO order (most recent first).

## `Direction` enum

| Variant | Meaning |
|---|---|
| `Up` | +y |
| `Right` | +x |
| `Down` | -y |
| `Left` | -x |
| `Stay` | No movement |

## `Context`

Passed to `think()` and `preprocess()`.

- **`should_stop()`** returns `true` when the deadline has passed, useful for iterative deepening loops
- **`time_remaining_ms()`** returns milliseconds left before the deadline (0 when expired)

### `send_info()`

Send debug and visualization data to the host/GUI during `think()`. Takes an `InfoParams` struct:

```rust
use pyrat_sdk::InfoParams;

ctx.send_info(&InfoParams {
    depth: 5,
    score: Some(3.0),
    message: "best: Right",
    ..InfoParams::for_player(state.my_player())
});
```

| Field | Type | Description |
|---|---|---|
| `player` | `Player` | Which player this info is about |
| `multipv` | `u16` | Principal variation index (0 for single line) |
| `target` | `Option<Coordinates>` | Cell the bot is heading toward |
| `depth` | `u16` | Search depth reached |
| `nodes` | `u32` | Nodes evaluated |
| `score` | `Option<f32>` | Evaluation score |
| `pv` | `&[Direction]` | Principal variation (sequence of moves) |
| `message` | `&str` | Free-form debug text |

`InfoParams::for_player(player)` sets `player` and zeroes everything else — use struct update syntax to override what you need.

### `info_sender()`

For multi-threaded bots, clone the sender and move it into worker threads:

```rust
if let Some(sender) = ctx.info_sender() {
    let player = state.my_player();
    std::thread::spawn(move || {
        sender.send_info(&InfoParams {
            depth: 10,
            nodes: 50_000,
            ..InfoParams::for_player(player)
        });
    });
}
```

`InfoSender` is `Clone + Send + Sync`. Returns `None` during preprocessing.

## Hivemind

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

Same lifecycle as `Bot` (`preprocess`, `on_game_over`). Uses the raw `player1_*()` / `player2_*()` methods on `GameState`.

## Options

Declare tunable parameters with the `DeriveOptions` macro. The host can override them before the game starts.

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

    // No attribute: not an option, ignored by derive
    cache: Vec<u32>,
}
```

| Attribute | Field type | Description |
|---|---|---|
| `#[spin(default, min, max)]` | `i32` | Integer in a range |
| `#[check(default)]` | `bool` | Boolean toggle |
| `#[combo(default, choices)]` | `String` | String from a fixed set |
| `#[str_opt(default)]` | `String` | Free-form string |

Bots without options: `impl Options for MyBot {}`.
