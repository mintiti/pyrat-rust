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

The [GUI](../gui/) discovers botpack bots automatically. Point it at `botpack/` as a scan path and they all show up.

## Bots

Listed from simplest to most complex.

| Bot | Strategy | SDK features | Tags | Language |
|-----|----------|--------------|------|----------|
| [Smart Random.rs](smart-random/) | Random valid direction each turn | [`effective_moves`](../sdk/#raw-state-whats-happening-right-now) | baseline | Rust |
| [Smart Random.py](smart-random-py/) | Random valid direction each turn | [`effective_moves`](../sdk/#raw-state-whats-happening-right-now) | baseline | Python |
| [Greedy.rs](greedy/) | Nearest cheese, random tiebreaking | [`nearest_cheeses`](../sdk/#pathfinding-where-should-i-go), [`send_info`](../sdk/#thinking-out-loud) | greedy, shortest-path | Rust |
| [Greedy.py](greedy-py/) | Nearest cheese, random tiebreaking | [`nearest_cheeses`](../sdk/#pathfinding-where-should-i-go), [`send_info`](../sdk/#thinking-out-loud) | greedy, shortest-path | Python |
| [Search.rs](search/) | Naive tree search with iterative deepening, both players think ahead to grab the most cheese | [`GameSim`](../sdk/#simulation-what-happens-if-i-go-there), [`effective_moves`](../sdk/#raw-state-whats-happening-right-now), [`should_stop`](../sdk/#lifecycle), [`send_info`](../sdk/#thinking-out-loud) | tree-search, iterative-deepening | Rust |
| [Search.py](search-py/) | Naive tree search with iterative deepening, both players think ahead to grab the most cheese | [`GameSim`](../sdk/#simulation-what-happens-if-i-go-there), [`effective_moves`](../sdk/#raw-state-whats-happening-right-now), [`should_stop`](../sdk/#lifecycle), [`send_info`](../sdk/#thinking-out-loud) | tree-search, iterative-deepening | Python |

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

`settings` tells the runner how to launch the bot. `details` is metadata for discovery and the bot listing above. The entire `[details]` section is optional: all fields default to empty.

`agent_id` must be globally unique. Discovery deduplicates on it. The convention for botpack bots is `pyrat/<name>` (e.g. `pyrat/greedy`).

`run_command` is passed to `sh -c`, so it runs as a shell command. Only run bots you trust.

The GUI scans directories up to 3 levels deep for these files. Add a scan path that contains your bots and they appear automatically.

## Contributing a bot

Got a bot you want people to play against? Add a directory with your source and a `bot.toml`, open a PR.
