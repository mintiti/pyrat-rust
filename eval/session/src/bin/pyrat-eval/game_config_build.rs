//! Shared game-config layer between `run-one` and `tournament run`.
//!
//! `ResolvedGame` captures the "which game to build" decision in the
//! resolver's documented algebra: a whole *shape* decision plus an
//! optional `max_turns` overlay. The type mirrors the two decisions so
//! consumers never reach through variants for the overlay field.
//! `build_game_config` turns the decision into a runtime `GameConfig`;
//! `tournament_save` serializes it back into the TOML `[game]` section.

use std::num::NonZeroU16;

use pyrat::game::builder::{GameBuilder, GameConfig, MazeParams};

/// The game-config decision after CLI flags, config, and defaults have
/// been resolved.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedGame {
    pub shape: GameShape,
    /// Overlay on top of whichever shape won; `None` keeps the shape's
    /// own default (the preset's value, or 300 for custom dims).
    pub max_turns: Option<NonZeroU16>,
}

/// The indivisible shape half of the decision: a named preset XOR fully
/// specified custom dims.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GameShape {
    Preset {
        name: String,
    },
    Custom {
        width: u8,
        height: u8,
        cheese: u16,
        symmetric: bool,
    },
}

/// Build a runtime `GameConfig` from the resolver's decision.
///
/// Custom dims go through `GameBuilder` so the `symmetric` flag is honored
/// for both maze layout and cheese placement; presets pin their own
/// shape (see `GameConfig::preset`). The optional `max_turns` overlay
/// rides on top of either path.
pub fn build_game_config(game: &ResolvedGame) -> Result<GameConfig, String> {
    let mut cfg = match &game.shape {
        GameShape::Preset { name } => GameConfig::preset(name)?,
        GameShape::Custom {
            width,
            height,
            cheese,
            symmetric,
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
    if let Some(n) = game.max_turns {
        cfg = cfg.with_max_turns(n.get());
    }
    Ok(cfg)
}
