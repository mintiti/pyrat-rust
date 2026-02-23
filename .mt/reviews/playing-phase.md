# Review: `feature/playing-phase`

**Date:** 2026-02-23
**Scope:** `host/src/game_loop/playing.rs` (308 lines), rename Rat/Python → Player1/Player2 across wire protocol + tests, new `host/tests/playing_integration.rs` (739 lines, 9 tests). Single commit.
**What:** The playing phase turn loop — send TurnState, collect actions with timeout/disconnect handling, step the engine, check game over, repeat.

## Fix before merging

### 1. ~~`stale_turn_ignored` test doesn't test stale turns~~ — Fixed

Deleted the redundant integration test. Added `stale_action_is_ignored` unit test in `playing.rs` that calls `collect_actions` directly with a wrong turn number, verifying the action is dropped and the session receives a `Timeout` command.

### 2. ~~`cheese_updates_in_state` doesn't verify cheese updates~~ — Fixed

Rewrote with 3 cheese (none under P2's start), so the game survives turn 0. Now reads a second TurnState on turn 1 and asserts cheese count dropped from 3 → 2 and `(1,0)` is gone.

## Should do

### 3. Failed TurnState send doesn't mark session as disconnected

`playing.rs:86-90` — `let _ =` on the channel send means a dead session isn't added to `disconnected`. Every subsequent turn clones and sends into a dead channel, and `collect_actions` waits the full `move_timeout` before the `Disconnected` message arrives. Not a correctness bug (timeout catches it), but adds one full timeout penalty per turn per crashed bot. Consider marking the session as disconnected when send fails.

### 4. No tracing in the timeout path

`playing.rs:240-258` — when a bot times out, no log is emitted. Setup logs timeouts. Session logs timeouts. Playing silently absorbs them. A `debug!` or `warn!` when defaulting to STAY would match crate conventions. Same for adding a `tracing::debug_span!("turn", turn = current_turn)` at the top of the loop.

### 5. `PlayingConfig` missing `#[derive(Debug, Clone)]`

Every other config/data struct in the crate derives both. Also defined inline in `playing.rs` rather than in `config.rs` where `MatchSetup` and `SetupTiming` live.

### 6. Tests never verify TurnState content beyond turn number and cheese

`read_turn_state` only extracts `(turn, cheese)`. Player positions, scores, mud turns, and `last_move` are serialized by `build_turn_state` but never checked. At minimum, `happy_path_both_respond` should verify P1's position advances across turns.

### 7. `MatchResult.result` uses wire-layer `GameResult`

Couples the game loop's public API to generated FlatBuffers types. A proper Rust enum (`enum Outcome { Player1Win, Player2Win, Draw }`) at the game_loop boundary would decouple the public surface from the wire format.

### 8. `SessionHandle` defined in `setup.rs` but shared across phases

Creates a dependency from playing → setup. This type belongs in `config.rs` or a shared module.

### 9. Missing test coverage

- Duplicate actions from one session for the same player in one turn (first-wins semantics)
- Hivemind partial response (one of two actions sent, other times out)
- Hivemind disconnect (one `Disconnected` must fill both players)
- Player2 winning (only Player1 wins in current tests; `determine_result`'s Player2 branch unexercised)
- Score majority termination (game stops before max turns and before all cheese gone)
- `AllDisconnected` error path (channel closes during `collect_actions`)

## What's solid

- Core loop structure: read-evaluate-step-check, each concern in a distinct block, easy to follow
- Graceful degradation: disconnect → STAY, timeout → STAY + notify, stale → drop — right behavior for a game host
- First-wins action semantics prevent bots from overwriting their own move
- `PlayingError` is minimal: one variant for the structural failure, everything else degrades into game result
- `select!` loop, channel patterns, section comments, naming all match conventions from setup and session
- Test infrastructure: helpers compose cleanly, message-type assertions prevent wrong-frame consumption, timeouts prevent hangs
- Rename is thorough and mechanical — no missed references
