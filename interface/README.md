# pyrat-engine-interface

High-level interface for the PyRat game engine. Sits between the raw engine (`pyrat-rust`) and SDK consumers (Rust bots, Python bots via FFI).

## Who is this for?

Bot SDK authors. If you're building a bot, you interact with `GameView` — not the engine directly. If you're building an SDK that wraps this crate for another language, the types here are your public surface.

## Key types

| Type | What it does |
|------|-------------|
| `GameView` | SDK-facing wrapper — game state, graph queries, pathfinding |
| `Maze` | Borrow-bundle for static maze topology (walls, mud, dimensions) |
| `PlayerSnapshot` | Copy of a player's position, score, mud state |
| `PathResult` | Shortest-path result with cost and first-move options |

## Quick start

```rust
use pyrat_engine_interface::{Coordinates, Direction, GameView};

let view = GameView::from_config(
    5, 5, 300,
    &[],  // walls
    &[],  // mud
    vec![Coordinates::new(2, 2)],
    Coordinates::new(0, 0),
    Coordinates::new(4, 4),
).unwrap();

// Greedy bot: move toward nearest cheese
let nearest = view.nearest_cheeses(view.player1().position);
let dir = nearest.first()
    .and_then(|r| r.first_moves.first().copied())
    .unwrap_or(Direction::Stay);
```

## Common patterns

### Greedy cheese collection

```rust
let me = view.player1();
let nearest = view.nearest_cheeses(me.position);
let dir = nearest.first()
    .and_then(|r| r.first_moves.first().copied())
    .unwrap_or(Direction::Stay);
```

### Distance comparison

```rust
let my_dists = view.distances_from(view.player1().position);
let opp_dists = view.distances_from(view.player2().position);

// Find cheese I can reach before the opponent
for &cheese_pos in &view.cheese() {
    let my_cost = my_dists.get(&cheese_pos);
    let opp_cost = opp_dists.get(&cheese_pos);
    if my_cost < opp_cost {
        // I'm closer
    }
}
```

### Simulation with snapshot

`snapshot()` returns a raw engine `GameState` — intentionally not a `GameView`. This gives direct access to `make_move`/`unmake_move` without wrapper overhead.

```rust
use pyrat_engine_interface::Maze;

let mut sim = view.snapshot();
let undo = sim.make_move(Direction::Right, Direction::Stay);

// Build a Maze from the snapshot for pathfinding
let maze = Maze::new(&sim.move_table, &sim.mud, sim.width, sim.height);

// Undo is LIFO — unmake in reverse order
sim.unmake_move(undo);
```

Note: `PlayerSnapshot` uses `position` and `mud_turns`, while `GameState.playerN` uses `current_pos` and `mud_timer`.

## Build and test

```bash
# From repository root
cargo test -p pyrat-engine-interface
cargo doc -p pyrat-engine-interface --open
cargo clippy -p pyrat-engine-interface -- -D warnings
```

## Part of the PyRat monorepo

See the root README for full build commands and project overview.
