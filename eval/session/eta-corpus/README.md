# Tournament ETA evidence corpus

This starter corpus keeps raw, replayable timing evidence for the M0/M1/M1P comparison. Its manifest
defines report order and provenance. Inclusion is evidence, not model selection. M1P changes only
active service: it conditions remaining time on completed observations of the same phase and turn.

Regenerate the nine deterministic synthetic traces from the repository root:

```bash
cargo run -p pyrat-eval --bin eta-evidence -- \
  synthetic --output-dir eval/session/eta-corpus
```

The six live traces are frozen observations of the four-bot workload in
`mixed-botpack.toml`. Capture each policy at concurrency 1, 2, and 4 with a
disposable store, preserving the alternating run order recorded in
`joint-manifest.json`. For example:

```bash
pyrat-eval tournament run \
  --config eval/session/eta-corpus/mixed-botpack.toml \
  --max-parallel 2 \
  --dispatch-policy bot-balanced \
  --store-path /tmp/pyrat-eta-live-balanced-c2.db \
  --timing-trace eval/session/eta-corpus/live-mixed-bot-balanced-c2.json
```

The seed and flags reproduce the schedule and game inputs, not subprocess wall time or JSON bytes.
Use a disposable `--store-path` when recapturing it.

## Production-shaped replicated block

`gui-default-six-bot.toml` mirrors the GUI's default workload: all six botpack
bots, eight paired mazes per matchup (240 games), 150 turns, 200 ms moves,
2,000 ms preprocessing, and four workers. The first replicated block uses
three seeds and alternates policy order:

| Seed | Run 1 | Run 2 |
|---:|---|---|
| 20260813 | bot-balanced | pair-major |
| 20260814 | pair-major | bot-balanced |
| 20260815 | bot-balanced | pair-major |

Capture runs sequentially so one block does not contend with another. Use a
fresh disposable store for every run and a `.json.gz` timing path; gzip is
lossless and prevents full turn-level traces from exhausting local disk. A
single capture has this shape:

```bash
pyrat-eval tournament run \
  --config eval/session/eta-corpus/gui-default-six-bot.toml \
  --seed 20260813 \
  --dispatch-policy bot-balanced \
  --store-path /tmp/pyrat-eta-gui-seed-20260813-balanced.db \
  --timing-trace eval/session/eta-corpus/gui-seed-20260813-bot-balanced-c4.json.gz
```

The whole tournament is the replication unit. Its 240 games are correlated
measurements inside one seed/policy/machine block, not 240 independent policy
trials.

The frozen block in this corpus completed all 1,440 scheduled games without a
failure. Across the three matched seeds, bot-balanced was faster in one block
and had a mean duration delta of +1.9 seconds relative to pair-major (range
-24.8 to +25.3 seconds). It greatly reduced the pair-major ordering bias seen
by the global ETA candidates, but increased measured same-bot overlap at four
workers. Winner and rating comparisons are included through the companion
`*-results.json` files; interpret them as stability warnings rather than a
causal dispatch effect because several botpack bots use unseeded randomness.
This block therefore widens the evidence without selecting a production
dispatcher or ETA.

## Six-bot 16-maze diagnostic

Seed `20260816` doubles the schedule to 16 paired mazes per matchup: 32 games
per pair and 480 games per policy. Pair-major ran first and bot-balanced
second, with no build or analysis work between the clean captures. Both runs
completed 480/480 games without failure. This is one larger diagnostic block,
not a fourth independent replication of the eight-maze experiment.

The larger trace isolates why finer point models can lose to M0:

- mean attempt time spans roughly 0.2 to 22 seconds across the 15 matchup
  classes (about 100×);
- pair-major's first four completions average 0.6 seconds versus 9.8 seconds
  over the full run, while balanced starts at 3.1 seconds versus 10.1 seconds;
- median same-pair late-minus-early duration is within 0.1 seconds and launch
  drift rounds to zero, rejecting a material warmup/rebuild explanation;
- on the common ready window, pair-major MAE is M0/M1/M1P =
  305.1/314.9/312.4 seconds, while balanced is 63.1/56.1/58.6 seconds.

M1's scheduler structure therefore helps once matchup coverage is
representative. M1P's exact phase/turn residual does not consistently add
value because it changes only active work, pools completed profiles globally,
and leaves every pending match on the same completed-only mean. The next M2
must separate setup/play, partially pool by bot/pair/load, and guard against
right-censoring of slow active attempts; raw repetition count alone is not the
missing ingredient.

Generate the local project-memory report with:

```bash
cargo run -p pyrat-eval --bin eta-evidence -- report \
  --manifest eval/session/eta-corpus/joint-manifest.json \
  --output .mt/reference/reports/gui-tournament-eta-dispatch.html
```
