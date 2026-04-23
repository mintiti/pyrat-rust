//! In-process [`Player`](super::Player) implementation.
//!
//! `EmbeddedPlayer` runs a bot in the same process as the Match. No TCP, no
//! FlatBuffers, no subprocess — a dispatcher task translates [`HostMsg`] into
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

use pyrat::{Coordinates, Direction, GameBuilder, GameState, MudMap};
use pyrat_protocol::{
    BotMsg, HashedTurnState, HostMsg, OwnedMatchConfig, OwnedOptionDef, OwnedTurnState,
    SearchLimits,
};
use pyrat_wire::{GameResult, Player as PlayerSlot};
use tokio::sync::mpsc;
use tokio::task::JoinHandle;

use super::{EventSink, Player, PlayerError, PlayerIdentity};

// ── Bot-author-facing surface ──────────────────────────

/// Bot-declared option application.
///
/// Shape mirrors the SDK `pyrat_sdk::Options` trait verbatim, re-declared
/// here to avoid a `pyrat-host → pyrat-sdk` dependency edge. If drift between
/// the two becomes a problem, the open plan item is to extract a shared
/// `pyrat-bot-api` crate.
pub trait Options {
    /// Declare configurable options.
    fn option_defs(&self) -> Vec<OwnedOptionDef> {
        vec![]
    }

    /// Apply a named option value. Called once per entry in
    /// [`HostMsg::Configure`]'s `options` vector.
    fn apply_option(&mut self, _name: &str, _value: &str) -> Result<(), String> {
        Err("unknown option".into())
    }
}

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

/// Parameters for sending an Info message.
///
/// Shape mirrors `pyrat_sdk::InfoParams`. Use [`InfoParams::for_player`]
/// and override fields with struct update syntax.
#[derive(Debug)]
pub struct InfoParams<'a> {
    pub player: PlayerSlot,
    pub multipv: u16,
    pub target: Option<Coordinates>,
    pub depth: u16,
    pub nodes: u32,
    pub score: Option<f32>,
    pub pv: &'a [Direction],
    pub message: &'a str,
}

impl InfoParams<'_> {
    /// Defaults for the given player slot.
    pub fn for_player(player: PlayerSlot) -> Self {
        Self {
            player,
            multipv: 0,
            target: None,
            depth: 0,
            nodes: 0,
            score: None,
            pv: &[],
            message: "",
        }
    }
}

/// Per-turn context passed to [`EmbeddedBot::think`] and
/// [`EmbeddedBot::preprocess`].
///
/// Shape mirrors the SDK `Context` API surface. `should_stop` reads an atomic
/// that the dispatcher flips on [`HostMsg::Stop`]. Sideband routing:
/// - `send_info` → observer-facing, forwarded to [`EventSink`] as
///   `MatchEvent::BotInfo`.
/// - `send_provisional` → game-driving, forwarded to the Match's `recv()`
///   queue as [`BotMsg::Provisional`] (Match uses the latest as timeout
///   fallback).
pub struct EmbeddedCtx {
    event_sink: EventSink,
    bot_tx: mpsc::UnboundedSender<BotMsg>,
    turn: u16,
    state_hash: u64,
    player: PlayerSlot,
    stopped: Arc<AtomicBool>,
}

impl EmbeddedCtx {
    pub(crate) fn new(
        event_sink: EventSink,
        bot_tx: mpsc::UnboundedSender<BotMsg>,
        turn: u16,
        state_hash: u64,
        player: PlayerSlot,
        stopped: Arc<AtomicBool>,
    ) -> Self {
        Self {
            event_sink,
            bot_tx,
            turn,
            state_hash,
            player,
            stopped,
        }
    }

    /// True if the host has signalled Stop. Cooperatively polled from the
    /// bot's think loop.
    pub fn should_stop(&self) -> bool {
        self.stopped.load(Ordering::Relaxed)
    }

    /// Send an Info message. Routed to the attached [`EventSink`] as a
    /// `MatchEvent::BotInfo` (observer-facing, never inspected by Match).
    pub fn send_info(&self, params: &InfoParams<'_>) {
        let info = pyrat_protocol::OwnedInfo {
            player: params.player,
            multipv: params.multipv,
            target: params.target,
            depth: params.depth,
            nodes: params.nodes,
            score: params.score,
            pv: params.pv.to_vec(),
            message: params.message.to_string(),
            turn: self.turn,
            state_hash: self.state_hash,
        };
        self.event_sink.emit(crate::game_loop::MatchEvent::BotInfo {
            sender: self.player,
            turn: self.turn,
            state_hash: self.state_hash,
            info,
        });
    }

    /// Send a provisional (best-so-far) direction. Emitted as
    /// [`BotMsg::Provisional`] on the game-driving channel: Match holds the
    /// latest as its timeout fallback.
    pub fn send_provisional(&self, direction: Direction) {
        let _ = self.bot_tx.send(BotMsg::Provisional {
            direction,
            player: self.player,
            turn: self.turn,
            state_hash: self.state_hash,
        });
    }
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
}

impl EmbeddedPlayer {
    /// Construct an EmbeddedPlayer wrapping `bot`. Spawns a dispatcher task
    /// on the current tokio runtime.
    pub fn new<B: EmbeddedBot>(bot: B, identity: PlayerIdentity, event_sink: EventSink) -> Self {
        let (host_tx, host_rx) = mpsc::unbounded_channel();
        let (bot_tx, bot_rx) = mpsc::unbounded_channel();
        let dispatcher = tokio::spawn(dispatcher_task(
            bot,
            identity.clone(),
            event_sink,
            host_rx,
            bot_tx,
        ));
        Self {
            identity,
            host_tx,
            bot_rx,
            dispatcher: Some(dispatcher),
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
            mut bot_rx,
            dispatcher,
            identity: _,
        } = self;
        // Signal the dispatcher by dropping host_tx; drain any pending BotMsgs.
        drop(host_tx);
        while bot_rx.recv().await.is_some() {}
        reap(dispatcher).await
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
) -> Result<(), PlayerError> {
    let core = DispatcherCore {
        event_sink,
        host_rx,
        bot_tx,
        stopped: Arc::new(AtomicBool::new(false)),
    };

    let initial = Setup::<B, Initial>::new(bot, core);
    let connected = initial.emit_identify(&identity)?;
    let identified = connected.await_welcome().await?;
    let mut playing = identified.await_configure_emit_ready().await?;

    loop {
        match playing.next_event().await? {
            Event::Continue(p) => playing = p,
            Event::Desynced(d) => playing = d.recover().await?,
            Event::GameOver => return Ok(()),
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
    ///
    /// Synchronous: `UnboundedSender::send` never awaits.
    fn emit_identify(self, identity: &PlayerIdentity) -> Result<Setup<B, Connected>, PlayerError> {
        let options = self.bot.option_defs();
        self.core
            .bot_tx
            .send(BotMsg::Identify {
                name: identity.name.clone(),
                author: identity.author.clone(),
                agent_id: identity.agent_id.clone(),
                options,
            })
            .map_err(|_| PlayerError::TransportError("bot_tx closed".into()))?;
        Ok(Setup {
            bot: self.bot,
            core: self.core,
            state: Connected,
        })
    }
}

impl<B: EmbeddedBot> Setup<B, Connected> {
    /// Await `HostMsg::Welcome`, store the assigned player slot.
    async fn await_welcome(mut self) -> Result<Setup<B, Identified>, PlayerError> {
        match self.core.host_rx.recv().await {
            Some(HostMsg::Welcome { player_slot }) => Ok(Setup {
                bot: self.bot,
                core: self.core,
                state: Identified { player_slot },
            }),
            Some(HostMsg::ProtocolError { reason }) => Err(PlayerError::ProtocolError(reason)),
            Some(other) => Err(PlayerError::ProtocolError(format!(
                "expected Welcome, got {other:?}"
            ))),
            None => Err(PlayerError::TransportError("host_rx closed".into())),
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
    async fn await_configure_emit_ready(mut self) -> Result<Playing<B, Synced>, PlayerError> {
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
            None => return Err(PlayerError::TransportError("host_rx closed".into())),
        };

        for (name, value) in &options {
            if let Err(err) = self.bot.apply_option(name, value) {
                tracing::warn!(option = %name, error = %err, "bot rejected option");
            }
        }

        let game = build_engine_state(&match_config)?;
        let hash = hash_from_game(&game, Direction::Stay, Direction::Stay);

        self.core
            .bot_tx
            .send(BotMsg::Ready { state_hash: hash })
            .map_err(|_| PlayerError::TransportError("bot_tx closed".into()))?;

        Ok(Playing {
            bot: Some(self.bot),
            core: self.core,
            player_slot: self.state.player_slot,
            match_config,
            game,
            last_moves: (Direction::Stay, Direction::Stay),
            inner: InnerState::Idle,
            _sync: std::marker::PhantomData,
        })
    }
}

// ── Playing<Sync> typestate ───────────────────────────

/// Sync status markers. `Playing<Synced>` exposes the turn-handling methods;
/// `Playing<Desynced>` exposes only `recover`.
struct Synced;
struct Desynced;

/// Inner state machine inside `Playing<Synced>`. Tracked as a runtime enum —
/// typestate in a message-driven loop collapses back to this shape anyway.
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

struct Playing<B, Sync> {
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
    inner: InnerState,
    _sync: std::marker::PhantomData<Sync>,
}

/// Return value of one iteration of `Playing<Synced>::next_event`.
enum Event<B> {
    /// Still Synced; loop continues.
    Continue(Playing<B, Synced>),
    /// Hash mismatch observed; await FullState before doing anything else.
    Desynced(Playing<B, Desynced>),
    /// `HostMsg::GameOver` received and processed; dispatcher exits.
    GameOver,
}

impl<B: EmbeddedBot> Playing<B, Synced> {
    /// Advance the dispatcher by one host message.
    ///
    /// Takes `self` and returns one of:
    /// - `Event::Continue` — still Synced, loop with the returned value.
    /// - `Event::Desynced` — hash mismatch detected, Resync emitted.
    /// - `Event::GameOver` — GameOver processed, dispatcher exits.
    async fn next_event(mut self) -> Result<Event<B>, PlayerError> {
        let msg = self
            .core
            .host_rx
            .recv()
            .await
            .ok_or_else(|| PlayerError::TransportError("host_rx closed".into()))?;

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
            HostMsg::FullState { .. } => Err(PlayerError::ProtocolError(
                "FullState received while Synced — server sent without Resync".into(),
            )),
            HostMsg::Welcome { .. } | HostMsg::Configure { .. } => Err(PlayerError::ProtocolError(
                format!("setup message in Playing: {msg:?}"),
            )),
            HostMsg::ProtocolError { reason } => Err(PlayerError::ProtocolError(reason)),
        }
    }

    /// Handle GoPreprocess: run bot.preprocess, emit PreprocessingDone.
    async fn handle_go_preprocess(mut self, state_hash: u64) -> Result<Event<B>, PlayerError> {
        if !matches!(self.inner, InnerState::Idle) {
            return Err(PlayerError::ProtocolError(format!(
                "GoPreprocess in state {:?}",
                self.inner
            )));
        }
        self.inner = InnerState::Preprocessing;
        self.core.stopped.store(false, Ordering::Relaxed);
        self.run_preprocess(state_hash).await?;
        self.core
            .bot_tx
            .send(BotMsg::PreprocessingDone)
            .map_err(|_| PlayerError::TransportError("bot_tx closed".into()))?;
        self.inner = InnerState::Idle;
        Ok(Event::Continue(self))
    }

    /// Handle Go: run bot.think, emit Action.
    async fn handle_go(
        mut self,
        state_hash: u64,
        _limits: SearchLimits,
    ) -> Result<Event<B>, PlayerError> {
        if !matches!(self.inner, InnerState::Idle | InnerState::Syncing) {
            return Err(PlayerError::ProtocolError(format!(
                "Go in state {:?}",
                self.inner
            )));
        }
        self.inner = InnerState::Thinking;
        self.core.stopped.store(false, Ordering::Relaxed);
        let (turn, direction, think_ms) = self.run_think(state_hash).await?;
        self.core
            .bot_tx
            .send(BotMsg::Action {
                direction,
                player: self.player_slot,
                turn,
                state_hash,
                think_ms,
            })
            .map_err(|_| PlayerError::TransportError("bot_tx closed".into()))?;
        self.inner = InnerState::Idle;
        Ok(Event::Continue(self))
    }

    /// Handle GoState: overwrite local state with the provided turn state,
    /// then run Go as usual.
    async fn handle_go_state(
        mut self,
        turn_state: OwnedTurnState,
        state_hash: u64,
        limits: SearchLimits,
    ) -> Result<Event<B>, PlayerError> {
        // Rebuild the engine mirror to match the injected state.
        self.game = rebuild_engine_state(&self.match_config, &turn_state)?;
        self.last_moves = (turn_state.player1_last_move, turn_state.player2_last_move);
        self.handle_go(state_hash, limits).await
    }

    /// Handle Advance: apply moves locally, verify hash.
    fn handle_advance(
        mut self,
        p1_dir: Direction,
        p2_dir: Direction,
        _turn: u16,
        new_hash: u64,
    ) -> Result<Event<B>, PlayerError> {
        if !matches!(self.inner, InnerState::Idle) {
            return Err(PlayerError::ProtocolError(format!(
                "Advance in state {:?}",
                self.inner
            )));
        }
        // Apply the two moves. The returned undo token is discarded — on
        // desync we defer to the server's FullState rather than rolling back.
        let _undo = self.game.make_move(p1_dir, p2_dir);
        self.last_moves = (p1_dir, p2_dir);
        let local_hash = hash_from_game(&self.game, p1_dir, p2_dir);

        if local_hash == new_hash {
            self.core
                .bot_tx
                .send(BotMsg::SyncOk { hash: local_hash })
                .map_err(|_| PlayerError::TransportError("bot_tx closed".into()))?;
            self.inner = InnerState::Syncing;
            Ok(Event::Continue(self))
        } else {
            self.core
                .bot_tx
                .send(BotMsg::Resync {
                    my_hash: local_hash,
                })
                .map_err(|_| PlayerError::TransportError("bot_tx closed".into()))?;
            Ok(Event::Desynced(Playing {
                bot: self.bot,
                core: self.core,
                player_slot: self.player_slot,
                match_config: self.match_config,
                game: self.game,
                last_moves: self.last_moves,
                inner: InnerState::Idle, // not meaningful while desynced
                _sync: std::marker::PhantomData,
            }))
        }
    }

    /// Run `bot.preprocess` while selecting on `host_rx` for concurrent Stop
    /// messages. Sideband forwarding goes through `EmbeddedCtx`.
    async fn run_preprocess(&mut self, state_hash: u64) -> Result<(), PlayerError> {
        let (hts, ctx) = self.prepare_ctx(state_hash);
        let mut bot = self.bot.take().expect("bot present between turns");
        let mut handle = tokio::task::spawn_blocking(move || {
            bot.preprocess(&hts, &ctx);
            bot
        });
        let stopped = self.core.stopped.clone();
        let returned_bot = watch_for_stop(&mut self.core.host_rx, &stopped, &mut handle).await?;
        self.bot = Some(returned_bot);
        Ok(())
    }

    /// Run `bot.think` while selecting on `host_rx` for concurrent Stop.
    /// Returns the turn number, chosen direction, and recorded think time.
    async fn run_think(&mut self, state_hash: u64) -> Result<(u16, Direction, u32), PlayerError> {
        let (hts, ctx) = self.prepare_ctx(state_hash);
        let turn = hts.turn;
        let start = std::time::Instant::now();
        let mut bot = self.bot.take().expect("bot present between turns");
        let mut handle = tokio::task::spawn_blocking(move || {
            let dir = bot.think(&hts, &ctx);
            (bot, dir)
        });
        let stopped = self.core.stopped.clone();
        let (returned_bot, direction) =
            watch_for_stop(&mut self.core.host_rx, &stopped, &mut handle).await?;
        self.bot = Some(returned_bot);
        let think_ms = start.elapsed().as_millis() as u32;
        Ok((turn, direction, think_ms))
    }

    fn prepare_ctx(&self, state_hash: u64) -> (HashedTurnState, EmbeddedCtx) {
        let hts = HashedTurnState::new(owned_turn_state_from_game(&self.game, self.last_moves));
        let ctx = EmbeddedCtx::new(
            self.core.event_sink.clone(),
            self.core.bot_tx.clone(),
            self.game.turn,
            state_hash,
            self.player_slot,
            self.core.stopped.clone(),
        );
        (hts, ctx)
    }
}

impl<B: EmbeddedBot> Playing<B, Desynced> {
    /// Await `HostMsg::FullState`, rebuild the local mirror, emit `SyncOk`,
    /// return to `Playing<Synced>`.
    async fn recover(mut self) -> Result<Playing<B, Synced>, PlayerError> {
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
            None => return Err(PlayerError::TransportError("host_rx closed".into())),
        };

        self.match_config = match_config;
        self.game = rebuild_engine_state(&self.match_config, &turn_state)?;
        self.last_moves = (turn_state.player1_last_move, turn_state.player2_last_move);
        let hash = hash_from_game(&self.game, self.last_moves.0, self.last_moves.1);
        self.core
            .bot_tx
            .send(BotMsg::SyncOk { hash })
            .map_err(|_| PlayerError::TransportError("bot_tx closed".into()))?;

        Ok(Playing {
            bot: self.bot,
            core: self.core,
            player_slot: self.player_slot,
            match_config: self.match_config,
            game: self.game,
            last_moves: self.last_moves,
            inner: InnerState::Idle,
            _sync: std::marker::PhantomData,
        })
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

/// Extract an `OwnedTurnState` from an engine `GameState`.
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

/// Await a blocking bot task while watching `host_rx` for Stop messages.
///
/// - If the blocking task completes first, return its value.
/// - If Stop arrives, flip `stopped` and keep waiting.
/// - Any other message during bot execution is a protocol error.
async fn watch_for_stop<T>(
    host_rx: &mut mpsc::UnboundedReceiver<HostMsg>,
    stopped: &Arc<AtomicBool>,
    handle: &mut JoinHandle<T>,
) -> Result<T, PlayerError> {
    loop {
        tokio::select! {
            biased;
            result = &mut *handle => {
                return result.map_err(|e| PlayerError::TransportError(
                    format!("bot task panicked: {e}"),
                ));
            }
            msg = host_rx.recv() => {
                match msg {
                    Some(HostMsg::Stop) => {
                        stopped.store(true, Ordering::Relaxed);
                    }
                    Some(other) => {
                        return Err(PlayerError::ProtocolError(format!(
                            "unexpected {other:?} while bot is working"
                        )));
                    }
                    None => {
                        return Err(PlayerError::TransportError(
                            "host_rx closed while bot is working".into(),
                        ));
                    }
                }
            }
        }
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
        // host message. Close the player; dispatcher sees host_tx drop and
        // exits with a TransportError ("host_rx closed"). That's the
        // expected clean-close semantics for a synced player with no pending
        // GameOver.
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
        assert!(matches!(err, PlayerError::ProtocolError(_)), "{err:?}");
    }
}
