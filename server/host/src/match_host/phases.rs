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

/// Owner of a match's lifecycle. Holds the engine state, both players, the
/// per-player option assignments, and the event sink.
pub struct Match<S> {
    game: GameState,
    players: [Box<dyn Player>; 2],
    match_config: MatchConfig,
    /// Per-player option overrides keyed in slot order ([0] = Player1).
    options: [Vec<(String, String)>; 2],
    setup_timing: SetupTiming,
    playing_config: PlayingConfig,
    event_sink: EventSink,
    /// Internal sender backing `event_sink`, so the Match can call the
    /// `emit(Option<&Sender>, …)` helper directly without going through the
    /// public `EventSink::emit` (which exposes only the sink shape).
    event_tx: Option<tokio::sync::mpsc::UnboundedSender<MatchEvent>>,
    state: S,
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
            game,
            players,
            match_config,
            options,
            setup_timing,
            playing_config,
            event_sink,
            event_tx,
            state: Created,
        }
    }

    /// The event sink Players forward sideband through. Hand to
    /// `EmbeddedPlayer::accept` / `accept_players` so observer-facing
    /// messages reach the same channel as the events Match emits internally.
    pub fn event_sink(&self) -> &EventSink {
        &self.event_sink
    }
}

impl<S> Match<S> {
    pub fn game(&self) -> &GameState {
        &self.game
    }

    pub fn match_config(&self) -> &MatchConfig {
        &self.match_config
    }
}

impl Match<Finished> {
    pub fn result(&self) -> &MatchResult {
        &self.state.result
    }
}

// ── Setup: Configure → Ready → GoPreprocess ───────────

impl Match<Created> {
    /// Drive both bots from post-Welcome through Configure, Ready (with
    /// hash verification), GoPreprocess, and PreprocessingDone. Returns
    /// `Match<Ready>` carrying the engine hash both bots agreed to.
    pub async fn setup(mut self) -> Result<Match<Ready>, MatchError> {
        let expected_hash = self.game.state_hash();
        let configure_timeout = self.setup_timing.configure_timeout;
        let preprocessing_timeout = self.setup_timing.preprocessing_timeout;

        // Send Configure to both bots in slot order. The same MatchConfig
        // body goes to both — Match doesn't read or rewrite
        // `controlled_players` (kept for legacy callers; removed in slice 9).
        for slot_idx in 0..2 {
            let opts = std::mem::take(&mut self.options[slot_idx]);
            let msg = HostMsg::Configure {
                options: opts,
                match_config: Box::new(self.match_config.clone()),
            };
            let slot = slot_for(slot_idx);
            self.players[slot_idx]
                .send(msg)
                .await
                .map_err(|e| MatchError::from_player(slot, e))?;
        }

        // Recv Ready from each, verify hash.
        for slot_idx in 0..2 {
            let slot = slot_for(slot_idx);
            let msg = recv_with_timeout(
                self.players[slot_idx].as_mut(),
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
        }

        // Send GoPreprocess to both, recv PreprocessingDone.
        emit(self.event_tx.as_ref(), MatchEvent::PreprocessingStarted);
        for slot_idx in 0..2 {
            let slot = slot_for(slot_idx);
            self.players[slot_idx]
                .send(HostMsg::GoPreprocess {
                    state_hash: expected_hash,
                })
                .await
                .map_err(|e| MatchError::from_player(slot, e))?;
        }
        for slot_idx in 0..2 {
            let slot = slot_for(slot_idx);
            let msg = recv_with_timeout(
                self.players[slot_idx].as_mut(),
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
        }

        emit(self.event_tx.as_ref(), MatchEvent::SetupComplete);

        Ok(Match {
            game: self.game,
            players: self.players,
            match_config: self.match_config,
            options: self.options,
            setup_timing: self.setup_timing,
            playing_config: self.playing_config,
            event_sink: self.event_sink,
            event_tx: self.event_tx,
            state: Ready {
                bot_synced_hash: expected_hash,
            },
        })
    }

    /// Convenience: setup → start → step until game over → return result.
    /// Closes both players (best effort) before returning, regardless of
    /// success or failure.
    pub async fn run(self) -> Result<MatchResult, MatchError> {
        let ready = match self.setup().await {
            Ok(r) => r,
            Err(e) => return Err(e),
        };
        let mut playing = ready.start();
        loop {
            match playing.step().await? {
                StepResult::Continue(next) => playing = next,
                StepResult::GameOver(finished) => {
                    let [p1, p2] = finished.players;
                    let _ = p1.close().await;
                    let _ = p2.close().await;
                    return Ok(finished.state.result);
                },
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
            self.event_tx.as_ref(),
            MatchEvent::MatchStarted {
                config: self.match_config.clone(),
            },
        );
        Match {
            game: self.game,
            players: self.players,
            match_config: self.match_config,
            options: self.options,
            setup_timing: self.setup_timing,
            playing_config: self.playing_config,
            event_sink: self.event_sink,
            event_tx: self.event_tx,
            state: Playing {
                turn: 0,
                bot_synced_hash,
                pending_advance: None,
            },
        }
    }
}

// ── Playing::step ─────────────────────────────────────

impl Match<Playing> {
    /// Advance the match by one full turn:
    /// 1. Optional Advance + SyncOk (skipped on the first turn).
    /// 2. Send Go to both, await Action with deadline + Stop fallback.
    /// 3. Apply actions to the engine, emit `TurnPlayed`.
    /// 4. Either `GameOver` (and return `Finished`) or queue a new
    ///    `pending_advance` (and return `Continue`).
    pub async fn step(mut self) -> Result<StepResult, MatchError> {
        // 1. Acknowledge previous turn (skip on first turn).
        if let Some(pa) = self.state.pending_advance.take() {
            self.run_advance(pa).await?;
            self.state.bot_synced_hash = pa.new_hash;
            self.state.turn = pa.turn;
        }

        // 2. Send Go (clears each Player's stored provisional via the
        //    `send(Go|GoState)` whole-turn-boundary contract).
        let go_hash = self.state.bot_synced_hash;
        let limits = build_search_limits(&self.playing_config);
        for slot_idx in 0..2 {
            let slot = slot_for(slot_idx);
            self.players[slot_idx]
                .send(HostMsg::Go {
                    state_hash: go_hash,
                    limits: limits.clone(),
                })
                .await
                .map_err(|e| MatchError::from_player(slot, e))?;
        }

        // 3. Collect actions with per-turn deadline + Stop fallback.
        let collected = self.collect_actions(go_hash).await?;

        // 4. Apply to engine.
        let result = self
            .game
            .process_turn(collected.p1_action, collected.p2_action);
        let new_hash = self.game.state_hash();

        emit(
            self.event_tx.as_ref(),
            MatchEvent::TurnPlayed {
                state: build_turn_state(&self.game, collected.p1_action, collected.p2_action),
                p1_action: collected.p1_action,
                p2_action: collected.p2_action,
                p1_think_ms: collected.p1_think_ms,
                p2_think_ms: collected.p2_think_ms,
            },
        );

        if result.game_over {
            // Notify both bots, emit MatchOver, return Finished.
            let match_result = MatchResult::from_game(&self.game);
            for slot_idx in 0..2 {
                let slot = slot_for(slot_idx);
                if let Err(e) = self.players[slot_idx]
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
                self.event_tx.as_ref(),
                MatchEvent::MatchOver {
                    result: match_result.clone(),
                },
            );
            Ok(StepResult::GameOver(Match {
                game: self.game,
                players: self.players,
                match_config: self.match_config,
                options: self.options,
                setup_timing: self.setup_timing,
                playing_config: self.playing_config,
                event_sink: self.event_sink,
                event_tx: self.event_tx,
                state: Finished {
                    result: match_result,
                },
            }))
        } else {
            self.state.pending_advance = Some(PendingAdvance {
                p1_action: collected.p1_action,
                p2_action: collected.p2_action,
                turn: self.state.turn + 1,
                new_hash,
            });
            Ok(StepResult::Continue(self))
        }
    }

    /// Send `Advance` to both players, await SyncOk from each, handling
    /// `Resync → FullState → SyncOk` with a bounded retry (1 per player per
    /// turn). Verifies each `SyncOk.hash` against the host's `new_hash`.
    async fn run_advance(&mut self, pa: PendingAdvance) -> Result<(), MatchError> {
        for slot_idx in 0..2 {
            let slot = slot_for(slot_idx);
            self.players[slot_idx]
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
            let msg = recv_required(self.players[slot_idx].as_mut(), slot).await?;
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
                    let turn_state = build_turn_state_owned(&self.game, pa.p1_action, pa.p2_action);
                    self.players[slot_idx]
                        .send(HostMsg::FullState {
                            match_config: Box::new(self.match_config.clone()),
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

    /// Wait for an `Action` from each player. On per-turn deadline expiry,
    /// send `Stop` to both, accept any in-flight Action within the network
    /// grace window, then fall back to the player's stored provisional or
    /// `Direction::Stay`.
    async fn collect_actions(&mut self, expected_hash: u64) -> Result<Collected, MatchError> {
        let mut p1: Option<ActionSlot> = None;
        let mut p2: Option<ActionSlot> = None;

        let move_timeout = self.playing_config.move_timeout;
        let infinite = move_timeout.is_zero();
        let deadline = if infinite {
            None
        } else {
            Some(Instant::now() + move_timeout)
        };
        let grace = self.playing_config.network_grace;

        let mut stop_sent = false;
        let mut effective_deadline = deadline;
        loop {
            if p1.is_some() && p2.is_some() {
                break;
            }

            let outcome = poll_either(
                &mut self.players,
                &mut p1,
                &mut p2,
                expected_hash,
                self.state.turn,
                effective_deadline,
            )
            .await?;

            match outcome {
                PollOutcome::Progress => continue,
                PollOutcome::Timeout if !stop_sent => {
                    debug!(
                        turn = self.state.turn,
                        "move timeout — sending Stop and entering grace window"
                    );
                    for slot_idx in 0..2 {
                        let slot = slot_for(slot_idx);
                        if let Err(e) = self.players[slot_idx].send(HostMsg::Stop).await {
                            warn!(?slot, error = %e, "Stop send failed");
                        }
                    }
                    stop_sent = true;
                    effective_deadline = Some(Instant::now() + grace);
                },
                PollOutcome::Timeout => break, // grace expired
            }
        }

        // Any missing slots resolve to provisional (turn-scoped) or Stay.
        let p1_resolved = self.resolve_slot(0, expected_hash, p1);
        let p2_resolved = self.resolve_slot(1, expected_hash, p2);

        Ok(Collected {
            p1_action: p1_resolved.direction,
            p2_action: p2_resolved.direction,
            p1_think_ms: p1_resolved.think_ms,
            p2_think_ms: p2_resolved.think_ms,
        })
    }

    /// Resolve a slot to a final action: committed > provisional > Stay.
    /// Emits `BotTimeout` when neither a committed action nor a provisional
    /// is available.
    fn resolve_slot(
        &mut self,
        slot_idx: usize,
        expected_hash: u64,
        action: Option<ActionSlot>,
    ) -> ActionSlot {
        if let Some(a) = action {
            return a;
        }
        let slot = slot_for(slot_idx);
        let turn = self.state.turn;
        let provisional = self.players[slot_idx].take_provisional(turn, expected_hash);
        emit(
            self.event_tx.as_ref(),
            MatchEvent::BotTimeout { player: slot, turn },
        );
        ActionSlot {
            direction: provisional.unwrap_or(Direction::Stay),
            think_ms: 0,
        }
    }
}

// ── Helpers ───────────────────────────────────────────

#[derive(Debug, Clone, Copy)]
struct ActionSlot {
    direction: Direction,
    think_ms: u32,
}

#[derive(Debug, Clone, Copy)]
struct Collected {
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

/// Poll both players for the next [`BotMsg::Action`], honoring an optional
/// deadline. `Action`s tagged for the wrong slot or for a stale turn are
/// rejected (as `UnexpectedMessage`) or dropped (stale) per the protocol
/// spec. A `None` deadline means wait forever (used in infinite-timeout
/// mode where Stop is consumer-driven).
async fn poll_either(
    players: &mut [Box<dyn Player>; 2],
    p1: &mut Option<ActionSlot>,
    p2: &mut Option<ActionSlot>,
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
                res = pa.recv(), if p1.is_none() => {
                    handle_recv(res, PlayerSlot::Player1, p1, p2, expected_hash, expected_turn)?;
                    Ok(PollOutcome::Progress)
                }
                res = pb.recv(), if p2.is_none() => {
                    handle_recv(res, PlayerSlot::Player2, p1, p2, expected_hash, expected_turn)?;
                    Ok(PollOutcome::Progress)
                }
            }
        },
        None => {
            tokio::select! {
                biased;
                res = pa.recv(), if p1.is_none() => {
                    handle_recv(res, PlayerSlot::Player1, p1, p2, expected_hash, expected_turn)?;
                    Ok(PollOutcome::Progress)
                }
                res = pb.recv(), if p2.is_none() => {
                    handle_recv(res, PlayerSlot::Player2, p1, p2, expected_hash, expected_turn)?;
                    Ok(PollOutcome::Progress)
                }
            }
        },
    }
}

fn handle_recv(
    res: Result<Option<BotMsg>, PlayerError>,
    slot: PlayerSlot,
    p1: &mut Option<ActionSlot>,
    p2: &mut Option<ActionSlot>,
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
            let target = match slot {
                PlayerSlot::Player1 => p1,
                _ => p2,
            };
            *target = Some(ActionSlot {
                direction,
                think_ms,
            });
            Ok(())
        },
        Ok(Some(other)) => Err(MatchError::UnexpectedMessage {
            slot,
            detail: format!("expected Action, got {other:?}"),
        }),
        Ok(None) => Err(MatchError::BotDisconnected(slot)),
        Err(e) => Err(MatchError::from_player(slot, e)),
    }
}
