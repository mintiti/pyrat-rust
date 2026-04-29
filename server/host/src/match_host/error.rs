//! Errors the [`Match`](super::Match) lifecycle can return.

use pyrat_wire::Player as PlayerSlot;

use crate::player::PlayerError;

/// What can go wrong during a match.
#[derive(Debug, thiserror::Error)]
pub enum MatchError {
    /// A bot didn't reply to `Configure` within the setup timeout, or its
    /// `Ready` never arrived.
    #[error("setup timed out for player {0:?}")]
    SetupTimeout(PlayerSlot),

    /// Preprocessing didn't finish within `preprocessing_timeout`.
    #[error("preprocessing timed out for player {0:?}")]
    PreprocessingTimeout(PlayerSlot),

    /// Bot's `Ready { state_hash }` doesn't match the host's engine hash.
    /// Authoritative — the match cannot start.
    #[error("ready hash mismatch for player {slot:?}: expected {expected:#x}, got {got:#x}")]
    ReadyHashMismatch {
        slot: PlayerSlot,
        expected: u64,
        got: u64,
    },

    /// A bot sent a second `Resync` for the same turn after we already
    /// supplied a `FullState`. Bounded retry: 1 per turn per player.
    #[error("persistent desync for player {0:?}")]
    PersistentDesync(PlayerSlot),

    /// Bot's `Action.state_hash` doesn't match the host's expected hash for
    /// this turn — the bot computed a move for a different state than the
    /// one the host believes is current.
    #[error("action hash mismatch for player {slot:?}: expected {expected:#x}, got {got:#x}")]
    ActionHashMismatch {
        slot: PlayerSlot,
        expected: u64,
        got: u64,
    },

    /// A bot disconnected (clean close) at a phase where Match needs both
    /// players present.
    #[error("player {0:?} disconnected during match")]
    BotDisconnected(PlayerSlot),

    /// Underlying [`PlayerError`] from `send`/`recv`/`close`.
    #[error("player {slot:?} error: {source}")]
    PlayerError {
        slot: PlayerSlot,
        #[source]
        source: PlayerError,
    },

    /// A bot sent a message that doesn't fit the expected sequence (e.g.
    /// `Action` while the host was waiting for `SyncOk`).
    #[error("unexpected message from player {slot:?}: {detail}")]
    UnexpectedMessage { slot: PlayerSlot, detail: String },

    /// Host-side invariant violation; report and investigate.
    #[error("internal match error: {0}")]
    Internal(String),
}

impl MatchError {
    pub(crate) fn from_player(slot: PlayerSlot, source: PlayerError) -> Self {
        Self::PlayerError { slot, source }
    }
}
