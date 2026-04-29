//! Re-exports the canonical [`MatchEvent`] from [`crate::match_host`].
//!
//! `MatchEvent` was re-homed to `match_host` in slice 5 of the host
//! restructure. This shim keeps the legacy `game_loop` paths compiling
//! until slice 9 deletes the module entirely.

pub(crate) use crate::match_host::emit;
pub use crate::match_host::MatchEvent;
