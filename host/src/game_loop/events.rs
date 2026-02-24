//! Match events emitted by the host during setup and play.
//!
//! The host is a pipe, not a database — it forwards events through
//! a channel. Consumers (headless binary, GUI, tournament system)
//! decide what to record, display, or discard.

use crate::session::messages::{DisconnectReason, OwnedInfo, OwnedTurnState};
use crate::wire::{Direction, Player};

use super::playing::MatchResult;

/// Events emitted during a match, from setup through game over.
#[derive(Debug, Clone)]
pub enum MatchEvent {
    // ── Setup ────────────────────────────────────
    /// A bot identified and was assigned to a player slot.
    BotIdentified {
        player: Player,
        name: String,
        author: String,
    },
    /// All bots connected, identified, configured, and preprocessed.
    SetupComplete,

    // ── Playing ──────────────────────────────────
    /// A turn was played and the engine stepped.
    TurnPlayed {
        turn: u16,
        state: OwnedTurnState,
        p1_action: Direction,
        p2_action: Direction,
    },
    /// A bot sent debug/analysis info.
    BotInfo {
        player: Player,
        turn: u16,
        info: OwnedInfo,
    },
    /// A bot timed out on an action this turn.
    BotTimeout { player: Player, turn: u16 },
    /// A bot disconnected during play.
    BotDisconnected {
        player: Player,
        reason: DisconnectReason,
    },

    // ── End ──────────────────────────────────────
    /// The match ended.
    MatchOver { result: MatchResult },
}
