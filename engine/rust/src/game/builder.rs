//! Typestate builder for game creation.
//!
//! The builder enforces a compile-time sequence: maze → players → cheese.
//! Once built, `GameConfig` can stamp out `GameState` instances via
//! `create(Option<u64>)`, enabling reuse for RL training loops.
//!
//! ```rust,no_run
//! use pyrat_engine::{GameBuilder, GameConfig, MazeParams};
//!
//! // Quick classic game
//! let config = GameConfig::classic(21, 15, 41);
//! let game = config.create(Some(42));
//!
//! // Builder with named maze constructors
//! let config = GameBuilder::new(21, 15)
//!     .with_classic_maze()
//!     .with_corner_positions()
//!     .with_random_cheese(41, true)
//!     .build();
//!
//! let game1 = config.create(Some(42));
//! let game2 = config.create(Some(43));
//! ```

use crate::game::maze_generation::{CheeseConfig, CheeseGenerator, MazeConfig, MazeGenerator};
use crate::game::types::MudMap;
use crate::{Coordinates, GameState};
use rand::{Rng, RngExt, SeedableRng};
use std::collections::HashMap;
use std::marker::PhantomData;

// ---------------------------------------------------------------------------
// Marker types (zero-sized)
// ---------------------------------------------------------------------------

/// Builder needs a maze strategy.
pub struct NeedsMaze;
/// Builder needs a player placement strategy.
pub struct NeedsPlayers;
/// Builder needs a cheese strategy.
pub struct NeedsCheese;
/// Builder is ready to produce a `GameConfig`.
pub struct Ready;

// ---------------------------------------------------------------------------
// Strategy enums
// ---------------------------------------------------------------------------

/// How the maze (walls + mud) is generated.
#[derive(Clone)]
pub enum MazeStrategy {
    /// Generate walls/mud randomly from parameters.
    Random(MazeParams),
    /// Use a fixed wall map and mud map.
    Fixed {
        walls: HashMap<Coordinates, Vec<Coordinates>>,
        mud: MudMap,
    },
}

/// How player starting positions are chosen.
#[derive(Clone)]
pub enum PlayerStrategy {
    /// Player 1 at (0,0), player 2 at (width-1, height-1).
    Corners,
    /// Both players placed randomly (guaranteed different).
    Random,
    /// Explicit positions.
    Fixed(Coordinates, Coordinates),
}

/// How cheese is placed on the board.
#[derive(Clone)]
pub enum CheeseStrategy {
    /// Place `count` cheese randomly, optionally with 180° symmetry.
    Random { count: u16, symmetric: bool },
    /// Use exact positions.
    Fixed(Vec<Coordinates>),
}

// ---------------------------------------------------------------------------
// MazeParams — the knobs for random maze generation
// ---------------------------------------------------------------------------

/// Parameters for random maze generation (no width/height/seed — those live
/// on the builder / are derived at create-time).
#[derive(Clone, Debug)]
pub struct MazeParams {
    /// Probability of a wall between adjacent cells (0.0–1.0).
    pub target_density: f32,
    /// Whether the maze must be fully connected.
    pub connected: bool,
    /// Whether the maze has 180° rotational symmetry.
    pub symmetry: bool,
    /// Probability that a passage has mud (0.0–1.0).
    pub mud_density: f32,
    /// Maximum mud traversal cost (minimum is 2).
    pub mud_range: u8,
}

impl MazeParams {
    /// Classic maze: 0.7 wall density, 0.1 mud density, connected, symmetric.
    pub fn classic() -> Self {
        Self::default()
    }

    /// Open maze: no walls, no mud.
    pub fn open() -> Self {
        Self {
            target_density: 0.0,
            mud_density: 0.0,
            mud_range: 2,
            ..Self::default()
        }
    }
}

impl Default for MazeParams {
    fn default() -> Self {
        Self {
            target_density: 0.7,
            connected: true,
            symmetry: true,
            mud_density: 0.1,
            mud_range: 3,
        }
    }
}

// ---------------------------------------------------------------------------
// GameBuilder<State>
// ---------------------------------------------------------------------------

/// Typestate builder that assembles a [`GameConfig`].
///
/// The generic `State` parameter tracks the build phase at compile time:
/// `NeedsMaze` → `NeedsPlayers` → `NeedsCheese` → `Ready`.
pub struct GameBuilder<State> {
    width: u8,
    height: u8,
    max_turns: u16,
    maze: Option<MazeStrategy>,
    players: Option<PlayerStrategy>,
    cheese: Option<CheeseStrategy>,
    _state: PhantomData<State>,
}

// -- Methods available in any state --

impl<S> GameBuilder<S> {
    /// Override the default max_turns (300).
    ///
    /// # Panics
    /// Panics if `n == 0`.
    #[must_use]
    pub fn with_max_turns(mut self, n: u16) -> Self {
        assert!(n > 0, "max_turns must be > 0");
        self.max_turns = n;
        self
    }
}

// -- NeedsMaze: entry point --

impl GameBuilder<NeedsMaze> {
    /// Start building a game with the given board dimensions.
    ///
    /// # Panics
    /// Panics if `width < 2` or `height < 2`.
    #[must_use]
    pub fn new(width: u8, height: u8) -> Self {
        assert!(width >= 2, "width must be >= 2, got {width}");
        assert!(height >= 2, "height must be >= 2, got {height}");
        Self {
            width,
            height,
            max_turns: 300,
            maze: None,
            players: None,
            cheese: None,
            _state: PhantomData,
        }
    }

    /// Use random maze generation with the given parameters.
    #[must_use]
    pub fn with_random_maze(self, params: MazeParams) -> GameBuilder<NeedsPlayers> {
        GameBuilder {
            width: self.width,
            height: self.height,
            max_turns: self.max_turns,
            maze: Some(MazeStrategy::Random(params)),
            players: None,
            cheese: None,
            _state: PhantomData,
        }
    }

    /// Use a classic maze (0.7 wall density, 0.1 mud density).
    #[must_use]
    pub fn with_classic_maze(self) -> GameBuilder<NeedsPlayers> {
        self.with_random_maze(MazeParams::classic())
    }

    /// Use an open maze (no walls, no mud).
    #[must_use]
    pub fn with_open_maze(self) -> GameBuilder<NeedsPlayers> {
        self.with_random_maze(MazeParams::open())
    }

    /// Use a fixed wall/mud layout.
    #[must_use]
    pub fn with_custom_maze(
        self,
        walls: HashMap<Coordinates, Vec<Coordinates>>,
        mud: MudMap,
    ) -> GameBuilder<NeedsPlayers> {
        GameBuilder {
            width: self.width,
            height: self.height,
            max_turns: self.max_turns,
            maze: Some(MazeStrategy::Fixed { walls, mud }),
            players: None,
            cheese: None,
            _state: PhantomData,
        }
    }
}

// -- NeedsPlayers --

impl GameBuilder<NeedsPlayers> {
    /// Player 1 at (0,0), player 2 at (width-1, height-1).
    #[must_use]
    pub fn with_corner_positions(self) -> GameBuilder<NeedsCheese> {
        GameBuilder {
            width: self.width,
            height: self.height,
            max_turns: self.max_turns,
            maze: self.maze,
            players: Some(PlayerStrategy::Corners),
            cheese: None,
            _state: PhantomData,
        }
    }

    /// Place both players randomly (guaranteed different cells).
    #[must_use]
    pub fn with_random_positions(self) -> GameBuilder<NeedsCheese> {
        GameBuilder {
            width: self.width,
            height: self.height,
            max_turns: self.max_turns,
            maze: self.maze,
            players: Some(PlayerStrategy::Random),
            cheese: None,
            _state: PhantomData,
        }
    }

    /// Place players at explicit positions.
    #[must_use]
    pub fn with_custom_positions(
        self,
        p1: Coordinates,
        p2: Coordinates,
    ) -> GameBuilder<NeedsCheese> {
        GameBuilder {
            width: self.width,
            height: self.height,
            max_turns: self.max_turns,
            maze: self.maze,
            players: Some(PlayerStrategy::Fixed(p1, p2)),
            cheese: None,
            _state: PhantomData,
        }
    }
}

// -- NeedsCheese --

impl GameBuilder<NeedsCheese> {
    /// Place `count` cheese randomly, optionally with 180° symmetry.
    #[must_use]
    pub fn with_random_cheese(self, count: u16, symmetric: bool) -> GameBuilder<Ready> {
        GameBuilder {
            width: self.width,
            height: self.height,
            max_turns: self.max_turns,
            maze: self.maze,
            players: self.players,
            cheese: Some(CheeseStrategy::Random { count, symmetric }),
            _state: PhantomData,
        }
    }

    /// Place cheese at exact positions.
    #[must_use]
    pub fn with_custom_cheese(self, positions: Vec<Coordinates>) -> GameBuilder<Ready> {
        GameBuilder {
            width: self.width,
            height: self.height,
            max_turns: self.max_turns,
            maze: self.maze,
            players: self.players,
            cheese: Some(CheeseStrategy::Fixed(positions)),
            _state: PhantomData,
        }
    }
}

// -- Ready --

impl GameBuilder<Ready> {
    /// Consume the builder and produce a reusable [`GameConfig`].
    #[must_use]
    pub fn build(self) -> GameConfig {
        GameConfig {
            width: self.width,
            height: self.height,
            max_turns: self.max_turns,
            maze: self.maze.expect("maze strategy set in NeedsMaze phase"),
            players: self
                .players
                .expect("player strategy set in NeedsPlayers phase"),
            cheese: self
                .cheese
                .expect("cheese strategy set in NeedsCheese phase"),
        }
    }
}

// ---------------------------------------------------------------------------
// GameConfig
// ---------------------------------------------------------------------------

/// A reusable game configuration. Call [`create`](Self::create) to stamp out
/// `GameState` instances — each call can use a different seed.
#[derive(Clone)]
pub struct GameConfig {
    pub width: u8,
    pub height: u8,
    pub max_turns: u16,
    pub maze: MazeStrategy,
    pub players: PlayerStrategy,
    pub cheese: CheeseStrategy,
}

impl GameConfig {
    /// Standard game: classic maze, corner starts, symmetric random cheese.
    pub fn classic(width: u8, height: u8, cheese: u16) -> Self {
        GameBuilder::new(width, height)
            .with_classic_maze()
            .with_corner_positions()
            .with_random_cheese(cheese, true)
            .build()
    }

    /// Create a `GameState` from this config.
    ///
    /// `seed` controls all random generation. `None` uses OS entropy.
    /// Fixed strategies ignore the seed.
    pub fn create(&self, seed: Option<u64>) -> GameState {
        let mut rng: rand::rngs::StdRng =
            seed.map_or_else(rand::make_rng, SeedableRng::seed_from_u64);

        // 1. Maze
        let (walls, mud) = match &self.maze {
            MazeStrategy::Fixed { walls, mud } => (walls.clone(), mud.clone()),
            MazeStrategy::Random(params) => {
                let maze_config = MazeConfig {
                    width: self.width,
                    height: self.height,
                    target_density: params.target_density,
                    connected: params.connected,
                    symmetry: params.symmetry,
                    mud_density: params.mud_density,
                    mud_range: params.mud_range,
                    seed: Some(rng.random()),
                };
                let mut gen = MazeGenerator::new(maze_config);
                gen.generate()
            },
        };

        // 2. Players
        let (p1, p2) = match &self.players {
            PlayerStrategy::Corners => (
                Coordinates::new(0, 0),
                Coordinates::new(self.width - 1, self.height - 1),
            ),
            PlayerStrategy::Random => generate_random_positions(self.width, self.height, &mut rng),
            PlayerStrategy::Fixed(p1, p2) => (*p1, *p2),
        };

        // 3. Cheese
        let cheese_positions = match &self.cheese {
            CheeseStrategy::Fixed(positions) => positions.clone(),
            CheeseStrategy::Random { count, symmetric } => {
                let cheese_config = CheeseConfig {
                    count: *count,
                    symmetry: *symmetric,
                };
                let mut cheese_gen = CheeseGenerator::new(
                    cheese_config,
                    self.width,
                    self.height,
                    Some(rng.random()),
                );
                cheese_gen.generate(p1, p2)
            },
        };

        // 4. Assemble
        GameState::new_with_config(
            self.width,
            self.height,
            walls,
            mud,
            &cheese_positions,
            p1,
            p2,
            self.max_turns,
        )
    }

    /// Look up a named preset configuration.
    ///
    /// Presets combine a **size** with a **maze type**:
    ///
    /// | Size     | Board  | Cheese | Turns | Maze type |
    /// |----------|--------|--------|-------|-----------|
    /// | `tiny`   | 11×9   | 13     | 150   | classic   |
    /// | `small`  | 15×11  | 21     | 200   | classic   |
    /// | `medium` | 21×15  | 41     | 300   | classic   |
    /// | `large`  | 31×21  | 85     | 400   | classic   |
    /// | `huge`   | 41×31  | 165    | 500   | classic   |
    /// | `open`   | 21×15  | 41     | 300   | open      |
    /// | `asymmetric` | 21×15 | 41  | 300   | classic (no symmetry) |
    ///
    /// **Maze types:** *classic* = 0.7 wall density, 0.1 mud density;
    /// *open* = no walls, no mud.
    pub fn preset(name: &str) -> Result<Self, String> {
        let (width, height, cheese, max_turns, symmetry, maze_params) = match name {
            "tiny" => (11, 9, 13, 150, true, MazeParams::classic()),
            "small" => (15, 11, 21, 200, true, MazeParams::classic()),
            "medium" => (21, 15, 41, 300, true, MazeParams::classic()),
            "large" => (31, 21, 85, 400, true, MazeParams::classic()),
            "huge" => (41, 31, 165, 500, true, MazeParams::classic()),
            "open" => (21, 15, 41, 300, true, MazeParams::open()),
            "asymmetric" => (
                21,
                15,
                41,
                300,
                false,
                MazeParams {
                    symmetry: false,
                    ..MazeParams::classic()
                },
            ),
            _ => {
                return Err(format!(
                    "Unknown preset '{name}'. Available: tiny, small, medium, large, huge, open, asymmetric"
                ))
            }
        };

        Ok(GameBuilder::new(width, height)
            .with_max_turns(max_turns)
            .with_random_maze(MazeParams {
                symmetry,
                ..maze_params
            })
            .with_corner_positions()
            .with_random_cheese(cheese, symmetry)
            .build())
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Pick two distinct random positions on the board.
fn generate_random_positions(
    width: u8,
    height: u8,
    rng: &mut impl Rng,
) -> (Coordinates, Coordinates) {
    let p1 = Coordinates::new(rng.random_range(0..width), rng.random_range(0..height));
    loop {
        let p2 = Coordinates::new(rng.random_range(0..width), rng.random_range(0..height));
        if p2 != p1 {
            return (p1, p2);
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builder_chain_compiles_and_produces_config() {
        let config = GameBuilder::new(21, 15)
            .with_random_maze(MazeParams::default())
            .with_corner_positions()
            .with_random_cheese(41, true)
            .build();

        assert_eq!(config.width, 21);
        assert_eq!(config.height, 15);
        assert_eq!(config.max_turns, 300);
    }

    #[test]
    fn with_max_turns_overrides_default() {
        let config = GameBuilder::new(21, 15)
            .with_max_turns(500)
            .with_random_maze(MazeParams::default())
            .with_corner_positions()
            .with_random_cheese(41, true)
            .build();

        assert_eq!(config.max_turns, 500);
    }

    #[test]
    fn fixed_strategies_produce_deterministic_game() {
        let mut walls = HashMap::new();
        walls.insert(Coordinates::new(0, 0), vec![Coordinates::new(1, 0)]);
        walls.insert(Coordinates::new(1, 0), vec![Coordinates::new(0, 0)]);

        let mut mud = MudMap::new();
        mud.insert(Coordinates::new(1, 1), Coordinates::new(1, 2), 2);

        let cheese = vec![Coordinates::new(1, 1), Coordinates::new(2, 2)];

        let config = GameBuilder::new(3, 3)
            .with_custom_maze(walls, mud)
            .with_custom_positions(Coordinates::new(0, 0), Coordinates::new(2, 2))
            .with_custom_cheese(cheese)
            .build();

        // Seed shouldn't matter for fully-fixed config
        let game1 = config.create(Some(1));
        let game2 = config.create(Some(999));

        assert_eq!(game1.width, 3);
        assert_eq!(game1.height, 3);
        assert_eq!(game1.cheese.total_cheese(), 2);
        assert_eq!(game1.player1_position(), Coordinates::new(0, 0));
        assert_eq!(game1.player2_position(), Coordinates::new(2, 2));
        assert!(game1
            .mud
            .contains(Coordinates::new(1, 1), Coordinates::new(1, 2)));

        // Same game regardless of seed
        assert_eq!(
            game1.cheese.get_all_cheese_positions(),
            game2.cheese.get_all_cheese_positions()
        );
        assert_eq!(game1.player1_position(), game2.player1_position());
    }

    #[test]
    fn same_seed_same_game() {
        let config = GameBuilder::new(11, 9)
            .with_random_maze(MazeParams::default())
            .with_corner_positions()
            .with_random_cheese(13, true)
            .build();

        let game1 = config.create(Some(42));
        let game2 = config.create(Some(42));

        assert_eq!(
            game1.cheese.get_all_cheese_positions(),
            game2.cheese.get_all_cheese_positions()
        );
    }

    #[test]
    fn different_seeds_different_games() {
        let config = GameBuilder::new(21, 15)
            .with_random_maze(MazeParams::default())
            .with_corner_positions()
            .with_random_cheese(41, true)
            .build();

        let game1 = config.create(Some(1));
        let game2 = config.create(Some(2));

        // Overwhelmingly likely to differ
        assert_ne!(
            game1.cheese.get_all_cheese_positions(),
            game2.cheese.get_all_cheese_positions()
        );
    }

    #[test]
    fn random_positions_in_bounds_and_differ() {
        let config = GameBuilder::new(5, 5)
            .with_random_maze(MazeParams {
                target_density: 0.0,
                mud_density: 0.0,
                ..MazeParams::default()
            })
            .with_random_positions()
            .with_random_cheese(4, false)
            .build();

        for seed in 0..20 {
            let game = config.create(Some(seed));
            let p1 = game.player1_position();
            let p2 = game.player2_position();

            assert!(p1.x < 5 && p1.y < 5, "p1 out of bounds: {p1:?}");
            assert!(p2.x < 5 && p2.y < 5, "p2 out of bounds: {p2:?}");
            assert_ne!(p1, p2, "players must not overlap (seed={seed})");
        }
    }

    #[test]
    fn random_positions_seeded_reproducibility() {
        let config = GameBuilder::new(10, 10)
            .with_random_maze(MazeParams::default())
            .with_random_positions()
            .with_random_cheese(10, true)
            .build();

        let game1 = config.create(Some(42));
        let game2 = config.create(Some(42));

        assert_eq!(game1.player1_position(), game2.player1_position());
        assert_eq!(game1.player2_position(), game2.player2_position());
    }

    #[test]
    fn all_presets_work() {
        for name in [
            "tiny",
            "small",
            "medium",
            "large",
            "huge",
            "open",
            "asymmetric",
        ] {
            let config =
                GameConfig::preset(name).unwrap_or_else(|e| panic!("preset '{name}': {e}"));
            let game = config.create(Some(42));
            assert!(
                game.cheese.total_cheese() > 0,
                "preset '{name}' has no cheese"
            );
        }
    }

    #[test]
    fn preset_invalid_name_errors() {
        assert!(GameConfig::preset("nonexistent").is_err());
    }

    #[test]
    fn preset_values_match_expected() {
        let config = GameConfig::preset("tiny").unwrap();
        assert_eq!(config.width, 11);
        assert_eq!(config.height, 9);
        assert_eq!(config.max_turns, 150);

        let config = GameConfig::preset("medium").unwrap();
        assert_eq!(config.width, 21);
        assert_eq!(config.height, 15);
        assert_eq!(config.max_turns, 300);
    }

    #[test]
    fn mixed_strategies() {
        // Random maze + fixed positions + random cheese
        let config = GameBuilder::new(11, 9)
            .with_random_maze(MazeParams::default())
            .with_custom_positions(Coordinates::new(2, 2), Coordinates::new(8, 6))
            .with_random_cheese(10, true)
            .build();

        let game = config.create(Some(42));
        assert_eq!(game.player1_position(), Coordinates::new(2, 2));
        assert_eq!(game.player2_position(), Coordinates::new(8, 6));
        assert_eq!(game.cheese.total_cheese(), 10);
    }

    #[test]
    fn corner_positions_match_expected() {
        let config = GameBuilder::new(21, 15)
            .with_random_maze(MazeParams::default())
            .with_corner_positions()
            .with_random_cheese(41, true)
            .build();

        let game = config.create(Some(42));
        assert_eq!(game.player1_position(), Coordinates::new(0, 0));
        assert_eq!(game.player2_position(), Coordinates::new(20, 14));
    }

    #[test]
    #[should_panic(expected = "width must be >= 2")]
    fn zero_width_panics() {
        let _ = GameBuilder::new(0, 5);
    }

    #[test]
    #[should_panic(expected = "height must be >= 2")]
    fn zero_height_panics() {
        let _ = GameBuilder::new(5, 0);
    }

    #[test]
    #[should_panic(expected = "width must be >= 2")]
    fn one_by_one_width_panics() {
        let _ = GameBuilder::new(1, 5);
    }

    #[test]
    #[should_panic(expected = "height must be >= 2")]
    fn one_by_one_height_panics() {
        let _ = GameBuilder::new(5, 1);
    }

    #[test]
    #[should_panic(expected = "max_turns must be > 0")]
    fn zero_max_turns_panics() {
        let _ = GameBuilder::new(5, 5).with_max_turns(0);
    }

    #[test]
    fn two_by_two_board_works() {
        let config = GameBuilder::new(2, 2)
            .with_open_maze()
            .with_corner_positions()
            .with_custom_cheese(vec![Coordinates::new(1, 0)])
            .build();
        let game = config.create(Some(42));
        assert_eq!(game.width, 2);
        assert_eq!(game.height, 2);
        assert_eq!(game.player1_position(), Coordinates::new(0, 0));
        assert_eq!(game.player2_position(), Coordinates::new(1, 1));
    }

    #[test]
    fn custom_maze_empty_walls() {
        let config = GameBuilder::new(3, 3)
            .with_custom_maze(HashMap::new(), MudMap::new())
            .with_corner_positions()
            .with_custom_cheese(vec![Coordinates::new(1, 1)])
            .build();

        let game = config.create(None);
        assert_eq!(game.width, 3);
        assert_eq!(game.height, 3);
        assert_eq!(game.cheese.total_cheese(), 1);
        assert!(game.mud.is_empty());
    }
}
