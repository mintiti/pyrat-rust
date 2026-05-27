//! Shared game-config layer between `run-one` and `tournament run`.
//!
//! `ResolvedGameChoice` captures the "which game to build" decision in a
//! shape that (a) maps to a runtime `GameConfig` for the orchestrator and
//! (b) round-trips back into TOML via `--save-as`. Building the runtime
//! `GameConfig` from this choice happens at the point of use, not on the
//! resolver — one source of truth.
//!
//! `build_game_config(&ResolvedGameChoice)` lands in Chunk 5.

#![allow(dead_code)] // build_game_config arrives in Chunk 5.

use std::num::NonZeroU16;

/// The game-config decision after CLI flags, config, and defaults have been
/// resolved. Carries enough information to build a runtime `GameConfig` and
/// to serialize back into the TOML schema's `[game]` section.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResolvedGameChoice {
    Preset {
        name: String,
        max_turns_override: Option<NonZeroU16>,
    },
    Custom {
        width: u8,
        height: u8,
        cheese: u16,
        symmetric: bool,
        max_turns: Option<NonZeroU16>,
    },
}
