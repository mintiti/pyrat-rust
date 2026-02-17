//! Shared scenario definitions for benchmarks and profiling.
//!
//! Used by both `cargo bench` (criterion) and the `profile_game` binary.

use crate::{Direction, GameBuilder, GameState, MazeParams};
use rand::{Rng, RngExt};

/// Board dimensions and game parameters for a benchmark scenario.
pub struct BoardSize {
    pub name: &'static str,
    pub width: u8,
    pub height: u8,
    pub cheese: u16,
    pub max_turns: u16,
}

/// Wall/mud density combination for a benchmark scenario.
pub struct FeatureCombo {
    pub name: &'static str,
    pub wall_density: f32,
    pub mud_density: f32,
}

pub const SIZES: &[BoardSize] = &[
    BoardSize {
        name: "tiny",
        width: 11,
        height: 9,
        cheese: 13,
        max_turns: 150,
    },
    BoardSize {
        name: "small",
        width: 15,
        height: 11,
        cheese: 21,
        max_turns: 200,
    },
    BoardSize {
        name: "default",
        width: 21,
        height: 15,
        cheese: 41,
        max_turns: 300,
    },
    BoardSize {
        name: "large",
        width: 31,
        height: 21,
        cheese: 85,
        max_turns: 400,
    },
    BoardSize {
        name: "huge",
        width: 41,
        height: 31,
        cheese: 165,
        max_turns: 500,
    },
];

pub const COMBOS: &[FeatureCombo] = &[
    FeatureCombo {
        name: "empty",
        wall_density: 0.0,
        mud_density: 0.0,
    },
    FeatureCombo {
        name: "walls_only",
        wall_density: 0.7,
        mud_density: 0.0,
    },
    FeatureCombo {
        name: "mud_only",
        wall_density: 0.0,
        mud_density: 0.1,
    },
    FeatureCombo {
        name: "default",
        wall_density: 0.7,
        mud_density: 0.1,
    },
];

#[inline]
pub fn random_direction(rng: &mut impl Rng) -> Direction {
    match rng.random_range(0u8..5) {
        0 => Direction::Up,
        1 => Direction::Right,
        2 => Direction::Down,
        3 => Direction::Left,
        _ => Direction::Stay,
    }
}

pub fn create_game(size: &BoardSize, combo: &FeatureCombo, seed: u64) -> GameState {
    GameBuilder::new(size.width, size.height)
        .with_max_turns(size.max_turns)
        .with_random_maze(MazeParams {
            target_density: combo.wall_density,
            mud_density: combo.mud_density,
            ..MazeParams::default()
        })
        .with_corner_positions()
        .with_random_cheese(size.cheese, true)
        .build()
        .create(Some(seed))
}
