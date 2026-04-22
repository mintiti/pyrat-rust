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
/// Lifecycle: setup typestate chain (Initial → Connected → Identified →
/// Configured → Playing) then the playing loop. The setup chain is fully
/// wired in this commit. Playing currently exits immediately after Ready;
/// the turn loop lands in a follow-up.
async fn dispatcher_task<B: EmbeddedBot>(
    bot: B,
    identity: PlayerIdentity,
    event_sink: EventSink,
    host_rx: mpsc::UnboundedReceiver<HostMsg>,
    bot_tx: mpsc::UnboundedSender<BotMsg>,
) -> Result<(), PlayerError> {
    let stopped = Arc::new(AtomicBool::new(false));
    let core = DispatcherCore {
        bot,
        event_sink,
        host_rx,
        bot_tx,
        stopped,
    };

    let initial = Setup::<B, Initial>::new(core);
    let connected = initial.emit_identify(&identity)?;
    let identified = connected.await_welcome().await?;
    let _playing = identified.await_configure_emit_ready().await?;
    // TODO: playing loop lands in the next commit.
    Ok(())
}

/// Data shared across every dispatcher state. Re-typed but not rebuilt across
/// transitions: the bot, event sink, channels, and stop flag persist for the
/// lifetime of the dispatcher.
struct DispatcherCore<B> {
    bot: B,
    #[expect(
        dead_code,
        reason = "consumed by the playing loop in a follow-up commit"
    )]
    event_sink: EventSink,
    host_rx: mpsc::UnboundedReceiver<HostMsg>,
    bot_tx: mpsc::UnboundedSender<BotMsg>,
    #[expect(
        dead_code,
        reason = "consumed by the playing loop in a follow-up commit"
    )]
    stopped: Arc<AtomicBool>,
}

// ── Setup typestate ───────────────────────────────────
//
// Each state is a marker type. `Setup<B, S>` threads them through transitions
// that consume `self` and return the next state, so skipping or reordering a
// step is a compile error. State-specific data (`player_slot` after Welcome)
// lives on the marker struct.

struct Setup<B, S> {
    core: DispatcherCore<B>,
    state: S,
}

struct Initial;
struct Connected;
struct Identified {
    player_slot: PlayerSlot,
}

impl<B: EmbeddedBot> Setup<B, Initial> {
    fn new(core: DispatcherCore<B>) -> Self {
        Self {
            core,
            state: Initial,
        }
    }

    /// Emit `BotMsg::Identify` from the bot's declared identity + options.
    ///
    /// Synchronous: `UnboundedSender::send` never awaits.
    fn emit_identify(self, identity: &PlayerIdentity) -> Result<Setup<B, Connected>, PlayerError> {
        let options = self.core.bot.option_defs();
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
    /// Await `HostMsg::Configure`, apply options, compute the initial state
    /// hash, emit `BotMsg::Ready`. Returns a `Playing<Synced>`.
    ///
    /// Options whose `apply_option` returns `Err` are reported through the
    /// event sink but do not abort setup (the fault policy for bad option
    /// values is "warn and use default", matching the SDK).
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
            if let Err(err) = self.core.bot.apply_option(name, value) {
                tracing::warn!(option = %name, error = %err, "bot rejected option");
            }
        }

        let initial_state = initial_turn_state(&match_config);
        let hash = initial_state.state_hash();

        self.core
            .bot_tx
            .send(BotMsg::Ready { state_hash: hash })
            .map_err(|_| PlayerError::TransportError("bot_tx closed".into()))?;

        Ok(Playing {
            core: self.core,
            player_slot: self.state.player_slot,
            match_config,
            state: initial_state,
            _sync: std::marker::PhantomData,
        })
    }
}

// ── Playing<Sync> (placeholder — dispatcher loop in follow-up) ────────

/// Sync marker: the local state mirror agrees with the server's.
struct Synced;

/// Playing state with sync-status marker. Methods and the inner runtime-enum
/// step loop (Idle / Syncing / Thinking / Preprocessing) land in the next
/// commit; the struct exists here so setup has a concrete return type.
#[allow(
    dead_code,
    reason = "fields consumed by the playing loop in a follow-up commit"
)]
struct Playing<B, Sync> {
    core: DispatcherCore<B>,
    player_slot: PlayerSlot,
    match_config: Box<pyrat_protocol::OwnedMatchConfig>,
    state: HashedTurnState,
    _sync: std::marker::PhantomData<Sync>,
}

/// Build the initial `HashedTurnState` a client would compute at Ready time.
///
/// Positions from the config, scores zero, no mud, no last move, all cheese
/// still on the board. Does not require the engine.
fn initial_turn_state(cfg: &pyrat_protocol::OwnedMatchConfig) -> HashedTurnState {
    HashedTurnState::new(pyrat_protocol::OwnedTurnState {
        turn: 0,
        player1_position: cfg.player1_start,
        player2_position: cfg.player2_start,
        player1_score: 0.0,
        player2_score: 0.0,
        player1_mud_turns: 0,
        player2_mud_turns: 0,
        cheese: cfg.cheese.clone(),
        player1_last_move: Direction::Stay,
        player2_last_move: Direction::Stay,
    })
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
        let expected_hash = initial_turn_state(&match_config).state_hash();
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

        // Setup complete. The current (partial) dispatcher exits immediately
        // after Ready; close the player and confirm clean exit.
        // recv() should surface Ok(None) once the dispatcher drops bot_tx.
        assert!(matches!(player.recv().await, Ok(None)));
        player.close().await.unwrap();
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
