# Botpack

A collection of bots to test against and learn from. Each one is a complete, working bot. Read the source to see how it's built, or just run it as an opponent.

## Running a match

Pick a bot and play yours against it:

```bash
cargo run -p pyrat-headless -- \
  "your_bot_command" \
  "cd botpack/greedy && cargo run --release"
```

Replace `your_bot_command` with however you launch your bot. Each bot's run command is in its `bot.toml`.

The GUI will discover botpack bots automatically from their `bot.toml` metadata once bot management lands.

## Bots

Listed from simplest to most complex.

| Bot | Strategy | SDK features | Tags | Language |
|-----|----------|--------------|------|----------|
| [Smart Random](smart-random/) | Random valid direction each turn | `effective_moves` | baseline | Rust |
| [Greedy](greedy/) | Nearest cheese, random tiebreaking | `nearest_cheeses`, `send_info` | greedy, shortest-path | Rust |
| [Greedy](greedy-py/) | Nearest cheese, random tiebreaking | `nearest_cheeses`, `send_info` | greedy, shortest-path | Python |
| [Search](search/) | Iterative-deepening best-response tree search | `GameSim`, `effective_moves`, `should_stop`, `send_info`, `DeriveOptions` | tree-search, iterative-deepening | Rust |

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

Got a bot? Add a directory with your source and a `bot.toml`, open a PR.
