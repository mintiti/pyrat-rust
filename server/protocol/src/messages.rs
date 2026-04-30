//! Protocol message enums: the vocabulary of the Player trait pipe.
//!
//! `HostMsg` is what the host sends to a player. `BotMsg` is what a player
//! sends back. The Match drives the protocol by sending and receiving these
//! messages through the Player trait's `send`/`recv` methods.

use pyrat::Direction;
use pyrat_wire::{GameResult, Player};

use crate::{Info, MatchConfig, OptionDef, TurnState};

// ── Search limits ───────────────────────────────────

/// Search limits sent with Go/GoState, analogous to UCI `go` variants.
///
/// All fields are optional. Unset = unconstrained. Combines into a single
/// flat struct: `go timeout 100`, `go depth 5`, `go nodes 10000`, or
/// `go infinite` (all fields `None`).
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
    ///
    /// Sent once per client in response to [`BotMsg::Identify`]. The server
    /// assigns the slot; the client does not choose. Once all players are
    /// welcomed, the match transitions to the Lobby phase.
    Welcome { player_slot: Player },

    /// Lobby phase: configure options and send match config.
    ///
    /// Batches option overrides and the full maze/timing configuration in one
    /// message. The client loads the config, computes the initial state hash,
    /// and responds with [`BotMsg::Ready`]. Options may be empty if the bot
    /// declared none in Identify.
    Configure {
        options: Vec<(String, String)>,
        match_config: Box<MatchConfig>,
    },

    /// Playing phase: begin preprocessing.
    ///
    /// Sent once per match, before the turn loop. The client may send
    /// [`BotMsg::Info`] and [`BotMsg::RenderCommands`] during preprocessing
    /// (but not [`BotMsg::Provisional`]). Ends when the client sends
    /// [`BotMsg::PreprocessingDone`].
    GoPreprocess { state_hash: u64 },

    /// Playing phase: delta update after a turn.
    ///
    /// Carries both directions, the new turn number, and the hash of the
    /// resulting state. The client applies the directions locally and verifies
    /// `new_hash` against its own computation. On match, it responds with
    /// [`BotMsg::SyncOk`]. On mismatch, it sends [`BotMsg::Resync`].
    ///
    /// Not sent before the first turn (players are already synced from Ready).
    Advance {
        p1_dir: Direction,
        p2_dir: Direction,
        turn: u16,
        new_hash: u64,
    },

    /// Playing phase: start thinking.
    ///
    /// Lightweight: hash + search limits only. The player is already synced
    /// via the Advance/SyncOk exchange. Sent to both players simultaneously
    /// after both have sent [`BotMsg::SyncOk`] (fairness gate). On the first
    /// turn, sent directly after all Ready messages (no preceding Advance).
    Go {
        state_hash: u64,
        limits: SearchLimits,
    },

    /// Playing phase: start thinking on an arbitrary state.
    ///
    /// Self-contained: carries full turn state, hash, and search limits. No
    /// prior Advance/SyncOk exchange needed. Used for analysis mode (GUI sends
    /// arbitrary position), restart, and reconnection recovery.
    GoState {
        turn_state: Box<TurnState>,
        state_hash: u64,
        limits: SearchLimits,
    },

    /// Playing phase: stop thinking, send best action immediately.
    ///
    /// No fields. The bot sends its best [`BotMsg::Action`] as soon as
    /// possible (best effort). The server resolves on its own schedule:
    /// committed Action > latest Provisional > STAY. Covers both
    /// deadline-fired (timeout) and consumer-triggered (GUI pause) stops.
    Stop,

    /// Any phase: full reconstruction payload, response to [`BotMsg::Resync`].
    ///
    /// Carries MatchConfig + TurnState (~2-4 KB). The client loads it,
    /// recomputes the hash, and sends [`BotMsg::SyncOk`]. The current phase
    /// is preserved: recovery returns to where the client was, not back to
    /// the start.
    FullState {
        match_config: Box<MatchConfig>,
        turn_state: Box<TurnState>,
    },

    /// Any phase: protocol violation, followed by disconnect.
    ///
    /// Terminal. No recoverable protocol errors: if a client violates the
    /// protocol, the server cannot trust it. Sideband errors (malformed Info
    /// or RenderCommands) are a different category: logged and dropped, since
    /// they don't affect game state.
    ProtocolError { reason: String },

    /// End: game is over.
    ///
    /// Sent instead of [`Advance`](Self::Advance) when the game ends.
    /// Transitions to PostMatch: clients have a cleanup window (persist data,
    /// diagnostics, final Info) before the connection closes.
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
    ///
    /// First message after connect. `options` are UCI-style declarations
    /// (name, type, default, constraints) advertising knobs the host or GUI
    /// can set. Empty list if the bot has no configurable parameters. Server
    /// responds with [`HostMsg::Welcome`].
    Identify {
        name: String,
        author: String,
        agent_id: String,
        options: Vec<OptionDef>,
    },

    /// Lobby phase: ready with state hash (initial sync).
    ///
    /// This IS the initial sync: no separate exchange needed. The server
    /// verifies the hash against its own state. All hashes match → transition
    /// to Playing. Mismatch → the match cannot start.
    Ready { state_hash: u64 },

    /// Playing phase: preprocessing complete.
    ///
    /// Transitions the player to Idle. Next message from the host will be
    /// [`HostMsg::Go`] (first turn) or [`HostMsg::Advance`] (if waiting for
    /// the other player to finish preprocessing).
    PreprocessingDone,

    /// Playing phase: state sync confirmed after Advance.
    ///
    /// Part of the fairness gate: the server waits for both players to send
    /// SyncOk before sending [`HostMsg::Go`] to either. Near-instant (just a
    /// hash compare on the client side).
    SyncOk { hash: u64 },

    /// Any phase: client detected hash mismatch, requests FullState.
    ///
    /// Triggers [`HostMsg::FullState`] from the server. After loading the
    /// full state, the client sends [`SyncOk`](Self::SyncOk) to resume.
    /// Valid in any InMatch state (Preprocessing, WaitingForSyncOk,
    /// WaitingForAction). Phase is preserved.
    Resync { my_hash: u64 },

    /// Playing phase: committed action for this turn.
    ///
    /// `state_hash` tags which game state the move was computed for,
    /// enabling stale-action detection. `think_ms` reports actual wall-clock
    /// time spent thinking (for display and diagnostics, not enforcement).
    Action {
        direction: Direction,
        player: Player,
        turn: u16,
        state_hash: u64,
        think_ms: u32,
    },

    /// Playing phase: provisional best-so-far action (sideband).
    ///
    /// The host holds the latest provisional as a timeout fallback. If a
    /// newer provisional arrives before the previous one was consumed, it
    /// replaces it (not queued). Resolution order on timeout:
    /// committed Action > latest Provisional > STAY.
    Provisional {
        direction: Direction,
        player: Player,
        turn: u16,
        state_hash: u64,
    },

    /// Playing phase: analysis/debug info (sideband).
    ///
    /// Observer-facing: the host forwards to the event stream without
    /// inspecting contents. Tagged with `state_hash` for correlation with
    /// the game state being analyzed. Valid during both Preprocessing and
    /// Thinking.
    Info(Info),

    /// Playing phase: render commands for GUI visualization (sideband).
    ///
    /// Same forwarding model as [`Info`](Self::Info): observer-facing, host
    /// never inspects. Tagged with player, turn, and state_hash. Body format
    /// TBD (separate brief).
    RenderCommands {
        player: Player,
        turn: u16,
        state_hash: u64,
    },
}
