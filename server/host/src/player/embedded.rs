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

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use pyrat::{Coordinates, Direction};
use pyrat_protocol::{BotMsg, HashedTurnState, HostMsg, OwnedOptionDef};
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
    #[expect(dead_code, reason = "consumed by dispatcher in follow-up commit")]
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
        let dispatcher = tokio::spawn(dispatcher_task(bot, event_sink, host_rx, bot_tx));
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

// ── Dispatcher (scaffold) ─────────────────────────────

/// Run one bot to completion, translating between the pipe and bot methods.
///
/// **Status: scaffold.** This initial commit wires the channels and ensures
/// the type surface compiles. The actual state machine (setup typestate,
/// `Playing<Sync>`, inner step loop) lands in follow-up commits.
async fn dispatcher_task<B: EmbeddedBot>(
    _bot: B,
    _event_sink: EventSink,
    mut host_rx: mpsc::UnboundedReceiver<HostMsg>,
    _bot_tx: mpsc::UnboundedSender<BotMsg>,
) -> Result<(), PlayerError> {
    // Drain host_rx until the sender is dropped. No BotMsgs are produced.
    while host_rx.recv().await.is_some() {}
    Ok(())
}
