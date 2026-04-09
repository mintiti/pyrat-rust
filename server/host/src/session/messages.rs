use tokio::sync::mpsc;

use pyrat_wire::{GameResult, Player};

// Re-export protocol types so internal `use crate::session::messages::*` paths stay valid.
pub use pyrat_protocol::{
    HashedTurnState, MudEntry, OwnedInfo, OwnedMatchConfig, OwnedOptionDef, OwnedTurnState,
};

// ── Session identity ────────────────────────────────

/// Opaque session identifier assigned by the caller.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SessionId(pub u64);

impl SessionId {
    /// Placeholder ID for non-game sessions (stubs, test harnesses, etc.).
    pub const STUB: Self = Self(u64::MAX);
}

// ── Session → Game loop ─────────────────────────────

/// Messages sent from a session task to the game loop.
///
/// All sessions send into one shared mpsc channel. The `session_id` field
/// identifies the sender.
#[derive(Debug)]
pub enum SessionMsg {
    /// Session established — includes the command channel for the game loop
    /// to send host commands back to this session.
    Connected {
        session_id: SessionId,
        cmd_tx: mpsc::Sender<HostCommand>,
    },
    /// Bot sent Identify with name, author, declared options, and agent_id.
    Identified {
        session_id: SessionId,
        name: String,
        author: String,
        options: Vec<OwnedOptionDef>,
        agent_id: String,
    },
    /// Bot declared itself ready to receive match configuration.
    Ready { session_id: SessionId },
    /// Bot finished preprocessing.
    PreprocessingDone { session_id: SessionId },
    /// Bot submitted a move for a player.
    Action {
        session_id: SessionId,
        player: Player,
        direction: pyrat::Direction,
        turn: u16,
        provisional: bool,
        think_ms: u32,
    },
    /// Bot sent debug/analysis info (forwarded as-is).
    Info {
        session_id: SessionId,
        info: OwnedInfo,
    },
    /// Session ended (TCP closed, shutdown, or error).
    Disconnected {
        session_id: SessionId,
        reason: DisconnectReason,
    },
}

/// Why a session ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DisconnectReason {
    /// The remote peer closed the connection cleanly.
    PeerClosed,
    /// A framing error occurred on the wire.
    FrameError,
    /// The game loop dropped the command channel.
    ChannelClosed,
    /// The bot never sent Identify within the allowed window.
    HandshakeTimeout,
    /// Post-shutdown/game-over drain budget exhausted.
    DrainComplete,
}

// ── Game loop → Session ─────────────────────────────

/// Commands sent from the game loop to an individual session task.
#[derive(Debug, Clone)]
pub enum HostCommand {
    SetOption {
        name: String,
        value: String,
    },
    MatchConfig(Box<OwnedMatchConfig>),
    StartPreprocessing {
        state_hash: u64,
    },
    TurnState(Box<HashedTurnState>),
    Timeout {
        default_move: pyrat::Direction,
    },
    GameOver {
        result: GameResult,
        player1_score: f32,
        player2_score: f32,
    },
    Ping,
    /// Tell the bot to stop thinking. Session stays alive.
    Stop,
    /// Send Stop on the wire, then enter drain mode and close the session.
    Shutdown,
}
