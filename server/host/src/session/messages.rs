use std::hash::{Hash, Hasher};
use std::ops::Deref;

use tokio::sync::mpsc;

use pyrat_wire::{Direction, GameResult, OptionType, Player, TimingMode};

// ── Session identity ────────────────────────────────

/// Opaque session identifier assigned by the caller.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SessionId(pub u64);

impl SessionId {
    /// Placeholder ID for non-game sessions (stubs, test harnesses, etc.).
    pub const STUB: Self = Self(u64::MAX);
}

// ── Owned types extracted from FlatBuffers ──────────

/// Owned copy of a bot-declared option (from Identify).
#[derive(Debug, Clone)]
pub struct OwnedOptionDef {
    pub name: String,
    pub option_type: OptionType,
    pub default_value: String,
    pub min: i32,
    pub max: i32,
    pub choices: Vec<String>,
}

/// Owned copy of a bot Info message.
#[derive(Debug, Clone)]
pub struct OwnedInfo {
    pub player: Player,
    pub multipv: u16,
    pub target: Option<(u8, u8)>,
    pub depth: u16,
    pub nodes: u32,
    pub score: Option<f32>,
    pub pv: Vec<Direction>,
    pub message: String,
    pub turn: u16,
    pub state_hash: u64,
}

/// Mud entry: (pos1, pos2, mud_value).
pub type MudEntry = ((u8, u8), (u8, u8), u8);

/// Owned match configuration sent to the bot.
#[derive(Debug, Clone)]
pub struct OwnedMatchConfig {
    pub width: u8,
    pub height: u8,
    pub max_turns: u16,
    pub walls: Vec<((u8, u8), (u8, u8))>,
    pub mud: Vec<MudEntry>,
    pub cheese: Vec<(u8, u8)>,
    pub player1_start: (u8, u8),
    pub player2_start: (u8, u8),
    pub controlled_players: Vec<Player>,
    pub timing: TimingMode,
    pub move_timeout_ms: u32,
    pub preprocessing_timeout_ms: u32,
}

/// Owned turn state sent to the bot each turn.
///
/// Contains the raw game-position fields. Does **not** include `state_hash`,
/// which is a derived value. Use [`HashedTurnState`] to pair a turn state with
/// its content-addressable hash.
///
/// If you add or change position-defining fields here, update
/// [`HashedTurnState::compute_hash`] in this same file.
#[derive(Debug, Clone)]
pub struct OwnedTurnState {
    pub turn: u16,
    pub player1_position: (u8, u8),
    pub player2_position: (u8, u8),
    pub player1_score: f32,
    pub player2_score: f32,
    pub player1_mud_turns: u8,
    pub player2_mud_turns: u8,
    pub cheese: Vec<(u8, u8)>,
    pub player1_last_move: Direction,
    pub player2_last_move: Direction,
}

/// An [`OwnedTurnState`] paired with a content-addressable hash of its
/// position-defining fields.
///
/// The hash is computed once at construction time. Two states that a bot would
/// analyze differently will hash differently.
#[derive(Debug, Clone)]
pub struct HashedTurnState {
    inner: OwnedTurnState,
    state_hash: u64,
}

impl HashedTurnState {
    /// Wrap a turn state, computing the hash from its fields.
    pub fn new(ts: OwnedTurnState) -> Self {
        let state_hash = Self::compute_hash(&ts);
        Self {
            inner: ts,
            state_hash,
        }
    }

    /// Wrap a turn state with a pre-computed hash (from `GameState::state_hash()`).
    pub fn with_hash(ts: OwnedTurnState, state_hash: u64) -> Self {
        Self {
            inner: ts,
            state_hash,
        }
    }

    /// The content-addressable hash for this turn state.
    pub fn state_hash(&self) -> u64 {
        self.state_hash
    }

    /// Deterministic hash of all game-position fields.
    ///
    /// Two states that a bot would analyze differently must hash differently.
    /// If you add a field to [`OwnedTurnState`], update this function.
    fn compute_hash(ts: &OwnedTurnState) -> u64 {
        let mut h = std::collections::hash_map::DefaultHasher::new();
        ts.turn.hash(&mut h);
        ts.player1_position.hash(&mut h);
        ts.player2_position.hash(&mut h);
        // Hash scores as half-point u16 to avoid float instability
        ((ts.player1_score * 2.0) as u16).hash(&mut h);
        ((ts.player2_score * 2.0) as u16).hash(&mut h);
        ts.player1_mud_turns.hash(&mut h);
        ts.player2_mud_turns.hash(&mut h);
        ts.cheese.hash(&mut h);
        ts.player1_last_move.0.hash(&mut h);
        ts.player2_last_move.0.hash(&mut h);
        h.finish()
    }
}

impl Deref for HashedTurnState {
    type Target = OwnedTurnState;

    fn deref(&self) -> &OwnedTurnState {
        &self.inner
    }
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
        direction: Direction,
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
        default_move: Direction,
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
