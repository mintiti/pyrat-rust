# pyrat-engine-interface

High-level interface for the PyRat game engine. Sits between the raw engine
(`pyrat-rust`) and SDK consumers.

## Modules

| Module | What it provides |
|--------|-----------------|
| `maze` | `Maze` borrow-bundle for static topology, graph queries |
| `pathfinding` | Dijkstra shortest paths, nearest cheese, distance maps |
| `view` | `GameView` — SDK-facing wrapper with game state + queries |

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

## Part of the PyRat monorepo

See the root `CLAUDE.md` for build commands. For full API docs: `cargo doc -p pyrat-engine-interface --open`.
