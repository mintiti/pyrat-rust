# Botpack

Bots to play against, learn from, or just see what's possible. From a random walker to a tree-searching opponent that thinks ahead, each one is a working bot you can run right now.

## Running a match

Rust bots need the [Rust toolchain](https://rustup.rs/). Python bots need [uv](https://docs.astral.sh/uv/).

Pick a bot and play yours against it. From the repo root:

```bash
cargo run -p pyrat-headless -- \
  "your_bot_command" \
  "cd botpack/greedy && cargo run --release"
```

Replace `your_bot_command` with however you launch your bot. Each bot's `bot.toml` has its run command.

The GUI will discover botpack bots automatically from their `bot.toml` metadata once bot management lands.

## Bots

Listed from simplest to most complex.

| Bot | Strategy | SDK features | Tags | Language |
|-----|----------|--------------|------|----------|
| [Smart Random](smart-random/) | Random valid direction each turn | `effective_moves` | baseline | Rust |
| [Smart Random](smart-random-py/) | Random valid direction each turn | `effective_moves` | baseline | Python |
| [Greedy](greedy/) | Nearest cheese, random tiebreaking | `nearest_cheeses`, `send_info` | greedy, shortest-path | Rust |
| [Greedy](greedy-py/) | Nearest cheese, random tiebreaking | `nearest_cheeses`, `send_info` | greedy, shortest-path | Python |
| [Search](search/) | Iterative-deepening best-response tree search | `GameSim`, `effective_moves`, `should_stop`, `send_info`, `DeriveOptions` | tree-search, iterative-deepening | Rust |
| [Search](search-py/) | Iterative-deepening best-response tree search | `GameSim`, `effective_moves`, `should_stop`, `send_info`, `Spin` | tree-search, iterative-deepening | Python |

Looking for a specific SDK feature? The source code is the documentation: each bot's comments explain the strategy reasoning and SDK usage.

## bot.toml

Every bot has a `bot.toml` that describes how to run it:

```toml
[settings]
name = "Greedy"
agent_id = "pyrat/greedy"
run_command = "cargo run --release"

[details]
description = "Picks the nearest cheese, simple and effective"
developer = "mintiti"
language = "Rust"
tags = ["greedy", "shortest-path"]
```

`settings` tells the runner how to launch the bot. `details` is metadata for discovery and the bot listing above.

## Contributing a bot

Got a bot you want people to play against? Add a directory with your source and a `bot.toml`, open a PR.
