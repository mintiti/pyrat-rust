//! Shared game-config layer between `run-one` and `tournament run`.
//!
//! `ResolvedGameChoice` captures the "which game to build" decision in a
//! shape that (a) maps to a runtime `GameConfig` for the orchestrator and
//! (b) round-trips back into TOML via `--save-as`. `build_game_config`
//! turns the choice into the runtime `GameConfig`.

use std::num::NonZeroU16;

use pyrat::game::builder::{GameBuilder, GameConfig, MazeParams};

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

/// Build a runtime `GameConfig` from the resolver's decision.
///
/// Custom dims go through `GameBuilder` so the `symmetric` flag is honored
/// for both maze layout and cheese placement; presets pin their own
/// shape (see `GameConfig::preset`). The optional `max_turns` override
/// rides on top of either path.
pub fn build_game_config(choice: &ResolvedGameChoice) -> Result<GameConfig, String> {
    let mut cfg = match choice {
        ResolvedGameChoice::Preset { name, .. } => GameConfig::preset(name)?,
        ResolvedGameChoice::Custom {
            width,
            height,
            cheese,
            symmetric,
            ..
        } => {
            let params = MazeParams {
                symmetric: *symmetric,
                ..MazeParams::classic()
            };
            GameBuilder::new(*width, *height)
                .with_random_maze(params)
                .with_corner_positions()
                .with_random_cheese(*cheese, *symmetric)
                .build()
        },
    };
    let override_turns = match choice {
        ResolvedGameChoice::Preset {
            max_turns_override, ..
        } => *max_turns_override,
        ResolvedGameChoice::Custom { max_turns, .. } => *max_turns,
    };
    if let Some(n) = override_turns {
        cfg = cfg.with_max_turns(n.get());
    }
    Ok(cfg)
}
