//! Player abstraction: bidirectional message pipe between Match and bot endpoint.
//!
//! The `Player` trait is the single protocol interface Match uses to drive a
//! bot. It is transport-agnostic: an [`embedded::EmbeddedPlayer`] runs a bot
//! in-process (no TCP, no serialization), while a future `TcpPlayer` will wrap
//! the session layer. Either way, the Match only speaks in [`HostMsg`] /
//! [`BotMsg`] through [`Player::send`] and [`Player::recv`].

use pyrat_protocol::{BotMsg, HostMsg};
use tokio::sync::mpsc;

pub mod embedded;

pub use embedded::{EmbeddedBot, EmbeddedCtx, EmbeddedPlayer};
pub use pyrat_bot_api::{InfoParams, Options};

use crate::game_loop::MatchEvent;

/// Identity known at construction time (before any handshake).
///
/// Same fields as the bot-reported `Identify` message, but carried separately:
/// Match needs to know who the player is before it can route messages, and a
/// caller (test harness, orchestrator, GUI) knows the identity of an embedded
/// bot a priori.
#[derive(Debug, Clone)]
pub struct PlayerIdentity {
    pub name: String,
    pub author: String,
    pub agent_id: String,
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
    /// Reserved for future impls where the peer signals clean close
    /// explicitly (e.g., `TcpPlayer` receiving a Disconnect frame).
    /// `EmbeddedPlayer` signals clean close via `Ok(None)` on `recv()` and
    /// `Ok(())` on `close()`, so it never constructs this variant.
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
/// - `TcpPlayer` (future): wraps the existing session layer.
///
/// # Cancel-safety
///
/// The contract is asymmetric:
/// - `recv()` **MUST** be cancel-safe. Match selects on it (waiting for either
///   player plus a timeout); a cancelled `recv()` future must not lose
///   buffered bytes or drop an already-received message. For [`EmbeddedPlayer`]
///   this falls out of `tokio::sync::mpsc::Receiver::recv`.
/// - `send()` need not be cancel-safe. Match always awaits it to completion.
/// - `close()` need not be cancel-safe. Same reason.
///
/// # Sideband routing
///
/// Players receive an [`EventSink`] at construction and route observer-facing
/// messages (Info, Provisional, RenderCommands) directly to it. `recv()`
/// surfaces only game-driving messages (`Identify`, `Ready`,
/// `PreprocessingDone`, `SyncOk`, `Resync`, `Action`).
pub trait Player: Send + Sync {
    /// Identity known at construction, before any handshake.
    fn identity(&self) -> &PlayerIdentity;

    fn send(
        &mut self,
        msg: HostMsg,
    ) -> impl std::future::Future<Output = Result<(), PlayerError>> + Send;

    /// Receive the next bot-to-host message.
    ///
    /// - `Ok(Some(msg))`: a message was received.
    /// - `Ok(None)`: the peer closed cleanly.
    /// - `Err(_)`: a protocol, transport, or timeout failure occurred.
    fn recv(
        &mut self,
    ) -> impl std::future::Future<Output = Result<Option<BotMsg>, PlayerError>> + Send;

    /// Close the player cleanly.
    ///
    /// Best-effort, bounded, always attempted by Match's run loop on exit.
    /// If the peer is already gone, swallow the error and return `Ok(())`.
    fn close(self) -> impl std::future::Future<Output = Result<(), PlayerError>> + Send;
}
