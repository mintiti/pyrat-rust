//! Match events emitted by the host during setup and play.
//!
//! The host is a pipe, not a database — it forwards events through
//! a channel. Consumers (headless binary, GUI, tournament system)
//! decide what to record, display, or discard.

use tokio::sync::mpsc;
use tracing::warn;

use crate::session::messages::{DisconnectReason, HashedTurnState, OwnedInfo, OwnedMatchConfig};
use pyrat_wire::{Direction, Player};

use super::playing::MatchResult;

/// Events emitted during a match, from setup through game over.
#[derive(Debug, Clone)]
#[non_exhaustive]
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
    /// The match is starting — includes the resolved match configuration.
    MatchStarted { config: OwnedMatchConfig },

    // ── Playing ──────────────────────────────────
    /// A turn was played and the engine stepped.
    TurnPlayed {
        state: HashedTurnState,
        p1_action: Direction,
        p2_action: Direction,
        p1_think_ms: u32,
        p2_think_ms: u32,
    },
    /// A bot sent debug/analysis info.
    BotInfo {
        sender: Player,
        turn: u16,
        state_hash: u64,
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

/// Send an event if a receiver is attached.
///
/// Logs a warning if the receiver has been dropped (event lost).
pub(crate) fn emit(tx: Option<&mpsc::UnboundedSender<MatchEvent>>, event: MatchEvent) {
    if let Some(tx) = tx {
        if tx.send(event).is_err() {
            warn!("event receiver dropped — event lost");
        }
    }
}
