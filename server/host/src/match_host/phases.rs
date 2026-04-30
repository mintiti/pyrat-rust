//! `Match<S>` phase implementations: setup, start, step, run.

use std::time::Duration;

use pyrat::{Direction, GameState};
use pyrat_protocol::{BotMsg, HashedTurnState, HostMsg, MatchConfig, SearchLimits, TurnState};
use pyrat_wire::Player as PlayerSlot;
use tokio::time::Instant;
use tracing::{debug, warn};

use crate::player::{EventSink, Player, PlayerError};

use super::config::{PlayingConfig, SetupTiming};
use super::error::MatchError;
use super::events::{emit, MatchEvent};
use super::policy::ActionOutcome;
use super::result::MatchResult;

// ── Phase markers ─────────────────────────────────────

/// Pre-setup phase. Bots have completed handshake (Identify → Welcome) but
/// haven't been configured yet.
#[derive(Debug)]
pub struct Created;

/// Configure → Ready → GoPreprocess → PreprocessingDone all done. Both bots
/// confirmed `bot_synced_hash` matches the host's engine hash.
#[derive(Debug)]
pub struct Ready {
    bot_synced_hash: u64,
}

/// Turn loop is running. `pending_advance` is `None` on the first turn (bots
/// are synced from `Ready`); `Some(...)` on every turn after, carrying the
/// previous turn's actions to be acknowledged at the top of the next `step`.
#[derive(Debug)]
pub struct Playing {
    /// Turn currently being played.
    turn: u16,
    /// Hash both bots are confirmed-synced to right now (the state being
    /// thought about this turn).
    bot_synced_hash: u64,
    /// Carries the previous turn's resolution forward. `None` only before
    /// the first turn.
    pending_advance: Option<PendingAdvance>,
}

/// Analysis-mode sub-state: bots are thinking, no `Action` collected yet.
/// Reached via `Match<Playing>::start_turn` (live state, Advance + Go) or
/// `Match<Playing>::start_turn_with` (injected state, GoState — no Advance).
/// The caller drives [`Match::stop_and_collect`] when ready.
#[derive(Debug)]
pub struct Thinking {
    /// Turn the bots are thinking for.
    turn: u16,
    /// Hash both bots are confirmed-synced to (the state they were sent in
    /// `Go` or `GoState`).
    bot_synced_hash: u64,
}

/// Analysis-mode sub-state: actions collected, ready to apply or override.
/// Holds raw [`ActionOutcome`]s; the [`FaultPolicy`](super::FaultPolicy)
/// runs in [`Match::advance`] (default), or is skipped entirely by
/// [`Match::advance_with`] (caller supplies override directions).
#[derive(Debug)]
pub struct Collected {
    /// Turn just finished thinking.
    turn: u16,
    /// Hash bots thought about. Used for `take_provisional` validation in
    /// [`Match::advance`].
    bot_synced_hash: u64,
    /// Per-slot collection outcomes. Read via [`Match::outcomes`].
    outcomes: [ActionOutcome; 2],
}

/// Final phase. `Match::run` returns `MatchResult` directly; consumers that
/// drive `step` manually consume `Match<Finished>` for the result.
#[derive(Debug)]
pub struct Finished {
    result: MatchResult,
}

#[derive(Debug, Clone, Copy)]
struct PendingAdvance {
    p1_action: Direction,
    p2_action: Direction,
    /// New turn number (= previous turn + 1).
    turn: u16,
    /// Engine Zobrist hash of the post-advance state (already computed when
    /// the previous step applied the actions).
    new_hash: u64,
}

// ── Match<S> ──────────────────────────────────────────

/// Owner of a match's lifecycle. Holds the carry-along context (engine
/// state, players, configs, event sink) and the current phase marker.
/// Phase transitions consume `Match<S>` and produce `Match<T>` carrying
/// the same `ctx` — a 2-field copy instead of rebuilding every field.
pub struct Match<S> {
    ctx: MatchCtx,
    state: S,
}

/// Per-match carry-along context. Threaded unchanged through every
/// typestate transition; only `state: S` varies per phase.
pub(super) struct MatchCtx {
    game: GameState,
    players: [Box<dyn Player>; 2],
    match_config: MatchConfig,
    /// Per-player option overrides keyed in slot order ([0] = Player1).
    options: [Vec<(String, String)>; 2],
    setup_timing: SetupTiming,
    playing_config: PlayingConfig,
    event_sink: EventSink,
    /// Internal sender backing `event_sink`, so methods can call the
    /// `emit(Option<&Sender>, …)` helper directly without going through
    /// the public `EventSink::emit` (which exposes only the sink shape).
    event_tx: Option<tokio::sync::mpsc::UnboundedSender<MatchEvent>>,
}

/// Outcome of `Match<Playing>::step`.
pub enum StepResult {
    /// More turns to play.
    Continue(Match<Playing>),
    /// Match ended this turn.
    GameOver(Match<Finished>),
}

// ── Constructors and shared accessors ─────────────────

impl Match<Created> {
    /// Construct a match. `players` is slot-indexed: position 0 controls
    /// Player1, position 1 controls Player2. `options` mirrors the same
    /// indexing — empty vectors are fine for bots without configurable
    /// options.
    pub fn new(
        game: GameState,
        players: [Box<dyn Player>; 2],
        match_config: MatchConfig,
        options: [Vec<(String, String)>; 2],
        setup_timing: SetupTiming,
        playing_config: PlayingConfig,
        event_tx: Option<tokio::sync::mpsc::UnboundedSender<MatchEvent>>,
    ) -> Self {
        let event_sink = match event_tx.clone() {
            Some(tx) => EventSink::new(tx),
            None => EventSink::noop(),
        };
        Self {
            ctx: MatchCtx {
                game,
                players,
                match_config,
                options,
                setup_timing,
                playing_config,
                event_sink,
                event_tx,
            },
            state: Created,
        }
    }

    /// The event sink Players forward sideband through. Hand to
    /// `EmbeddedPlayer::accept` / `accept_players` so observer-facing
    /// messages reach the same channel as the events Match emits internally.
    pub fn event_sink(&self) -> &EventSink {
        &self.ctx.event_sink
    }
}

impl<S> Match<S> {
    pub fn game(&self) -> &GameState {
        &self.ctx.game
    }

    pub fn match_config(&self) -> &MatchConfig {
        &self.ctx.match_config
    }
}

impl Match<Finished> {
    pub fn result(&self) -> &MatchResult {
        &self.state.result
    }

    /// Close both players (best-effort) and unwrap the result. Mirrors the
    /// cleanup [`Match::run`] does internally; analysis-mode callers that
    /// drive the typestate by hand should call this when they reach
    /// [`Finished`] so dispatcher tasks shut down cleanly.
    pub async fn finalize(self) -> MatchResult {
        let Self { ctx, state } = self;
        let [p1, p2] = ctx.players;
        let _ = p1.close().await;
        let _ = p2.close().await;
        state.result
    }
}

// ── Setup: Configure → Ready → GoPreprocess ───────────

impl Match<Created> {
    /// Drive both bots from post-Welcome through Configure, Ready (with
    /// hash verification), GoPreprocess, and PreprocessingDone. Returns
    /// `Match<Ready>` carrying the engine hash both bots agreed to.
    pub async fn setup(mut self) -> Result<Match<Ready>, MatchError> {
        let expected_hash = self.ctx.game.state_hash();
        let configure_timeout = self.ctx.setup_timing.configure_timeout;
        let preprocessing_timeout = self.ctx.setup_timing.preprocessing_timeout;

        // Emit BotIdentified for each slot. Identities are post-Welcome —
        // every player handed to Match::new has already completed the
        // Identify→Welcome handshake. This is the single canonical emission
        // point; consumers (headless, GUI, bot-check) listen on the event
        // stream rather than reaching into PlayerIdentity themselves.
        for slot_idx in 0..2 {
            let id = self.ctx.players[slot_idx].identity();
            emit(
                self.ctx.event_tx.as_ref(),
                MatchEvent::BotIdentified {
                    player: id.slot,
                    name: id.name.clone(),
                    author: id.author.clone(),
                    agent_id: id.agent_id.clone(),
                },
            );
        }

        // Send Configure to both bots in slot order. The same MatchConfig
        // body goes to both; per-slot identity comes from `Welcome.player_slot`.
        for slot_idx in 0..2 {
            let opts = std::mem::take(&mut self.ctx.options[slot_idx]);
            let msg = HostMsg::Configure {
                options: opts,
                match_config: Box::new(self.ctx.match_config.clone()),
            };
            let slot = slot_for(slot_idx);
            self.ctx.players[slot_idx]
                .send(msg)
                .await
                .map_err(|e| MatchError::from_player(slot, e))?;
            tracing::info!(?slot, "match: configure sent");
        }

        // Recv Ready from each, verify hash.
        for slot_idx in 0..2 {
            let slot = slot_for(slot_idx);
            let msg = recv_with_timeout(
                self.ctx.players[slot_idx].as_mut(),
                slot,
                configure_timeout,
                MatchError::SetupTimeout(slot),
            )
            .await?;
            match msg {
                BotMsg::Ready { state_hash } => {
                    if state_hash != expected_hash {
                        return Err(MatchError::ReadyHashMismatch {
                            slot,
                            expected: expected_hash,
                            got: state_hash,
                        });
                    }
                },
                other => {
                    return Err(MatchError::UnexpectedMessage {
                        slot,
                        detail: format!("expected Ready, got {other:?}"),
                    })
                },
            }
            tracing::info!(?slot, "match: ready received");
        }

        // Send GoPreprocess to both, recv PreprocessingDone.
        tracing::info!("match: preprocessing started");
        emit(self.ctx.event_tx.as_ref(), MatchEvent::PreprocessingStarted);
        for slot_idx in 0..2 {
            let slot = slot_for(slot_idx);
            self.ctx.players[slot_idx]
                .send(HostMsg::GoPreprocess {
                    state_hash: expected_hash,
                })
                .await
                .map_err(|e| MatchError::from_player(slot, e))?;
        }
        for slot_idx in 0..2 {
            let slot = slot_for(slot_idx);
            let msg = recv_with_timeout(
                self.ctx.players[slot_idx].as_mut(),
                slot,
                preprocessing_timeout,
                MatchError::PreprocessingTimeout(slot),
            )
            .await?;
            match msg {
                BotMsg::PreprocessingDone => {},
                other => {
                    return Err(MatchError::UnexpectedMessage {
                        slot,
                        detail: format!("expected PreprocessingDone, got {other:?}"),
                    })
                },
            }
            tracing::info!(?slot, "match: preprocessing done");
        }

        tracing::info!("match: setup complete");
        emit(self.ctx.event_tx.as_ref(), MatchEvent::SetupComplete);

        Ok(Match {
            ctx: self.ctx,
            state: Ready {
                bot_synced_hash: expected_hash,
            },
        })
    }

    /// Convenience: setup → start → step until game over → return result.
    /// Closes both players (best effort) on the success path; protocol
    /// errors propagate without close (the failing match's resources drop
    /// when the caller drops the error).
    pub async fn run(self) -> Result<MatchResult, MatchError> {
        let ready = self.setup().await?;
        let mut playing = ready.start();
        loop {
            match playing.step().await? {
                StepResult::Continue(next) => playing = next,
                StepResult::GameOver(finished) => return Ok(finished.finalize().await),
            }
        }
    }
}

// ── Ready → Playing ───────────────────────────────────

impl Match<Ready> {
    /// Transition into the playing phase. No IO — just rewires the typestate.
    pub fn start(self) -> Match<Playing> {
        let bot_synced_hash = self.state.bot_synced_hash;
        emit(
            self.ctx.event_tx.as_ref(),
            MatchEvent::MatchStarted {
                config: self.ctx.match_config.clone(),
            },
        );
        Match {
            ctx: self.ctx,
            state: Playing {
                turn: 0,
                bot_synced_hash,
                pending_advance: None,
            },
        }
    }
}

// ── Playing: step + start_turn + start_turn_with ──────

impl Match<Playing> {
    /// Advance the match by one full turn (live mode):
    /// 1. Optional Advance + SyncOk (skipped on the first turn).
    /// 2. Send Go.
    /// 3. Wait for actions with deadline + Stop fallback + grace.
    /// 4. Resolve outcomes via [`FaultPolicy`](super::FaultPolicy).
    /// 5. Apply to engine, emit `TurnPlayed`.
    /// 6. Either GameOver → [`Finished`] or queue `pending_advance` and
    ///    return [`Playing`].
    ///
    /// Convenience composition; analysis-mode callers use
    /// [`Self::start_turn`] + [`Match::stop_and_collect`] + [`Match::advance`]
    /// to drive the steps individually.
    pub async fn step(mut self) -> Result<StepResult, MatchError> {
        let go_hash = self.dispatch_go_live().await?;
        let turn = self.state.turn;
        let outcomes = self.collect_outcomes(go_hash, turn).await?;
        let resolved = self.resolve_outcomes(outcomes, go_hash, turn)?;
        self.apply_resolved(turn, go_hash, resolved).await
    }

    /// Analysis-mode: send Advance (if pending) + Go, transition to
    /// [`Thinking`]. The caller drives [`Match::stop_and_collect`] when
    /// ready to read outcomes.
    pub async fn start_turn(mut self) -> Result<Match<Thinking>, MatchError> {
        let go_hash = self.dispatch_go_live().await?;
        let turn = self.state.turn;
        Ok(Match {
            ctx: self.ctx,
            state: Thinking {
                turn,
                bot_synced_hash: go_hash,
            },
        })
    }

    /// Analysis-mode: inject an arbitrary [`TurnState`]. Drops any pending
    /// Advance (the injection IS the new sync), rebuilds `self.ctx.game` from
    /// the snapshot via Foundation F4, and sends `GoState` to both bots.
    /// Transitions to [`Thinking`] like [`Self::start_turn`].
    pub async fn start_turn_with(mut self, ts: TurnState) -> Result<Match<Thinking>, MatchError> {
        // Discontinuity: previous pending_advance is no longer relevant.
        self.state.pending_advance = None;

        // F4: rebuild engine from snapshot, recomputing the Zobrist.
        let new_game = crate::snapshot::rebuild_engine_state(&self.ctx.match_config, &ts)
            .map_err(MatchError::Internal)?;
        self.ctx.game = new_game;
        let go_hash = self.ctx.game.state_hash();
        let new_turn = self.ctx.game.turn;

        let limits = build_search_limits(&self.ctx.playing_config);
        for slot_idx in 0..2 {
            let slot = slot_for(slot_idx);
            self.ctx.players[slot_idx]
                .send(HostMsg::GoState {
                    turn_state: Box::new(ts.clone()),
                    state_hash: go_hash,
                    limits: limits.clone(),
                })
                .await
                .map_err(|e| MatchError::from_player(slot, e))?;
        }

        Ok(Match {
            ctx: self.ctx,
            state: Thinking {
                turn: new_turn,
                bot_synced_hash: go_hash,
            },
        })
    }

    /// Acknowledge any pending advance (Advance + SyncOk round-trip), then
    /// send `Go` to both bots. Mutates `self.state` so the post-call
    /// `bot_synced_hash` and `turn` reflect what bots are now thinking
    /// about. Shared by [`Self::step`] and [`Self::start_turn`].
    async fn dispatch_go_live(&mut self) -> Result<u64, MatchError> {
        if let Some(pa) = self.state.pending_advance.take() {
            self.run_advance(pa).await?;
            self.state.bot_synced_hash = pa.new_hash;
            self.state.turn = pa.turn;
        }

        let go_hash = self.state.bot_synced_hash;
        let limits = build_search_limits(&self.ctx.playing_config);
        for slot_idx in 0..2 {
            let slot = slot_for(slot_idx);
            self.ctx.players[slot_idx]
                .send(HostMsg::Go {
                    state_hash: go_hash,
                    limits: limits.clone(),
                })
                .await
                .map_err(|e| MatchError::from_player(slot, e))?;
        }
        Ok(go_hash)
    }

    /// Send `Advance` to both players, await SyncOk from each, handling
    /// `Resync → FullState → SyncOk` with a bounded retry (1 per player per
    /// turn). Verifies each `SyncOk.hash` against the host's `new_hash`.
    async fn run_advance(&mut self, pa: PendingAdvance) -> Result<(), MatchError> {
        for slot_idx in 0..2 {
            let slot = slot_for(slot_idx);
            self.ctx.players[slot_idx]
                .send(HostMsg::Advance {
                    p1_dir: pa.p1_action,
                    p2_dir: pa.p2_action,
                    turn: pa.turn,
                    new_hash: pa.new_hash,
                })
                .await
                .map_err(|e| MatchError::from_player(slot, e))?;
        }

        for slot_idx in 0..2 {
            let slot = slot_for(slot_idx);
            self.await_sync_ok_with_retry(slot_idx, slot, pa).await?;
        }
        Ok(())
    }

    async fn await_sync_ok_with_retry(
        &mut self,
        slot_idx: usize,
        slot: PlayerSlot,
        pa: PendingAdvance,
    ) -> Result<(), MatchError> {
        let mut resync_used = false;
        loop {
            let msg = recv_required(self.ctx.players[slot_idx].as_mut(), slot).await?;
            match msg {
                BotMsg::SyncOk { hash } => {
                    if hash != pa.new_hash {
                        return Err(MatchError::ActionHashMismatch {
                            slot,
                            expected: pa.new_hash,
                            got: hash,
                        });
                    }
                    return Ok(());
                },
                BotMsg::Resync { my_hash } => {
                    if resync_used {
                        return Err(MatchError::PersistentDesync(slot));
                    }
                    resync_used = true;
                    debug!(
                        ?slot,
                        my_hash,
                        expected = pa.new_hash,
                        "bot requested Resync"
                    );
                    let turn_state =
                        build_turn_state_owned(&self.ctx.game, pa.p1_action, pa.p2_action);
                    self.ctx.players[slot_idx]
                        .send(HostMsg::FullState {
                            match_config: Box::new(self.ctx.match_config.clone()),
                            turn_state: Box::new(turn_state),
                        })
                        .await
                        .map_err(|e| MatchError::from_player(slot, e))?;
                    // Loop: next message must be SyncOk.
                },
                other => {
                    return Err(MatchError::UnexpectedMessage {
                        slot,
                        detail: format!("expected SyncOk/Resync after Advance, got {other:?}"),
                    })
                },
            }
        }
    }
}

// ── Generic helpers shared across phases ──────────────

impl<S> Match<S> {
    /// Wait for one [`ActionOutcome`](super::ActionOutcome) per slot. A bot
    /// produces `Committed` by sending a hash-and-turn-validated `Action`,
    /// `Disconnected` by closing the channel, or `TimedOut` by failing to do
    /// either before the deadline + grace window expires.
    ///
    /// On per-turn deadline expiry, `Stop` is sent to any still-thinking bots
    /// and the grace window starts; in-flight Actions arriving within the
    /// grace are still accepted as `Committed`. Any slot not filled by the
    /// end of grace becomes `TimedOut`. `BotTimeout` is emitted for each
    /// such slot — observability of the protocol fact, not a policy
    /// decision (see [`FaultPolicy`](super::FaultPolicy)).
    ///
    /// Used by live `step()`. For analysis-mode collection (Stop sent
    /// immediately, no internal deadline) see [`Self::collect_outcomes_after_stop`].
    async fn collect_outcomes(
        &mut self,
        expected_hash: u64,
        turn: u16,
    ) -> Result<[ActionOutcome; 2], MatchError> {
        let mut outcomes: [Option<ActionOutcome>; 2] = [None, None];

        let move_timeout = self.ctx.playing_config.move_timeout;
        let infinite = move_timeout.is_zero();
        let deadline = if infinite {
            None
        } else {
            Some(Instant::now() + move_timeout)
        };
        let grace = self.ctx.playing_config.network_grace;

        let mut stop_sent = false;
        let mut effective_deadline = deadline;
        loop {
            if outcomes[0].is_some() && outcomes[1].is_some() {
                break;
            }

            let outcome = poll_either(
                &mut self.ctx.players,
                &mut outcomes,
                expected_hash,
                turn,
                effective_deadline,
            )
            .await?;

            match outcome {
                PollOutcome::Progress => continue,
                PollOutcome::Timeout if !stop_sent => {
                    debug!(
                        turn,
                        "move timeout — sending Stop and entering grace window"
                    );
                    self.send_stop_to_unfilled(&outcomes).await;
                    stop_sent = true;
                    effective_deadline = Some(Instant::now() + grace);
                },
                PollOutcome::Timeout => break, // grace expired
            }
        }

        Ok(self.finalize_outcomes(outcomes, turn))
    }

    /// Analysis-mode collection: send `Stop` to both bots immediately, then
    /// wait `network_grace` for Actions. Pre-committed Actions sitting in
    /// the recv queue are picked up by the wait loop's first poll.
    /// `EmbeddedPlayer` silently ignores `Stop` outside of thinking, so
    /// spurious Stops to bots that already committed are harmless.
    ///
    /// Slots not filled by grace expiry become `TimedOut`; `BotTimeout`
    /// fires for each. Used by [`Match::stop_and_collect`].
    async fn collect_outcomes_after_stop(
        &mut self,
        expected_hash: u64,
        turn: u16,
    ) -> Result<[ActionOutcome; 2], MatchError> {
        let mut outcomes: [Option<ActionOutcome>; 2] = [None, None];

        self.send_stop_to_unfilled(&outcomes).await;

        let grace = self.ctx.playing_config.network_grace;
        let deadline = Some(Instant::now() + grace);

        loop {
            if outcomes[0].is_some() && outcomes[1].is_some() {
                break;
            }

            let outcome = poll_either(
                &mut self.ctx.players,
                &mut outcomes,
                expected_hash,
                turn,
                deadline,
            )
            .await?;

            match outcome {
                PollOutcome::Progress => continue,
                PollOutcome::Timeout => break,
            }
        }

        Ok(self.finalize_outcomes(outcomes, turn))
    }

    /// Send `Stop` to every slot whose outcome hasn't been filled yet.
    /// Failures are logged (the peer may already be gone) but don't
    /// propagate — collection proceeds.
    async fn send_stop_to_unfilled(&mut self, outcomes: &[Option<ActionOutcome>; 2]) {
        for (slot_idx, outcome_slot) in outcomes.iter().enumerate() {
            if outcome_slot.is_some() {
                continue;
            }
            let slot = slot_for(slot_idx);
            if let Err(e) = self.ctx.players[slot_idx].send(HostMsg::Stop).await {
                warn!(?slot, error = %e, "Stop send failed");
            }
        }
    }

    /// Lower `Option<ActionOutcome>` slots to concrete `[ActionOutcome; 2]`,
    /// emitting `BotTimeout` for any slot the wait loop didn't fill.
    fn finalize_outcomes(
        &self,
        outcomes: [Option<ActionOutcome>; 2],
        turn: u16,
    ) -> [ActionOutcome; 2] {
        let resolved = [
            outcomes[0].unwrap_or(ActionOutcome::TimedOut),
            outcomes[1].unwrap_or(ActionOutcome::TimedOut),
        ];
        for (slot_idx, outcome) in resolved.iter().enumerate() {
            if matches!(outcome, ActionOutcome::TimedOut) {
                emit(
                    self.ctx.event_tx.as_ref(),
                    MatchEvent::BotTimeout {
                        player: slot_for(slot_idx),
                        turn,
                    },
                );
            }
        }
        resolved
    }

    /// Run each per-slot outcome through the configured
    /// [`FaultPolicy`](super::FaultPolicy) to produce final directions.
    ///
    /// `BotTimeout` emission already happened at collection time
    /// (see [`Self::finalize_outcomes`]); this function is a pure policy
    /// hook plus the `take_provisional` call for slots that timed out.
    fn resolve_outcomes(
        &mut self,
        outcomes: [ActionOutcome; 2],
        expected_hash: u64,
        turn: u16,
    ) -> Result<Resolved, MatchError> {
        let policy = self.ctx.playing_config.fault_policy.clone();

        let mut directions = [Direction::Stay; 2];
        let mut think_ms = [0u32; 2];

        for slot_idx in 0..2 {
            let slot = slot_for(slot_idx);
            let outcome = outcomes[slot_idx];

            let provisional = match outcome {
                ActionOutcome::TimedOut => {
                    self.ctx.players[slot_idx].take_provisional(turn, expected_hash)
                },
                _ => None,
            };

            directions[slot_idx] = policy.resolve_action(slot, outcome, provisional)?;
            think_ms[slot_idx] = match outcome {
                ActionOutcome::Committed { think_ms, .. } => think_ms,
                _ => 0,
            };
        }

        Ok(Resolved {
            p1_action: directions[0],
            p2_action: directions[1],
            p1_think_ms: think_ms[0],
            p2_think_ms: think_ms[1],
        })
    }

    /// Apply resolved actions to the engine, emit `TurnPlayed`, and either
    /// notify both bots of `GameOver` (returning [`Match<Finished>`]) or
    /// queue a fresh `pending_advance` (returning [`Match<Playing>`]).
    ///
    /// Generic over the source phase: called from [`Match<Playing>::step`]
    /// and from [`Match<Collected>::advance`] / [`advance_with`]. The new
    /// `Playing.bot_synced_hash` keeps `bot_synced_hash` (bots are still
    /// synced to the pre-action state until the next Advance lands).
    async fn apply_resolved(
        mut self,
        turn_just_played: u16,
        bot_synced_hash: u64,
        resolved: Resolved,
    ) -> Result<StepResult, MatchError> {
        let result = self
            .ctx
            .game
            .process_turn(resolved.p1_action, resolved.p2_action);
        let new_hash = self.ctx.game.state_hash();

        emit(
            self.ctx.event_tx.as_ref(),
            MatchEvent::TurnPlayed {
                state: build_turn_state(&self.ctx.game, resolved.p1_action, resolved.p2_action),
                p1_action: resolved.p1_action,
                p2_action: resolved.p2_action,
                p1_think_ms: resolved.p1_think_ms,
                p2_think_ms: resolved.p2_think_ms,
            },
        );

        if result.game_over {
            let match_result = MatchResult::from_game(&self.ctx.game);
            for slot_idx in 0..2 {
                let slot = slot_for(slot_idx);
                if let Err(e) = self.ctx.players[slot_idx]
                    .send(HostMsg::GameOver {
                        result: match_result.result,
                        player1_score: match_result.player1_score,
                        player2_score: match_result.player2_score,
                    })
                    .await
                {
                    debug!(?slot, error = %e, "GameOver send failed — bot already gone");
                }
            }
            emit(
                self.ctx.event_tx.as_ref(),
                MatchEvent::MatchOver {
                    result: match_result.clone(),
                },
            );
            Ok(StepResult::GameOver(Match {
                ctx: self.ctx,
                state: Finished {
                    result: match_result,
                },
            }))
        } else {
            Ok(StepResult::Continue(Match {
                ctx: self.ctx,
                state: Playing {
                    turn: turn_just_played,
                    bot_synced_hash,
                    pending_advance: Some(PendingAdvance {
                        p1_action: resolved.p1_action,
                        p2_action: resolved.p2_action,
                        turn: turn_just_played + 1,
                        new_hash,
                    }),
                },
            }))
        }
    }
}

// ── Thinking: stop_and_collect ────────────────────────

impl Match<Thinking> {
    /// Inspect the turn the bots are thinking for.
    pub fn turn(&self) -> u16 {
        self.state.turn
    }

    /// Inspect the engine hash the bots are thinking about.
    pub fn bot_synced_hash(&self) -> u64 {
        self.state.bot_synced_hash
    }

    /// Send `Stop` to any bots that haven't already committed an Action this
    /// turn, wait the network grace window, and fold incoming Actions into
    /// per-slot [`ActionOutcome`]s. Slots not filled by the grace expiry
    /// become `TimedOut`. Emits `BotTimeout` for each timed-out slot.
    ///
    /// Pre-committed Actions (sitting in the recv queue between
    /// `start_turn` and `stop_and_collect`) are picked up early.
    ///
    /// Returns [`Match<Collected>`] carrying raw outcomes —
    /// [`FaultPolicy`](super::FaultPolicy) resolution happens in
    /// [`Match::advance`].
    pub async fn stop_and_collect(mut self) -> Result<Match<Collected>, MatchError> {
        let bot_synced_hash = self.state.bot_synced_hash;
        let turn = self.state.turn;
        let outcomes = self
            .collect_outcomes_after_stop(bot_synced_hash, turn)
            .await?;
        Ok(Match {
            ctx: self.ctx,
            state: Collected {
                turn,
                bot_synced_hash,
                outcomes,
            },
        })
    }
}

// ── Collected: outcomes / advance / advance_with ──────

impl Match<Collected> {
    /// Inspect the per-slot outcomes (read-only; consumed by `advance`).
    pub fn outcomes(&self) -> &[ActionOutcome; 2] {
        &self.state.outcomes
    }

    /// Inspect the turn the outcomes were collected for.
    pub fn turn(&self) -> u16 {
        self.state.turn
    }

    /// Run [`FaultPolicy`](super::FaultPolicy) over the collected outcomes
    /// to produce final directions, then apply to the engine and transition
    /// to [`Playing`] (with `pending_advance` queued) or [`Finished`].
    ///
    /// A strict policy may escalate `TimedOut` / `Disconnected` to
    /// [`MatchError`] here.
    pub async fn advance(mut self) -> Result<StepResult, MatchError> {
        let outcomes = self.state.outcomes;
        let bot_synced_hash = self.state.bot_synced_hash;
        let turn = self.state.turn;
        let resolved = self.resolve_outcomes(outcomes, bot_synced_hash, turn)?;
        self.apply_resolved(turn, bot_synced_hash, resolved).await
    }

    /// Drop the collected outcomes without applying them. Used by GUI
    /// analysis-mode when the user repositions the cursor mid-turn: the
    /// already-stopped bots' actions are abandoned and the match returns to
    /// [`Playing`] for a fresh `start_turn` / `start_turn_with`.
    ///
    /// No engine mutation happens — `process_turn` is only called by
    /// [`advance`](Self::advance) / [`advance_with`](Self::advance_with).
    /// The next `start_turn` sees `pending_advance: None` (the bots are
    /// already synced at `bot_synced_hash` since `stop_and_collect`
    /// consumed their committed Action).
    pub fn discard(self) -> Match<Playing> {
        let Self { ctx, state } = self;
        Match {
            ctx,
            state: Playing {
                turn: state.turn,
                bot_synced_hash: state.bot_synced_hash,
                pending_advance: None,
            },
        }
    }

    /// Override the bots' actions with caller-supplied directions, skipping
    /// [`FaultPolicy`] entirely. Used by GUI analysis-mode "play this move
    /// instead" navigation. `think_ms` reports as 0 in `TurnPlayed` since
    /// the bots' computation isn't being used.
    pub async fn advance_with(
        self,
        p1_dir: Direction,
        p2_dir: Direction,
    ) -> Result<StepResult, MatchError> {
        let bot_synced_hash = self.state.bot_synced_hash;
        let turn = self.state.turn;
        let resolved = Resolved {
            p1_action: p1_dir,
            p2_action: p2_dir,
            p1_think_ms: 0,
            p2_think_ms: 0,
        };
        self.apply_resolved(turn, bot_synced_hash, resolved).await
    }
}

// ── Helpers ───────────────────────────────────────────

/// Per-turn resolution: Direction + think_ms per slot, ready to apply to
/// the engine and report on `TurnPlayed`.
#[derive(Debug, Clone, Copy)]
struct Resolved {
    p1_action: Direction,
    p2_action: Direction,
    p1_think_ms: u32,
    p2_think_ms: u32,
}

#[derive(Debug, Clone, Copy)]
enum PollOutcome {
    /// Got a new action from one of the players (or a stale message we
    /// dropped). Caller should re-check whether both slots are filled.
    Progress,
    /// Deadline fired before any new action arrived.
    Timeout,
}

const fn slot_for(idx: usize) -> PlayerSlot {
    match idx {
        0 => PlayerSlot::Player1,
        _ => PlayerSlot::Player2,
    }
}

fn build_search_limits(cfg: &PlayingConfig) -> SearchLimits {
    let timeout_ms = if cfg.move_timeout.is_zero() {
        None
    } else {
        Some(cfg.move_timeout.as_millis() as u32)
    };
    SearchLimits {
        timeout_ms,
        depth: None,
        nodes: None,
    }
}

fn build_turn_state(game: &GameState, p1_last: Direction, p2_last: Direction) -> HashedTurnState {
    HashedTurnState::with_unverified_hash(
        build_turn_state_owned(game, p1_last, p2_last),
        game.state_hash(),
    )
}

fn build_turn_state_owned(game: &GameState, p1_last: Direction, p2_last: Direction) -> TurnState {
    TurnState {
        turn: game.turn,
        player1_position: game.player1.current_pos,
        player2_position: game.player2.current_pos,
        player1_score: game.player1.score,
        player2_score: game.player2.score,
        player1_mud_turns: game.player1.mud_timer,
        player2_mud_turns: game.player2.mud_timer,
        cheese: game.cheese.get_all_cheese_positions(),
        player1_last_move: p1_last,
        player2_last_move: p2_last,
    }
}

async fn recv_required(player: &mut dyn Player, slot: PlayerSlot) -> Result<BotMsg, MatchError> {
    match player.recv().await {
        Ok(Some(msg)) => Ok(msg),
        Ok(None) => Err(MatchError::BotDisconnected(slot)),
        Err(e) => Err(MatchError::from_player(slot, e)),
    }
}

async fn recv_with_timeout(
    player: &mut dyn Player,
    slot: PlayerSlot,
    timeout: Duration,
    timeout_err: MatchError,
) -> Result<BotMsg, MatchError> {
    if timeout.is_zero() {
        return recv_required(player, slot).await;
    }
    match tokio::time::timeout(timeout, player.recv()).await {
        Ok(Ok(Some(msg))) => Ok(msg),
        Ok(Ok(None)) => Err(MatchError::BotDisconnected(slot)),
        Ok(Err(PlayerError::Timeout)) => Err(timeout_err),
        Ok(Err(e)) => Err(MatchError::from_player(slot, e)),
        Err(_) => Err(timeout_err),
    }
}

/// Poll both players, filling `outcomes[slot]` with `Committed` on a valid
/// `Action`, `Disconnected` on clean close, or returning a hard
/// [`MatchError`] on protocol violations (wrong-slot, hash mismatch, transport
/// error, unexpected message). Stale-turn Actions are dropped silently.
///
/// A `None` deadline means wait forever (used in infinite-timeout mode where
/// Stop is consumer-driven). Already-filled slots are skipped via the
/// `tokio::select!` guard so a second message from the same bot in the same
/// turn doesn't get processed.
async fn poll_either(
    players: &mut [Box<dyn Player>; 2],
    outcomes: &mut [Option<ActionOutcome>; 2],
    expected_hash: u64,
    expected_turn: u16,
    deadline: Option<Instant>,
) -> Result<PollOutcome, MatchError> {
    let [pa, pb] = players;
    match deadline {
        Some(d) if d <= Instant::now() => Ok(PollOutcome::Timeout),
        Some(d) => {
            tokio::select! {
                biased;
                () = tokio::time::sleep_until(d) => Ok(PollOutcome::Timeout),
                res = pa.recv(), if outcomes[0].is_none() => {
                    handle_recv(res, PlayerSlot::Player1, &mut outcomes[0], expected_hash, expected_turn)?;
                    Ok(PollOutcome::Progress)
                }
                res = pb.recv(), if outcomes[1].is_none() => {
                    handle_recv(res, PlayerSlot::Player2, &mut outcomes[1], expected_hash, expected_turn)?;
                    Ok(PollOutcome::Progress)
                }
            }
        },
        None => {
            tokio::select! {
                biased;
                res = pa.recv(), if outcomes[0].is_none() => {
                    handle_recv(res, PlayerSlot::Player1, &mut outcomes[0], expected_hash, expected_turn)?;
                    Ok(PollOutcome::Progress)
                }
                res = pb.recv(), if outcomes[1].is_none() => {
                    handle_recv(res, PlayerSlot::Player2, &mut outcomes[1], expected_hash, expected_turn)?;
                    Ok(PollOutcome::Progress)
                }
            }
        },
    }
}

fn handle_recv(
    res: Result<Option<BotMsg>, PlayerError>,
    slot: PlayerSlot,
    outcome: &mut Option<ActionOutcome>,
    expected_hash: u64,
    expected_turn: u16,
) -> Result<(), MatchError> {
    match res {
        Ok(Some(BotMsg::Action {
            direction,
            player,
            turn,
            state_hash,
            think_ms,
        })) => {
            if player != slot {
                return Err(MatchError::UnexpectedMessage {
                    slot,
                    detail: format!("Action tagged for {player:?} on slot {slot:?}"),
                });
            }
            if turn != expected_turn {
                debug!(
                    ?slot,
                    msg_turn = turn,
                    expected_turn,
                    "stale action ignored"
                );
                return Ok(());
            }
            if state_hash != expected_hash {
                return Err(MatchError::ActionHashMismatch {
                    slot,
                    expected: expected_hash,
                    got: state_hash,
                });
            }
            *outcome = Some(ActionOutcome::Committed {
                direction,
                think_ms,
            });
            Ok(())
        },
        Ok(Some(other)) => Err(MatchError::UnexpectedMessage {
            slot,
            detail: format!("expected Action, got {other:?}"),
        }),
        Ok(None) => {
            *outcome = Some(ActionOutcome::Disconnected);
            Ok(())
        },
        Err(e) => Err(MatchError::from_player(slot, e)),
    }
}
