//! In-process [`Player`](super::Player) implementation.
//!
//! `EmbeddedPlayer` runs a bot in the same process as the Match. No TCP, no
//! FlatBuffers, no subprocess. A dispatcher task translates [`HostMsg`] into
//! method calls on an [`EmbeddedBot`] and wraps the results in [`BotMsg`].
//!
//! ## Who writes what
//!
//! - Bot authors implement [`EmbeddedBot`] (and its supertrait [`Options`]).
//!   The shape mirrors the SDK `Bot` trait exactly (`think`, `preprocess`,
//!   `on_game_over`) so a single mental model covers both embedded and
//!   networked bots.
//! - The host constructs [`EmbeddedPlayer::new`] and hands the resulting
//!   [`Player`](super::Player) impl to Match.
//!
//! ## Sideband
//!
//! [`EmbeddedCtx`] exposes `send_info` / `send_provisional` / `should_stop`
//! with the same API surface as the SDK `Context`. Calls are routed to the
//! [`EventSink`](super::EventSink) instead of an mpsc-backed codec.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use pyrat::{Coordinates, Direction, GameBuilder, GameState, MudMap};
use pyrat_bot_api::{BotContext, InfoParams, InfoSink, Options};
use pyrat_protocol::{
    BotMsg, HashedTurnState, HostMsg, OwnedInfo, OwnedMatchConfig, OwnedTurnState, SearchLimits,
};
use pyrat_wire::{GameResult, Player as PlayerSlot};
use tokio::sync::mpsc;
use tokio::task::JoinHandle;

use super::{EventSink, Player, PlayerError, PlayerIdentity};

/// Maximum time `close` waits for the dispatcher to exit before aborting it.
/// A bot that ignores `ctx.should_stop()` may still be running on a
/// `spawn_blocking` thread when this fires; the task is detached. Honors the
/// `Player::close` "best-effort, bounded" contract.
const CLOSE_GRACE: Duration = Duration::from_secs(1);

/// In-process bot interface.
///
/// Shape mirrors the SDK `Bot` trait so a bot author switches between
/// networked and embedded with the same mental model. Argument types use
/// [`pyrat_protocol`] owned types directly.
pub trait EmbeddedBot: Options + Send + 'static {
    /// Choose a direction for this turn.
    fn think(&mut self, state: &HashedTurnState, ctx: &EmbeddedCtx) -> Direction;

    /// Called once before the first turn. Default: no-op.
    fn preprocess(&mut self, _state: &HashedTurnState, _ctx: &EmbeddedCtx) {}

    /// Called when the game ends. Default: no-op.
    fn on_game_over(&mut self, _result: GameResult, _scores: (f32, f32)) {}
}

/// Per-turn context passed to [`EmbeddedBot::think`] and
/// [`EmbeddedBot::preprocess`].
///
/// Shape mirrors the SDK `Context` API surface. `should_stop` reads both an
/// atomic the dispatcher flips on [`HostMsg::Stop`] and an optional per-turn
/// deadline derived from the match config. Sideband routing:
/// - `send_info` forwarded to [`EventSink`] as `MatchEvent::BotInfo`.
/// - `send_provisional` forwarded to the Match's `recv()` queue as
///   [`BotMsg::Provisional`] (Match uses the latest as timeout fallback).
pub struct EmbeddedCtx {
    pub(crate) event_sink: EventSink,
    pub(crate) bot_tx: mpsc::UnboundedSender<BotMsg>,
    pub(crate) turn: u16,
    pub(crate) state_hash: u64,
    pub(crate) player: PlayerSlot,
    pub(crate) stopped: Arc<AtomicBool>,
    pub(crate) think_start: Instant,
    pub(crate) deadline: Option<Instant>,
}

impl EmbeddedCtx {
    /// True when the bot should stop: host sent Stop, or the per-turn
    /// deadline passed. Cooperatively polled from the bot's think loop.
    pub fn should_stop(&self) -> bool {
        if self.stopped.load(Ordering::Relaxed) {
            return true;
        }
        self.deadline.is_some_and(|d| Instant::now() >= d)
    }

    /// Milliseconds remaining before the deadline. Returns 0 if the bot has
    /// been told to stop; `u64::MAX` if no deadline was configured.
    pub fn time_remaining_ms(&self) -> u64 {
        if self.stopped.load(Ordering::Relaxed) {
            return 0;
        }
        let Some(deadline) = self.deadline else {
            return u64::MAX;
        };
        deadline
            .checked_duration_since(Instant::now())
            .map_or(0, |d| d.as_millis() as u64)
    }

    /// Milliseconds elapsed since think started.
    pub fn think_elapsed_ms(&self) -> u32 {
        self.think_start.elapsed().as_millis() as u32
    }

    /// Send an Info message. Routed to the attached [`EventSink`] as a
    /// `MatchEvent::BotInfo` (observer-facing, never inspected by Match).
    pub fn send_info(&self, params: &InfoParams<'_>) {
        emit_bot_info(
            &self.event_sink,
            self.player,
            params,
            self.turn,
            self.state_hash,
        );
    }

    /// Send a provisional (best-so-far) direction. Emitted as
    /// [`BotMsg::Provisional`] on the game-driving channel: Match holds the
    /// latest as its timeout fallback.
    pub fn send_provisional(&self, direction: Direction) {
        if self
            .bot_tx
            .send(BotMsg::Provisional {
                direction,
                player: self.player,
                turn: self.turn,
                state_hash: self.state_hash,
            })
            .is_err()
        {
            tracing::debug!(
                player = ?self.player,
                turn = self.turn,
                "provisional dropped: bot_tx closed"
            );
        }
    }
}

impl BotContext for EmbeddedCtx {
    fn player(&self) -> PlayerSlot {
        self.player
    }

    fn turn(&self) -> u16 {
        self.turn
    }

    fn state_hash(&self) -> u64 {
        self.state_hash
    }

    fn should_stop(&self) -> bool {
        EmbeddedCtx::should_stop(self)
    }

    fn time_remaining_ms(&self) -> u64 {
        EmbeddedCtx::time_remaining_ms(self)
    }

    fn think_elapsed_ms(&self) -> u32 {
        EmbeddedCtx::think_elapsed_ms(self)
    }

    fn send_info(&self, params: &InfoParams<'_>) {
        EmbeddedCtx::send_info(self, params);
    }

    fn send_provisional(&self, direction: Direction) {
        EmbeddedCtx::send_provisional(self, direction);
    }

    fn info_sender(&self) -> Option<pyrat_bot_api::InfoSender> {
        let sink: Arc<dyn InfoSink> = Arc::new(EmbeddedInfoSink {
            event_sink: self.event_sink.clone(),
            player: self.player,
        });
        Some(pyrat_bot_api::InfoSender::new(sink))
    }
}

/// Adapter so `InfoSender` handles from worker threads reach the Match's
/// event sink as `MatchEvent::BotInfo`.
struct EmbeddedInfoSink {
    event_sink: EventSink,
    player: PlayerSlot,
}

impl InfoSink for EmbeddedInfoSink {
    fn send_info(&self, params: &InfoParams<'_>, turn: u16, state_hash: u64) {
        emit_bot_info(&self.event_sink, self.player, params, turn, state_hash);
    }
}

fn emit_bot_info(
    event_sink: &EventSink,
    sender: PlayerSlot,
    params: &InfoParams<'_>,
    turn: u16,
    state_hash: u64,
) {
    let info = OwnedInfo {
        player: params.player,
        multipv: params.multipv,
        target: params.target,
        depth: params.depth,
        nodes: params.nodes,
        score: params.score,
        pv: params.pv.to_vec(),
        message: params.message.to_string(),
        turn,
        state_hash,
    };
    event_sink.emit(crate::game_loop::MatchEvent::BotInfo {
        sender,
        turn,
        state_hash,
        info,
    });
}

// ── EmbeddedPlayer ────────────────────────────────────

/// In-process [`Player`] implementation.
///
/// Owns a dispatcher task that translates [`HostMsg`] → bot method calls →
/// [`BotMsg`]. See module docs for the design rationale.
pub struct EmbeddedPlayer {
    identity: PlayerIdentity,
    host_tx: mpsc::UnboundedSender<HostMsg>,
    bot_rx: mpsc::UnboundedReceiver<BotMsg>,
    dispatcher: Option<JoinHandle<Result<(), PlayerError>>>,
    /// Cooperative-stop signal shared with the dispatcher and any in-flight
    /// `EmbeddedCtx`. `close` flips this so bots polling `should_stop` exit.
    stopped: Arc<AtomicBool>,
}

impl EmbeddedPlayer {
    /// Construct an EmbeddedPlayer wrapping `bot`. Spawns a dispatcher task
    /// on the current tokio runtime.
    ///
    /// # Panics
    ///
    /// Panics if called outside a tokio runtime (via `tokio::spawn`).
    pub fn new<B: EmbeddedBot>(bot: B, identity: PlayerIdentity, event_sink: EventSink) -> Self {
        let (host_tx, host_rx) = mpsc::unbounded_channel();
        let (bot_tx, bot_rx) = mpsc::unbounded_channel();
        let stopped = Arc::new(AtomicBool::new(false));
        let dispatcher = tokio::spawn(dispatcher_task(
            bot,
            identity.clone(),
            event_sink,
            host_rx,
            bot_tx,
            stopped.clone(),
        ));
        Self {
            identity,
            host_tx,
            bot_rx,
            dispatcher: Some(dispatcher),
            stopped,
        }
    }

    /// Await the dispatcher handle and map its exit state. Callable at most
    /// once; subsequent calls short-circuit to Ok.
    async fn reap_dispatcher(&mut self) -> Result<(), PlayerError> {
        reap(self.dispatcher.take()).await
    }
}

impl Player for EmbeddedPlayer {
    fn identity(&self) -> &PlayerIdentity {
        &self.identity
    }

    async fn send(&mut self, msg: HostMsg) -> Result<(), PlayerError> {
        self.host_tx
            .send(msg)
            .map_err(|_| PlayerError::TransportError("dispatcher closed".into()))
    }

    async fn recv(&mut self) -> Result<Option<BotMsg>, PlayerError> {
        match self.bot_rx.recv().await {
            Some(msg) => Ok(Some(msg)),
            None => {
                // Dispatcher has dropped its sender → it has exited. Surface
                // its exit reason, if any.
                self.reap_dispatcher().await?;
                Ok(None)
            },
        }
    }

    async fn close(self) -> Result<(), PlayerError> {
        let Self {
            host_tx,
            bot_rx,
            dispatcher,
            stopped,
            identity: _,
        } = self;
        // Signal cooperative bots first so they get the earliest possible
        // chance to observe `should_stop()`. Order matters: another tokio
        // worker can drive the dispatcher past the closed channel before our
        // store completes if we drop host_tx first.
        stopped.store(true, Ordering::Relaxed);
        drop(host_tx);
        // Don't drain bot_rx: queued BotMsgs at close time are observer state
        // nobody is consuming, and any in-flight EmbeddedCtx in spawn_blocking
        // holds a bot_tx clone that keeps the channel open until the bot
        // returns — that's the unbounded-wait bug we're fixing.
        drop(bot_rx);

        let Some(mut handle) = dispatcher else {
            return Ok(());
        };
        // Bounded wait for the dispatcher to exit. We borrow the JoinHandle
        // so the timeout branch can still call `abort()` on it — using
        // `tokio::time::timeout(_, reap(dispatcher))` would consume the
        // handle into reap and drop it on timeout, leaving nothing to abort.
        tokio::select! {
            result = &mut handle => match result {
                // Swallow any dispatcher Err: close was intentional, and the
                // contract authorizes "if the peer is already gone, swallow
                // the error and return Ok(())". The expected error here is
                // `TransportError("host_rx closed while bot is working")`.
                Ok(_) => Ok(()),
                Err(join_err) => Err(PlayerError::TransportError(format!(
                    "dispatcher panicked: {join_err}"
                ))),
            },
            () = tokio::time::sleep(CLOSE_GRACE) => {
                // The bot ignored `should_stop`. Abort the dispatcher; the
                // spawn_blocking task running the bot is detached, which is
                // unavoidable since spawn_blocking tasks cannot be aborted.
                handle.abort();
                Ok(())
            }
        }
    }
}

async fn reap(dispatcher: Option<JoinHandle<Result<(), PlayerError>>>) -> Result<(), PlayerError> {
    let Some(handle) = dispatcher else {
        return Ok(());
    };
    match handle.await {
        Ok(Ok(())) => Ok(()),
        Ok(Err(e)) => Err(e),
        Err(join_err) => Err(PlayerError::TransportError(format!(
            "dispatcher panicked: {join_err}"
        ))),
    }
}

// ── Dispatcher ────────────────────────────────────────

/// Run one bot to completion.
///
/// Lifecycle: setup typestate chain (Initial → Connected → Identified) then
/// the playing loop (Playing<Synced> alternating with Playing<Desynced> on
/// hash mismatches). Exits cleanly on [`HostMsg::GameOver`], with an error on
/// unexpected messages or channel closure.
async fn dispatcher_task<B: EmbeddedBot>(
    bot: B,
    identity: PlayerIdentity,
    event_sink: EventSink,
    host_rx: mpsc::UnboundedReceiver<HostMsg>,
    bot_tx: mpsc::UnboundedSender<BotMsg>,
    stopped: Arc<AtomicBool>,
) -> Result<(), PlayerError> {
    let core = DispatcherCore {
        event_sink,
        host_rx,
        bot_tx,
        stopped,
    };

    let initial = Setup::<B, Initial>::new(bot, core);
    let Some(connected) = initial.emit_identify(&identity)? else {
        return Ok(());
    };
    let Some(identified) = connected.await_welcome().await? else {
        return Ok(());
    };
    let Some(mut playing) = identified.await_configure_emit_ready().await? else {
        return Ok(());
    };

    loop {
        match playing.next_event().await? {
            Event::Continue(p) => playing = p,
            Event::Desynced(d) => {
                let Some(p) = d.recover().await? else {
                    return Ok(());
                };
                playing = p;
            },
            Event::GameOver | Event::CleanClose => return Ok(()),
        }
    }
}

/// Data shared across every dispatcher state: channels, event sink, stop
/// flag. The bot lives separately on each state struct so it can move in and
/// out of `spawn_blocking` without any `Option` / sentinel dance.
struct DispatcherCore {
    event_sink: EventSink,
    host_rx: mpsc::UnboundedReceiver<HostMsg>,
    bot_tx: mpsc::UnboundedSender<BotMsg>,
    stopped: Arc<AtomicBool>,
}

// ── Setup typestate ───────────────────────────────────
//
// Each state is a marker type. `Setup<B, S>` threads them through transitions
// that consume `self` and return the next state, so skipping or reordering a
// step is a compile error. State-specific data (`player_slot` after Welcome)
// lives on the marker struct.

struct Setup<B, S> {
    bot: B,
    core: DispatcherCore,
    state: S,
}

struct Initial;
struct Connected;
struct Identified {
    player_slot: PlayerSlot,
}

impl<B: EmbeddedBot> Setup<B, Initial> {
    fn new(bot: B, core: DispatcherCore) -> Self {
        Self {
            bot,
            core,
            state: Initial,
        }
    }

    /// Emit `BotMsg::Identify` from the bot's declared identity + options.
    /// Returns `Ok(None)` if the player handle was already dropped.
    fn emit_identify(
        self,
        identity: &PlayerIdentity,
    ) -> Result<Option<Setup<B, Connected>>, PlayerError> {
        let options = self.bot.option_defs();
        if self
            .core
            .bot_tx
            .send(BotMsg::Identify {
                name: identity.name.clone(),
                author: identity.author.clone(),
                agent_id: identity.agent_id.clone(),
                options,
            })
            .is_err()
        {
            return Ok(None);
        }
        Ok(Some(Setup {
            bot: self.bot,
            core: self.core,
            state: Connected,
        }))
    }
}

impl<B: EmbeddedBot> Setup<B, Connected> {
    /// Await `HostMsg::Welcome`, store the assigned player slot.
    ///
    /// Returns `Ok(None)` if the player handle was dropped before Welcome
    /// arrived (clean local close).
    async fn await_welcome(mut self) -> Result<Option<Setup<B, Identified>>, PlayerError> {
        match self.core.host_rx.recv().await {
            Some(HostMsg::Welcome { player_slot }) => Ok(Some(Setup {
                bot: self.bot,
                core: self.core,
                state: Identified { player_slot },
            })),
            Some(HostMsg::ProtocolError { reason }) => Err(PlayerError::ProtocolError(reason)),
            Some(other) => Err(PlayerError::ProtocolError(format!(
                "expected Welcome, got {other:?}"
            ))),
            None => Ok(None),
        }
    }
}

impl<B: EmbeddedBot> Setup<B, Identified> {
    /// Await `HostMsg::Configure`, apply options, build the local engine
    /// state mirror, compute the initial hash, emit `BotMsg::Ready`.
    /// Returns a `Playing<Synced>`.
    ///
    /// Options whose `apply_option` returns `Err` are logged but do not abort
    /// setup (fault policy: "warn and use default", matching the SDK).
    /// Returns `Ok(None)` if the player handle was dropped before Configure
    /// arrived or before Ready could be emitted (clean local close).
    async fn await_configure_emit_ready(
        mut self,
    ) -> Result<Option<Playing<B, Synced>>, PlayerError> {
        let (options, match_config) = match self.core.host_rx.recv().await {
            Some(HostMsg::Configure {
                options,
                match_config,
            }) => (options, match_config),
            Some(HostMsg::ProtocolError { reason }) => {
                return Err(PlayerError::ProtocolError(reason));
            },
            Some(other) => {
                return Err(PlayerError::ProtocolError(format!(
                    "expected Configure, got {other:?}"
                )));
            },
            None => return Ok(None),
        };

        for (name, value) in &options {
            if let Err(err) = self.bot.apply_option(name, value) {
                tracing::warn!(option = %name, error = %err, "bot rejected option");
            }
        }

        let game = build_engine_state(&match_config)?;
        let hash = hash_from_game(&game, Direction::Stay, Direction::Stay);

        if self
            .core
            .bot_tx
            .send(BotMsg::Ready { state_hash: hash })
            .is_err()
        {
            return Ok(None);
        }

        Ok(Some(Playing {
            bot: Some(self.bot),
            core: self.core,
            player_slot: self.state.player_slot,
            match_config,
            game,
            last_moves: (Direction::Stay, Direction::Stay),
            state: Synced {
                inner: InnerState::Idle,
            },
        }))
    }
}

// ── Playing<S> typestate ──────────────────────────────

/// Sync status markers. `Playing<Synced>` exposes the turn-handling methods
/// and carries the runtime `InnerState`; `Playing<Desynced>` exposes only
/// `recover` and carries no inner state.
struct Synced {
    inner: InnerState,
}
struct Desynced;

/// Inner state machine inside `Playing<Synced>`. Tracked as a runtime enum
/// because typestate in a message-driven loop collapses back to this shape
/// anyway.
#[derive(Debug, Clone, Copy)]
enum InnerState {
    /// Between turns, awaiting Advance / GoState / GameOver.
    Idle,
    /// Preprocessing phase: awaiting Stop (optional) while the bot runs
    /// `preprocess`.
    Preprocessing,
    /// Post-Advance: SyncOk emitted, awaiting Go or FullState.
    Syncing,
    /// Thinking phase: bot is running `think`, awaiting Stop (optional).
    Thinking,
}

struct Playing<B, S> {
    /// The bot. `Option` so we can temporarily move it into a
    /// `spawn_blocking` closure during think/preprocess; always `Some` at
    /// every `.await` point in `next_event`. Unwrap sites document the
    /// invariant.
    bot: Option<B>,
    core: DispatcherCore,
    player_slot: PlayerSlot,
    match_config: Box<OwnedMatchConfig>,
    /// Local engine state mirror. Authoritative for the client's view.
    game: GameState,
    /// Last moves applied (for `OwnedTurnState::player{1,2}_last_move`).
    last_moves: (Direction, Direction),
    state: S,
}

impl<B, S> Playing<B, S> {
    /// Swap the sync-state marker, keeping everything else. Used for the
    /// `Synced` <-> `Desynced` transitions at Advance-mismatch and recovery.
    fn with_state<S2>(self, state: S2) -> Playing<B, S2> {
        Playing {
            bot: self.bot,
            core: self.core,
            player_slot: self.player_slot,
            match_config: self.match_config,
            game: self.game,
            last_moves: self.last_moves,
            state,
        }
    }
}

/// Return value of one iteration of `Playing<Synced>::next_event`.
enum Event<B> {
    /// Still Synced; loop continues.
    Continue(Playing<B, Synced>),
    /// Hash mismatch observed; await FullState before doing anything else.
    Desynced(Playing<B, Desynced>),
    /// `HostMsg::GameOver` received and processed; dispatcher exits.
    GameOver,
    /// Player handle dropped (host_rx closed or bot_tx send failed).
    /// Dispatcher exits cleanly with `Ok(())`.
    CleanClose,
}

impl<B: EmbeddedBot> Playing<B, Synced> {
    /// Advance the dispatcher by one host message.
    ///
    /// Takes `self` and returns one of:
    /// - `Event::Continue`: still Synced, loop with the returned value.
    /// - `Event::Desynced`: hash mismatch detected, Resync emitted.
    /// - `Event::GameOver`: GameOver processed, dispatcher exits.
    async fn next_event(mut self) -> Result<Event<B>, PlayerError> {
        let Some(msg) = self.core.host_rx.recv().await else {
            return Ok(Event::CleanClose);
        };

        match msg {
            HostMsg::GoPreprocess { state_hash } => self.handle_go_preprocess(state_hash).await,
            HostMsg::Go { state_hash, limits } => self.handle_go(state_hash, limits).await,
            HostMsg::GoState {
                turn_state,
                state_hash,
                limits,
            } => self.handle_go_state(*turn_state, state_hash, limits).await,
            HostMsg::Advance {
                p1_dir,
                p2_dir,
                turn,
                new_hash,
            } => self.handle_advance(p1_dir, p2_dir, turn, new_hash),
            HostMsg::GameOver {
                result,
                player1_score,
                player2_score,
            } => {
                self.bot
                    .as_mut()
                    .expect("bot present between turns")
                    .on_game_over(result, (player1_score, player2_score));
                Ok(Event::GameOver)
            },
            HostMsg::Stop => {
                // Stop outside of thinking/preprocessing is a no-op: nothing
                // to interrupt. Silently ignored.
                Ok(Event::Continue(self))
            },
            HostMsg::FullState { .. } => Err(PlayerError::ProtocolError(format!(
                "FullState received while Synced, server sent without Resync \
                 (player={:?}, turn={})",
                self.player_slot, self.game.turn
            ))),
            HostMsg::Welcome { .. } | HostMsg::Configure { .. } => {
                Err(PlayerError::ProtocolError(format!(
                    "setup message in Playing: {msg:?} (player={:?}, turn={})",
                    self.player_slot, self.game.turn
                )))
            },
            HostMsg::ProtocolError { reason } => Err(PlayerError::ProtocolError(reason)),
        }
    }

    /// Handle GoPreprocess: run bot.preprocess, emit PreprocessingDone.
    async fn handle_go_preprocess(mut self, state_hash: u64) -> Result<Event<B>, PlayerError> {
        if !matches!(self.state.inner, InnerState::Idle) {
            return Err(PlayerError::ProtocolError(format!(
                "GoPreprocess in state {:?} (player={:?}, turn={})",
                self.state.inner, self.player_slot, self.game.turn
            )));
        }
        self.state.inner = InnerState::Preprocessing;
        self.core.stopped.store(false, Ordering::Relaxed);
        self.run_preprocess(state_hash).await?;
        if self.core.bot_tx.send(BotMsg::PreprocessingDone).is_err() {
            return Ok(Event::CleanClose);
        }
        self.state.inner = InnerState::Idle;
        Ok(Event::Continue(self))
    }

    /// Handle Go: run bot.think, emit Action.
    async fn handle_go(
        mut self,
        state_hash: u64,
        _limits: SearchLimits,
    ) -> Result<Event<B>, PlayerError> {
        if !matches!(self.state.inner, InnerState::Idle | InnerState::Syncing) {
            return Err(PlayerError::ProtocolError(format!(
                "Go in state {:?} (player={:?}, turn={})",
                self.state.inner, self.player_slot, self.game.turn
            )));
        }
        self.state.inner = InnerState::Thinking;
        self.core.stopped.store(false, Ordering::Relaxed);
        let (turn, direction, think_ms) = self.run_think(state_hash).await?;
        if self
            .core
            .bot_tx
            .send(BotMsg::Action {
                direction,
                player: self.player_slot,
                turn,
                state_hash,
                think_ms,
            })
            .is_err()
        {
            return Ok(Event::CleanClose);
        }
        self.state.inner = InnerState::Idle;
        Ok(Event::Continue(self))
    }

    /// Handle GoState: overwrite local state with the provided turn state,
    /// verify the host-claimed hash against the rebuilt state, then run Go
    /// as usual.
    async fn handle_go_state(
        mut self,
        turn_state: OwnedTurnState,
        state_hash: u64,
        limits: SearchLimits,
    ) -> Result<Event<B>, PlayerError> {
        // Rebuild the engine mirror to match the injected state.
        self.game = rebuild_engine_state(&self.match_config, &turn_state)?;
        self.last_moves = (turn_state.player1_last_move, turn_state.player2_last_move);
        let local_hash = hash_from_game(&self.game, self.last_moves.0, self.last_moves.1);
        if local_hash != state_hash {
            return Err(PlayerError::ProtocolError(format!(
                "GoState hash mismatch: local={local_hash}, host={state_hash} \
                 (player={:?}, turn={})",
                self.player_slot, self.game.turn
            )));
        }
        self.handle_go(state_hash, limits).await
    }

    /// Handle Advance: apply moves locally, verify turn and hash.
    fn handle_advance(
        mut self,
        p1_dir: Direction,
        p2_dir: Direction,
        turn: u16,
        new_hash: u64,
    ) -> Result<Event<B>, PlayerError> {
        if !matches!(self.state.inner, InnerState::Idle) {
            return Err(PlayerError::ProtocolError(format!(
                "Advance in state {:?} (player={:?}, turn={})",
                self.state.inner, self.player_slot, self.game.turn
            )));
        }
        // Apply the two moves. The returned undo token is discarded: on
        // desync we defer to the server's FullState rather than rolling back.
        let _undo = self.game.make_move(p1_dir, p2_dir);
        self.last_moves = (p1_dir, p2_dir);
        if turn != self.game.turn {
            return Err(PlayerError::ProtocolError(format!(
                "Advance turn mismatch: msg={turn}, local={} (player={:?})",
                self.game.turn, self.player_slot
            )));
        }
        let local_hash = hash_from_game(&self.game, p1_dir, p2_dir);

        if local_hash == new_hash {
            if self
                .core
                .bot_tx
                .send(BotMsg::SyncOk { hash: local_hash })
                .is_err()
            {
                return Ok(Event::CleanClose);
            }
            self.state.inner = InnerState::Syncing;
            Ok(Event::Continue(self))
        } else {
            if self
                .core
                .bot_tx
                .send(BotMsg::Resync {
                    my_hash: local_hash,
                })
                .is_err()
            {
                return Ok(Event::CleanClose);
            }
            Ok(Event::Desynced(self.with_state(Desynced)))
        }
    }

    /// Run `bot.preprocess` while selecting on `host_rx` for concurrent Stop
    /// messages. Sideband forwarding goes through `EmbeddedCtx`.
    async fn run_preprocess(&mut self, state_hash: u64) -> Result<(), PlayerError> {
        let start = Instant::now();
        let deadline = deadline_from_ms(start, self.match_config.preprocessing_timeout_ms);
        let (hts, ctx) = self.prepare_ctx(state_hash, start, deadline);
        let turn = hts.turn;
        let mut bot = self.bot.take().expect("bot present between turns");
        let mut handle = tokio::task::spawn_blocking(move || {
            bot.preprocess(&hts, &ctx);
            bot
        });
        let returned_bot = self.watch_for_stop(&mut handle, turn).await?;
        self.bot = Some(returned_bot);
        Ok(())
    }

    /// Run `bot.think` while selecting on `host_rx` for concurrent Stop.
    /// Returns the turn number, chosen direction, and recorded think time.
    async fn run_think(&mut self, state_hash: u64) -> Result<(u16, Direction, u32), PlayerError> {
        let start = Instant::now();
        let deadline = deadline_from_ms(start, self.match_config.move_timeout_ms);
        let (hts, ctx) = self.prepare_ctx(state_hash, start, deadline);
        let turn = hts.turn;
        let mut bot = self.bot.take().expect("bot present between turns");
        let mut handle = tokio::task::spawn_blocking(move || {
            let dir = bot.think(&hts, &ctx);
            (bot, dir)
        });
        let (returned_bot, direction) = self.watch_for_stop(&mut handle, turn).await?;
        self.bot = Some(returned_bot);
        // Clamp to 1: the host rejects think_ms == 0 (indistinguishable from missing field).
        let think_ms = (start.elapsed().as_millis() as u32).max(1);
        Ok((turn, direction, think_ms))
    }

    /// Await a spawn_blocking bot task while draining `host_rx` for Stop or
    /// protocol faults. If Stop arrives, flips the `stopped` flag and keeps
    /// waiting. Any other in-flight message is a protocol error.
    async fn watch_for_stop<T>(
        &mut self,
        handle: &mut JoinHandle<T>,
        turn: u16,
    ) -> Result<T, PlayerError> {
        let player_slot = self.player_slot;
        loop {
            tokio::select! {
                biased;
                result = &mut *handle => {
                    return result.map_err(|e| PlayerError::TransportError(
                        format!("bot task panicked (player={player_slot:?}, turn={turn}): {e}"),
                    ));
                }
                msg = self.core.host_rx.recv() => {
                    match msg {
                        Some(HostMsg::Stop) => {
                            self.core.stopped.store(true, Ordering::Relaxed);
                        }
                        Some(other) => {
                            return Err(PlayerError::ProtocolError(format!(
                                "unexpected {other:?} while bot is working \
                                 (player={player_slot:?}, turn={turn})"
                            )));
                        }
                        None => {
                            // Defense in depth: signal the bot cooperatively
                            // so it gets a chance to exit before this
                            // dispatcher is reaped. `close` already sets this,
                            // but other paths that drop host_tx don't.
                            self.core.stopped.store(true, Ordering::Relaxed);
                            return Err(PlayerError::TransportError(format!(
                                "host_rx closed while bot is working \
                                 (player={player_slot:?}, turn={turn})"
                            )));
                        }
                    }
                }
            }
        }
    }

    fn prepare_ctx(
        &self,
        state_hash: u64,
        think_start: Instant,
        deadline: Option<Instant>,
    ) -> (HashedTurnState, EmbeddedCtx) {
        let hts = HashedTurnState::new(owned_turn_state_from_game(&self.game, self.last_moves));
        let ctx = EmbeddedCtx {
            event_sink: self.core.event_sink.clone(),
            bot_tx: self.core.bot_tx.clone(),
            turn: self.game.turn,
            state_hash,
            player: self.player_slot,
            stopped: self.core.stopped.clone(),
            think_start,
            deadline,
        };
        (hts, ctx)
    }
}

/// Derive a deadline instant from a `_timeout_ms` field. A zero timeout
/// means "no wall-clock deadline"; bots cooperate on `HostMsg::Stop` only.
fn deadline_from_ms(start: Instant, timeout_ms: u32) -> Option<Instant> {
    if timeout_ms == 0 {
        None
    } else {
        Some(start + Duration::from_millis(u64::from(timeout_ms)))
    }
}

impl<B: EmbeddedBot> Playing<B, Desynced> {
    /// Await `HostMsg::FullState`, rebuild the local mirror, emit `SyncOk`,
    /// return to `Playing<Synced>`.
    ///
    /// Returns `Ok(None)` if the player handle was dropped before recovery
    /// completed (clean local close).
    async fn recover(mut self) -> Result<Option<Playing<B, Synced>>, PlayerError> {
        let (match_config, turn_state) = match self.core.host_rx.recv().await {
            Some(HostMsg::FullState {
                match_config,
                turn_state,
            }) => (match_config, *turn_state),
            Some(HostMsg::ProtocolError { reason }) => {
                return Err(PlayerError::ProtocolError(reason));
            },
            Some(other) => {
                return Err(PlayerError::ProtocolError(format!(
                    "expected FullState while Desynced, got {other:?}"
                )));
            },
            None => return Ok(None),
        };

        self.match_config = match_config;
        self.game = rebuild_engine_state(&self.match_config, &turn_state)?;
        self.last_moves = (turn_state.player1_last_move, turn_state.player2_last_move);
        let hash = hash_from_game(&self.game, self.last_moves.0, self.last_moves.1);
        if self.core.bot_tx.send(BotMsg::SyncOk { hash }).is_err() {
            return Ok(None);
        }

        Ok(Some(self.with_state(Synced {
            inner: InnerState::Idle,
        })))
    }
}

// ── State mirror helpers ──────────────────────────────

/// Construct an engine `GameState` from an `OwnedMatchConfig`.
fn build_engine_state(cfg: &OwnedMatchConfig) -> Result<GameState, PlayerError> {
    let mut walls: HashMap<Coordinates, Vec<Coordinates>> = HashMap::new();
    for (a, b) in &cfg.walls {
        walls.entry(*a).or_default().push(*b);
        walls.entry(*b).or_default().push(*a);
    }
    let mut mud = MudMap::new();
    for entry in &cfg.mud {
        mud.insert(entry.pos1, entry.pos2, entry.turns);
    }

    GameBuilder::new(cfg.width, cfg.height)
        .with_max_turns(cfg.max_turns)
        .with_custom_maze(walls, mud)
        .with_custom_positions(cfg.player1_start, cfg.player2_start)
        .with_custom_cheese(cfg.cheese.clone())
        .build()
        .create(None)
        .map_err(|e| PlayerError::ProtocolError(format!("invalid match config: {e}")))
}

/// Rebuild the engine state to match an injected turn state (GoState or
/// FullState recovery).
fn rebuild_engine_state(
    cfg: &OwnedMatchConfig,
    ts: &OwnedTurnState,
) -> Result<GameState, PlayerError> {
    let mut game = build_engine_state(cfg)?;
    game.turn = ts.turn;
    game.player1.current_pos = ts.player1_position;
    game.player2.current_pos = ts.player2_position;
    game.player1.score = ts.player1_score;
    game.player2.score = ts.player2_score;
    game.player1.mud_timer = ts.player1_mud_turns;
    game.player2.mud_timer = ts.player2_mud_turns;
    // Cheese: replace whatever the builder placed with the set in `ts`.
    // Simplest path: clear and re-place. `CheeseBoard::place_cheese` is
    // idempotent; `clear` / per-cell remove is not exposed, so rebuild.
    for pos in cfg.cheese.iter() {
        if !ts.cheese.contains(pos) {
            game.cheese.take_cheese(*pos);
        }
    }
    Ok(game)
}

/// Hash a `GameState` via the protocol-canonical path: derive
/// `OwnedTurnState`, wrap in `HashedTurnState::new()` (DefaultHasher). This
/// is the same hashing used for Ready; `Advance` must produce a compatible
/// hash for desync detection to work.
fn hash_from_game(game: &GameState, last_p1: Direction, last_p2: Direction) -> u64 {
    HashedTurnState::new(owned_turn_state_from_game(game, (last_p1, last_p2))).state_hash()
}

fn owned_turn_state_from_game(
    game: &GameState,
    last_moves: (Direction, Direction),
) -> OwnedTurnState {
    OwnedTurnState {
        turn: game.turn,
        player1_position: game.player1.current_pos,
        player2_position: game.player2.current_pos,
        player1_score: game.player1.score,
        player2_score: game.player2.score,
        player1_mud_turns: game.player1.mud_timer,
        player2_mud_turns: game.player2.mud_timer,
        cheese: game.cheese_positions(),
        player1_last_move: last_moves.0,
        player2_last_move: last_moves.1,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pyrat_wire::TimingMode;

    struct DummyBot;
    impl Options for DummyBot {}
    impl EmbeddedBot for DummyBot {
        fn think(&mut self, _: &HashedTurnState, _: &EmbeddedCtx) -> Direction {
            Direction::Stay
        }
    }

    fn sample_match_config() -> Box<pyrat_protocol::OwnedMatchConfig> {
        Box::new(pyrat_protocol::OwnedMatchConfig {
            width: 5,
            height: 5,
            max_turns: 100,
            walls: vec![],
            mud: vec![],
            cheese: vec![Coordinates::new(2, 2)],
            player1_start: Coordinates::new(0, 0),
            player2_start: Coordinates::new(4, 4),
            controlled_players: vec![PlayerSlot::Player1],
            timing: TimingMode::Wait,
            move_timeout_ms: 100,
            preprocessing_timeout_ms: 1000,
        })
    }

    #[tokio::test]
    async fn setup_flow_emits_identify_then_ready() {
        let identity = PlayerIdentity {
            name: "Dummy".into(),
            author: "test".into(),
            agent_id: "pyrat/dummy".into(),
        };
        let mut player = EmbeddedPlayer::new(DummyBot, identity, EventSink::noop());

        // Dispatcher's first emission is Identify, without any host input.
        let identify = player.recv().await.unwrap().unwrap();
        match identify {
            BotMsg::Identify {
                name,
                author,
                agent_id,
                ..
            } => {
                assert_eq!(name, "Dummy");
                assert_eq!(author, "test");
                assert_eq!(agent_id, "pyrat/dummy");
            },
            other => panic!("expected Identify, got {other:?}"),
        }

        // Welcome assigns a slot (dispatcher stores it silently).
        player
            .send(HostMsg::Welcome {
                player_slot: PlayerSlot::Player1,
            })
            .await
            .unwrap();

        // Configure triggers the Ready emission with a hash over initial state.
        let match_config = sample_match_config();
        let expected_hash = hash_from_game(
            &build_engine_state(&match_config).unwrap(),
            Direction::Stay,
            Direction::Stay,
        );
        player
            .send(HostMsg::Configure {
                options: vec![],
                match_config,
            })
            .await
            .unwrap();

        match player.recv().await.unwrap().unwrap() {
            BotMsg::Ready { state_hash } => assert_eq!(state_hash, expected_hash),
            other => panic!("expected Ready, got {other:?}"),
        }

        // Setup is done; dispatcher is now in Playing<Synced>, awaiting a
        // host message. Dropping the player closes host_tx; the dispatcher
        // sees recv() yield None from Idle and exits cleanly.
        drop(player);
    }

    #[tokio::test]
    async fn unexpected_message_during_setup_is_a_protocol_error() {
        let identity = PlayerIdentity {
            name: "Dummy".into(),
            author: "test".into(),
            agent_id: "pyrat/dummy".into(),
        };
        let mut player = EmbeddedPlayer::new(DummyBot, identity, EventSink::noop());

        // Drain the bot's Identify.
        let _ = player.recv().await.unwrap().unwrap();

        // Send a Go instead of Welcome: protocol violation.
        player
            .send(HostMsg::Go {
                state_hash: 0,
                limits: pyrat_protocol::SearchLimits::default(),
            })
            .await
            .unwrap();

        // recv() drains bot_tx (empty) then reaps the dispatcher and surfaces
        // its error verbatim.
        let err = player.recv().await.unwrap_err();
        match err {
            PlayerError::ProtocolError(msg) => {
                assert!(msg.contains("expected Welcome"), "msg={msg}");
            },
            other => panic!("expected ProtocolError, got {other:?}"),
        }
    }

    /// Verifies the Advance + SyncOk happy path. Lives here (not in the
    /// integration tests) because computing the expected post-move hash
    /// needs the crate-private `build_engine_state` / `hash_from_game`
    /// helpers; exposing them publicly would leak implementation details.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn advance_with_correct_hash_emits_syncok() {
        let identity = PlayerIdentity {
            name: "Dummy".into(),
            author: "test".into(),
            agent_id: "pyrat/dummy".into(),
        };
        let mut player = EmbeddedPlayer::new(DummyBot, identity, EventSink::noop());

        // Setup.
        let _ = player.recv().await.unwrap().unwrap(); // Identify
        player
            .send(HostMsg::Welcome {
                player_slot: PlayerSlot::Player1,
            })
            .await
            .unwrap();
        let match_config = sample_match_config();
        player
            .send(HostMsg::Configure {
                options: vec![],
                match_config: match_config.clone(),
            })
            .await
            .unwrap();
        let hash0 = match player.recv().await.unwrap().unwrap() {
            BotMsg::Ready { state_hash } => state_hash,
            _ => panic!("expected Ready"),
        };

        // First turn: Go (from Configured/Idle) → Action. Bot plays Stay.
        player
            .send(HostMsg::Go {
                state_hash: hash0,
                limits: SearchLimits::default(),
            })
            .await
            .unwrap();
        match player.recv().await.unwrap().unwrap() {
            BotMsg::Action { .. } => {},
            other => panic!("expected Action, got {other:?}"),
        }

        // Compute the hash the dispatcher would arrive at after applying
        // (Stay, Stay) to the initial state, using the same helpers the dispatcher
        // uses internally.
        let mut game = build_engine_state(&match_config).unwrap();
        let _ = game.make_move(Direction::Stay, Direction::Stay);
        let hash_after = hash_from_game(&game, Direction::Stay, Direction::Stay);

        player
            .send(HostMsg::Advance {
                p1_dir: Direction::Stay,
                p2_dir: Direction::Stay,
                turn: 1,
                new_hash: hash_after,
            })
            .await
            .unwrap();

        match player.recv().await.unwrap().unwrap() {
            BotMsg::SyncOk { hash } => assert_eq!(hash, hash_after),
            other => panic!("expected SyncOk, got {other:?}"),
        }

        // Clean shutdown.
        player
            .send(HostMsg::GameOver {
                result: GameResult::Draw,
                player1_score: 0.0,
                player2_score: 0.0,
            })
            .await
            .unwrap();
        let _ = player.close().await;
    }
}
