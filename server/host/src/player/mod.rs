//! Player abstraction: bidirectional message pipe between Match and bot endpoint.
//!
//! The `Player` trait is the single protocol interface Match uses to drive a
//! bot. It is transport-agnostic: an [`embedded::EmbeddedPlayer`] runs a bot
//! in-process (no TCP, no serialization), while [`tcp::TcpPlayer`] wraps a TCP
//! connection. Either way, Match only speaks in [`HostMsg`] / [`BotMsg`]
//! through [`Player::send`] and [`Player::recv`].

use async_trait::async_trait;
use pyrat::Direction;
use pyrat_protocol::{BotMsg, HostMsg};
use pyrat_wire::Player as PlayerSlot;
use tokio::sync::mpsc;

pub mod embedded;
pub mod tcp;

pub use embedded::{EmbeddedBot, EmbeddedCtx, EmbeddedPlayer};
pub use pyrat_bot_api::{InfoParams, Options};
pub use tcp::{accept_players, AcceptError, TcpPlayer};

use crate::match_host::MatchEvent;

/// Identity of a player in this match.
///
/// `name` / `author` / `agent_id` come from the bot's `Identify` message and
/// are stable across the bot's lifetime. `slot` is host-assigned: it's the
/// position the bot was given when accepted into the match (the same value
/// that was sent back to the bot in `HostMsg::Welcome`). Both are stable for
/// the player's lifetime in this match.
#[derive(Debug, Clone)]
pub struct PlayerIdentity {
    pub name: String,
    pub author: String,
    pub agent_id: String,
    pub slot: PlayerSlot,
}

/// Optional consumer of observer-facing events.
///
/// Players forward sideband messages (Info, Provisional, RenderCommands) here
/// directly. `recv()` yields only game-driving messages; the Match never
/// inspects sideband.
#[derive(Clone, Default)]
pub struct EventSink {
    tx: Option<mpsc::UnboundedSender<MatchEvent>>,
}

impl EventSink {
    /// A sink that drops every event. Useful for tests that don't care about
    /// sideband.
    pub const fn noop() -> Self {
        Self { tx: None }
    }

    /// Wrap an existing sender.
    pub fn new(tx: mpsc::UnboundedSender<MatchEvent>) -> Self {
        Self { tx: Some(tx) }
    }

    /// Emit an event. Silently dropped if no consumer is attached or the
    /// receiver has hung up.
    pub fn emit(&self, event: MatchEvent) {
        if let Some(tx) = &self.tx {
            let _ = tx.send(event);
        }
    }
}

/// Errors returned by [`Player`] methods.
///
/// Variants separate clean close from protocol or transport faults, and
/// either of those from a deadline timeout.
#[derive(Debug, thiserror::Error)]
pub enum PlayerError {
    /// Reserved for impls where the peer signals clean close explicitly
    /// (e.g., `TcpPlayer` receiving a Disconnect frame). `EmbeddedPlayer`
    /// signals clean close via `Ok(None)` on `recv()` and `Ok(())` on
    /// `close()`, so it never constructs this variant.
    #[error("peer closed cleanly")]
    CleanClose,
    /// Parse failure, unexpected message type, or other protocol violation.
    #[error("protocol error: {0}")]
    ProtocolError(String),
    /// Channel closed unexpectedly, TCP died, or similar transport fault.
    #[error("transport error: {0}")]
    TransportError(String),
    /// Deadline exceeded (clock mode).
    #[error("deadline exceeded")]
    Timeout,
}

/// Bidirectional message pipe between the Match and a bot endpoint.
///
/// Transport-agnostic. Impls:
/// - [`EmbeddedPlayer`]: in-process, no TCP or FlatBuffers.
/// - [`TcpPlayer`]: wraps a TCP connection with its own minimal session task.
///
/// The trait is object-safe (`#[async_trait]` boxes futures): Match holds
/// `[Box<dyn Player>; 2]`, supporting mixed Embedded/Tcp pairs (e.g. GUI
/// step-mode where one slot is an in-process random bot).
///
/// # Cancel-safety
///
/// The contract is asymmetric:
/// - `recv()` **MUST** be cancel-safe. Match selects on it (waiting for either
///   player plus a timeout); a cancelled `recv()` future must not lose
///   buffered bytes or drop an already-received message. Both impls satisfy
///   this via `tokio::sync::mpsc::Receiver::recv` (cancel-safe by design).
/// - `send()` need not be cancel-safe. Match always awaits it to completion.
/// - `close()` need not be cancel-safe. Same reason.
///
/// # Sideband routing
///
/// Players receive an [`EventSink`] at construction and route observer-facing
/// messages directly to it. `recv()` surfaces only game-driving messages
/// (`Identify`, `Ready`, `PreprocessingDone`, `SyncOk`, `Resync`, `Action`).
///
/// # Provisional
///
/// `BotMsg::Provisional` is dual-use and **never** returned from `recv()`:
/// - **Observer-facing**: every receive is forwarded to `EventSink` as
///   `MatchEvent::BotProvisional`.
/// - **Game-driving (poll-style)**: each impl stores the latest provisional
///   keyed by `(turn, state_hash)`. Match calls [`Player::take_provisional`]
///   only on the Stop-during-think fallback path. The slot is cleared on the
///   next `Go` / `GoState` (whole-turn boundary) or by a successful `take`.
#[async_trait]
pub trait Player: Send + Sync {
    /// Identity of this player in the match (bot-declared + host-assigned slot).
    fn identity(&self) -> &PlayerIdentity;

    /// Send a host-to-bot message.
    async fn send(&mut self, msg: HostMsg) -> Result<(), PlayerError>;

    /// Receive the next bot-to-host message.
    ///
    /// - `Ok(Some(msg))`: a game-driving message was received.
    /// - `Ok(None)`: the peer closed cleanly.
    /// - `Err(_)`: a protocol, transport, or timeout failure occurred.
    ///
    /// Sideband messages (`Info`, `RenderCommands`, `Provisional`) are
    /// forwarded to `EventSink` and never returned here.
    async fn recv(&mut self) -> Result<Option<BotMsg>, PlayerError>;

    /// Take the latest provisional direction if it matches `expected_turn`
    /// and `expected_hash`.
    ///
    /// Returns `Some(direction)` and clears the slot if a stored provisional
    /// matches both fields. Returns `None` (and also clears the slot) if the
    /// stored provisional is stale or absent. Match calls this on the Stop
    /// fallback path: `committed > provisional > Stay`.
    fn take_provisional(&mut self, expected_turn: u16, expected_hash: u64) -> Option<Direction>;

    /// Close the player cleanly.
    ///
    /// Best-effort, bounded, always attempted by Match's run loop on exit.
    /// If the peer is already gone, swallow the error and return `Ok(())`.
    ///
    /// Takes `Box<Self>` so the trait remains object-safe — `Match` holds
    /// `[Box<dyn Player>; 2]` and consumes each on shutdown.
    async fn close(self: Box<Self>) -> Result<(), PlayerError>;
}

/// Turn-scoped storage of the latest provisional direction.
///
/// Each Player impl holds one `Option<ProvisionalSlot>`. `recv()` updates it
/// when a `BotMsg::Provisional` arrives; `take_provisional` reads and clears
/// it; sending `Go`/`GoState` also clears it (whole-turn boundary).
#[derive(Debug, Clone, Copy)]
pub(crate) struct ProvisionalSlot {
    pub direction: Direction,
    pub turn: u16,
    pub state_hash: u64,
}

impl ProvisionalSlot {
    /// Returns `Some(direction)` if `expected_turn` and `expected_hash` match
    /// the stored slot, else `None`.
    pub(crate) fn match_take(
        slot: &mut Option<Self>,
        expected_turn: u16,
        expected_hash: u64,
    ) -> Option<Direction> {
        let s = slot.take()?;
        if s.turn == expected_turn && s.state_hash == expected_hash {
            Some(s.direction)
        } else {
            None
        }
    }
}
