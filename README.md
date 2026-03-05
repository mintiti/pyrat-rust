<p align="center">
  <img src="docs/images/logo.png" alt="PyRat" width="150">
</p>

<h1 align="center">PyRat</h1>

A competitive, turn-based two-player maze game where a Rat and a Python race to collect cheese. Built from IMT Atlantique's original versions<!-- TODO: link to original -->.

<p align="center">
  <img src="docs/images/match.png" alt="A PyRat match in progress" width="600">
</p>

## What's in this repo?

This isn't just the game — it's the whole ecosystem: game engine, bot SDKs, match runner, and a GUI to watch it all happen. Pick what you need.

**[Write a bot](sdk/)** — Build an AI that plays PyRat. Python or Rust, your choice.

```python
from pyrat_sdk import Bot, Context, Direction, GameState

class MyBot(Bot):
    def think(self, state: GameState, ctx: Context) -> Direction:
        result = state.nearest_cheese()
        return result.directions[0] if result else Direction.STAY
```

**[Train an AI](engine/)** — Use the engine for reinforcement learning (PettingZoo) or game tree search.

```python
from pyrat_engine import GameConfig
from pyrat_engine.env import PyRatEnv

env = PyRatEnv(GameConfig.classic(15, 15, 21))
obs, info = env.reset(seed=42)
obs, rewards, terms, truncs, infos = env.step(actions)
```

**[Use the engine as a library](engine/)** — Embed the game in your own tools. Rust crate or Python package.

```rust
use pyrat_engine::{GameConfig, Direction};

let config = GameConfig::preset("large")?;
let mut game = config.create(Some(42));
let result = game.process_turn(Direction::Right, Direction::Left);
```

🚧 **Watch bots play** — Desktop GUI for running and watching matches. Coming soon.

## Setup

Prerequisites:
- [Rust toolchain](https://rustup.rs/)
- Python 3.10+
- [uv](https://docs.astral.sh/uv/)

```bash
git clone https://github.com/mintiti/pyrat-rust.git
cd pyrat-rust
uv sync --all-extras
```

## See it run

```bash
cargo run -p pyrat-headless -- \
  "cargo run -p pyrat-sdk --example greedy" \
  "cargo run -p pyrat-sdk --example smart_random"
```

## The game

A Rat and a Python drop into opposite corners of a maze. Cheese is scattered across the board, and both players move at the same time — so you're not reacting to your opponent, you're trying to outsmart them.

Some passages are filled with mud, which costs extra turns to cross. Do you take the shortcut through the mud, or go the long way around? The maze is symmetric so neither player has a positional advantage, but the strategies can look completely different.

First to grab more than half the cheese wins.

Full rules in the [engine README](engine/).

## Repository map

| Path | What it is |
|------|------------|
| [`engine/`](engine/) | Game engine — Rust core, Python bindings, PettingZoo env |
| [`sdk/`](sdk/) | Bot SDKs — currently [Python](sdk/python/) and [Rust](sdk/rust/), more languages to come |
| [`server/`](server/) | Match infrastructure — hosting, headless runner, wire protocol |
| `gui/` | 🚧 Desktop GUI — watch and manage matches (coming soon) |

Run `make help` for the full command list.
