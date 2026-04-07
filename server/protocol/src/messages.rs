//! Protocol message enums: the vocabulary of the Player trait pipe.
//!
//! `HostMsg` is what the host sends to a player. `BotMsg` is what a player
//! sends back. The Match drives the protocol by sending and receiving these
//! messages through the Player trait's `send`/`recv` methods.
//!
//! These enums define the *new* protocol from the protocol spec. They are
//! distinct from the host-internal `HostCommand`/`SessionMsg` channel types,
//! which will eventually be replaced.

use pyrat::Direction;
use pyrat_wire::{GameResult, Player};

use crate::{OwnedInfo, OwnedMatchConfig, OwnedOptionDef, OwnedTurnState};

// ── Search limits ───────────────────────────────────

/// Search limits sent with Go/GoState, analogous to UCI `go` variants.
///
/// All fields are optional. Unset = unconstrained.
#[derive(Debug, Clone, Default)]
pub struct SearchLimits {
    /// Think for up to N milliseconds. `None` = no time limit.
    pub timeout_ms: Option<u32>,
    /// Search to depth N. `None` = no depth limit.
    pub depth: Option<u16>,
    /// Search N nodes. `None` = no node limit.
    pub nodes: Option<u32>,
}

// ── Host → Player ───────────────────────────────────

/// Message from host to player.
///
/// The Match sends these through `Player::send()`. Each variant corresponds
/// to a protocol message from the spec.
#[derive(Debug)]
pub enum HostMsg {
    /// Waiting phase: assign player slot after Identify.
    Welcome { player_slot: Player },

    /// Lobby phase: configure options and send match config.
    Configure {
        options: Vec<(String, String)>,
        match_config: Box<OwnedMatchConfig>,
    },

    /// Playing phase: begin preprocessing.
    GoPreprocess { state_hash: u64 },

    /// Playing phase: delta update after a turn. Both directions, new turn
    /// number, and the hash of the resulting state.
    Advance {
        p1_dir: Direction,
        p2_dir: Direction,
        turn: u16,
        new_hash: u64,
    },

    /// Playing phase: start thinking. Player is already synced via Advance/SyncOk.
    Go {
        state_hash: u64,
        limits: SearchLimits,
    },

    /// Playing phase: start thinking on an arbitrary state. No prior sync needed.
    /// Used for analysis mode, restart, and reconnection recovery.
    GoState {
        turn_state: Box<OwnedTurnState>,
        state_hash: u64,
        limits: SearchLimits,
    },

    /// Playing phase: stop thinking, send best action immediately.
    Stop,

    /// Any phase: full reconstruction payload, response to Resync.
    FullState {
        match_config: Box<OwnedMatchConfig>,
        turn_state: Box<OwnedTurnState>,
    },

    /// Any phase: protocol violation, followed by disconnect.
    ProtocolError { reason: String },

    /// End: game is over.
    GameOver {
        result: GameResult,
        player1_score: f32,
        player2_score: f32,
    },
}

// ── Player → Host ───────────────────────────────────

/// Message from player to host.
///
/// The Match receives these through `Player::recv()`.
#[derive(Debug)]
pub enum BotMsg {
    /// Waiting phase: identify and declare configurable options.
    Identify {
        name: String,
        author: String,
        agent_id: String,
        options: Vec<OwnedOptionDef>,
    },

    /// Lobby phase: ready with state hash (initial sync).
    Ready { state_hash: u64 },

    /// Playing phase: preprocessing complete.
    PreprocessingDone,

    /// Playing phase: state sync confirmed after Advance.
    SyncOk { hash: u64 },

    /// Any phase: client detected hash mismatch, requests FullState.
    Resync { my_hash: u64 },

    /// Playing phase: committed action for this turn.
    Action {
        direction: Direction,
        player: Player,
        turn: u16,
        state_hash: u64,
        think_ms: u32,
    },

    /// Playing phase: provisional best-so-far action. Host holds latest
    /// as fallback if the bot doesn't commit in time.
    Provisional {
        direction: Direction,
        player: Player,
        turn: u16,
        state_hash: u64,
    },

    /// Playing phase: analysis/debug info (sideband). Host forwards to
    /// event stream without inspecting.
    Info(OwnedInfo),

    /// Playing phase: render commands for GUI visualization (sideband).
    /// Body TBD (separate brief).
    RenderCommands {
        player: Player,
        turn: u16,
        state_hash: u64,
    },
}
