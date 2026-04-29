//! Pluggable [`FaultPolicy`] for resolving per-slot action outcomes.
//!
//! The protocol layer (Match's `collect_actions`) produces an
//! [`ActionOutcome`] per slot — Committed, TimedOut, or Disconnected — without
//! deciding what should happen. The [`FaultPolicy`] trait turns each outcome
//! into a final [`Direction`] (or escalates to a [`MatchError`]).
//!
//! Two impls ship in-tree:
//!
//! - [`DefaultFaultPolicy`] — soft. Uses the bot's last `Provisional` on
//!   timeout; falls back to [`Direction::Stay`]. Escalates `Disconnected` to
//!   [`MatchError::BotDisconnected`]. This preserves the host's pre-seam
//!   behavior.
//! - [`StrictFaultPolicy`] — escalates both timeout and disconnect to
//!   [`MatchError`]. Useful for tournament configurations where a missed
//!   action is a forfeit, not a fallback.
//!
//! The policy is held by [`PlayingConfig`](super::PlayingConfig) as an
//! `Arc<dyn FaultPolicy>`, so [`PlayingConfig`] stays `Clone` and policies
//! are cheap to share across matches.

use std::sync::Arc;

use pyrat::Direction;
use pyrat_wire::Player as PlayerSlot;

use super::error::MatchError;

/// Per-slot result of action collection. Produced by Match before any
/// resolution policy runs.
#[derive(Debug, Clone, Copy)]
pub enum ActionOutcome {
    /// Bot sent a hash-and-turn-validated [`Action`](pyrat_protocol::BotMsg::Action).
    Committed { direction: Direction, think_ms: u32 },
    /// Bot didn't commit before the deadline (and Stop's grace window) expired.
    /// The bot may still have a stored `Provisional` for this turn.
    TimedOut,
    /// Bot's transport closed cleanly before committing an Action this turn.
    Disconnected,
}

/// Resolve a per-slot [`ActionOutcome`] to a final [`Direction`] or escalate
/// to a fatal [`MatchError`].
///
/// Called once per slot per turn. `provisional` is the bot's last
/// turn-and-hash-validated [`Provisional`](pyrat_protocol::BotMsg::Provisional)
/// for the current turn, or `None` if the bot didn't send one (or sent one
/// for a stale turn / hash, which the Player layer drops).
pub trait FaultPolicy: std::fmt::Debug + Send + Sync {
    fn resolve_action(
        &self,
        slot: PlayerSlot,
        outcome: ActionOutcome,
        provisional: Option<Direction>,
    ) -> Result<Direction, MatchError>;
}

/// Soft policy: provisional fallback on timeout, fatal on disconnect.
///
/// Resolution table:
///
/// | Outcome | Provisional | Result |
/// |---|---|---|
/// | `Committed` | * | Ok(committed direction) |
/// | `TimedOut` | Some(d) | Ok(d) |
/// | `TimedOut` | None | Ok(Stay) |
/// | `Disconnected` | * | Err(BotDisconnected) |
///
/// This is what Match did inline before the seam was extracted.
#[derive(Debug, Default, Clone, Copy)]
pub struct DefaultFaultPolicy;

impl FaultPolicy for DefaultFaultPolicy {
    fn resolve_action(
        &self,
        slot: PlayerSlot,
        outcome: ActionOutcome,
        provisional: Option<Direction>,
    ) -> Result<Direction, MatchError> {
        match outcome {
            ActionOutcome::Committed { direction, .. } => Ok(direction),
            ActionOutcome::TimedOut => Ok(provisional.unwrap_or(Direction::Stay)),
            ActionOutcome::Disconnected => Err(MatchError::BotDisconnected(slot)),
        }
    }
}

/// Strict policy: any non-Committed outcome is fatal.
///
/// `TimedOut` → [`MatchError::ActionTimeout`]; `Disconnected` →
/// [`MatchError::BotDisconnected`]. Useful for tournament-grade matches where
/// a missed action means a forfeit.
#[derive(Debug, Default, Clone, Copy)]
pub struct StrictFaultPolicy;

impl FaultPolicy for StrictFaultPolicy {
    fn resolve_action(
        &self,
        slot: PlayerSlot,
        outcome: ActionOutcome,
        _provisional: Option<Direction>,
    ) -> Result<Direction, MatchError> {
        match outcome {
            ActionOutcome::Committed { direction, .. } => Ok(direction),
            ActionOutcome::TimedOut => Err(MatchError::ActionTimeout(slot)),
            ActionOutcome::Disconnected => Err(MatchError::BotDisconnected(slot)),
        }
    }
}

/// `Arc<DefaultFaultPolicy>` — what [`PlayingConfig::default`](super::PlayingConfig)
/// hands out. Cheap-cloneable; policies are stateless and fine to share.
pub fn default_policy() -> Arc<dyn FaultPolicy> {
    Arc::new(DefaultFaultPolicy)
}
