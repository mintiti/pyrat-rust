//! Typestate builder for game creation.
//!
//! The builder enforces a compile-time sequence: maze → players → cheese.
//! Once built, `GameConfig` can stamp out `GameState` instances via
//! `create(Option<u64>)`, enabling reuse for RL training loops.
//!
//! ```rust,no_run
//! use pyrat::{GameBuilder, GameConfig, MazeParams};
//!
//! // Quick classic game
//! let config = GameConfig::classic(21, 15, 41);
//! let game = config.create(Some(42)).unwrap();
//!
//! // Builder with named maze constructors
//! let config = GameBuilder::new(21, 15)
//!     .with_classic_maze()
//!     .with_corner_positions()
//!     .with_random_cheese(41, true)
//!     .build();
//!
//! let game1 = config.create(Some(42)).unwrap();
//! let game2 = config.create(Some(43)).unwrap();
//!
//! // Reuse one generated maze across independent games
//! let maze = config.generate_maze(Some(42));
//! let game3 = config.create_with_maze(&maze, Some(7)).unwrap();
//! ```

use crate::game::maze_generation::{CheeseConfig, CheeseGenerator, MazeConfig, MazeGenerator};
use crate::game::types::MudMap;
use crate::game::zobrist;
use crate::{Coordinates, GameState, MazeLayout, MoveTable};
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
    pub wall_density: f32,
    /// Whether the maze must be fully connected.
    pub connected: bool,
    /// Whether the maze has 180° rotational symmetry.
    pub symmetric: bool,
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
            wall_density: 0.0,
            mud_density: 0.0,
            mud_range: 2,
            ..Self::default()
        }
    }
}

impl Default for MazeParams {
    fn default() -> Self {
        Self {
            wall_density: 0.7,
            connected: true,
            symmetric: true,
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
    ///
    /// # Panics
    /// Panics if densities are outside `[0.0, 1.0]` or `mud_range < 2`
    /// when `mud_density > 0`.
    #[must_use]
    pub fn with_random_maze(self, params: MazeParams) -> GameBuilder<NeedsPlayers> {
        assert!(
            (0.0..=1.0).contains(&params.wall_density),
            "wall_density must be between 0.0 and 1.0, got {}",
            params.wall_density
        );
        assert!(
            (0.0..=1.0).contains(&params.mud_density),
            "mud_density must be between 0.0 and 1.0, got {}",
            params.mud_density
        );
        if params.mud_density > 0.0 {
            assert!(
                params.mud_range >= 2,
                "mud_range must be >= 2 when mud_density > 0, got {}",
                params.mud_range
            );
        }
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
    ///
    /// # Panics
    /// Panics if `count == 0`.
    #[must_use]
    pub fn with_random_cheese(self, count: u16, symmetric: bool) -> GameBuilder<Ready> {
        assert!(count > 0, "cheese count must be > 0");
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
        GameConfig::from_parts(
            self.width,
            self.height,
            self.max_turns,
            self.maze.expect("maze strategy set in NeedsMaze phase"),
            self.players
                .expect("player strategy set in NeedsPlayers phase"),
            self.cheese
                .expect("cheese strategy set in NeedsCheese phase"),
        )
    }
}

// ---------------------------------------------------------------------------
// GameConfig
// ---------------------------------------------------------------------------

/// A reusable game configuration. Call [`create`](Self::create) to stamp out
/// `GameState` instances — each call can use a different seed.
#[derive(Clone)]
pub struct GameConfig {
    width: u8,
    height: u8,
    max_turns: u16,
    maze: MazeStrategy,
    players: PlayerStrategy,
    cheese: CheeseStrategy,
    fixed_maze: Option<MazeLayout>,
}

impl GameConfig {
    pub(crate) fn from_parts(
        width: u8,
        height: u8,
        max_turns: u16,
        maze: MazeStrategy,
        players: PlayerStrategy,
        cheese: CheeseStrategy,
    ) -> Self {
        let fixed_maze = match &maze {
            MazeStrategy::Fixed { walls, mud } => {
                Some(MazeLayout::from_walls(width, height, walls, mud.clone()))
            },
            MazeStrategy::Random(_) => None,
        };

        Self {
            width,
            height,
            max_turns,
            maze,
            players,
            cheese,
            fixed_maze,
        }
    }

    pub fn width(&self) -> u8 {
        self.width
    }
    pub fn height(&self) -> u8 {
        self.height
    }
    pub fn max_turns(&self) -> u16 {
        self.max_turns
    }
    pub fn maze(&self) -> &MazeStrategy {
        &self.maze
    }
    pub fn players(&self) -> &PlayerStrategy {
        &self.players
    }
    pub fn cheese(&self) -> &CheeseStrategy {
        &self.cheese
    }

    /// Standard game: classic maze, corner starts, symmetric random cheese.
    pub fn classic(width: u8, height: u8, cheese: u16) -> Self {
        GameBuilder::new(width, height)
            .with_classic_maze()
            .with_corner_positions()
            .with_random_cheese(cheese, true)
            .build()
    }

    /// Override the max_turns on an already-built config.
    ///
    /// Useful for callers that obtained the config from a preset or a shortcut
    /// like [`Self::classic`] and want to change the turn limit without
    /// rebuilding the maze, players, and cheese strategies.
    ///
    /// # Panics
    /// Panics if `n == 0`.
    #[must_use]
    pub fn with_max_turns(mut self, n: u16) -> Self {
        assert!(n > 0, "max_turns must be > 0");
        self.max_turns = n;
        self
    }

    /// Generate and compile this config's maze topology for reuse.
    ///
    /// For random maze strategies, `seed` selects the maze. Fixed-maze
    /// configs return their already compiled layout and ignore the seed.
    #[must_use]
    pub fn generate_maze(&self, seed: Option<u64>) -> MazeLayout {
        if matches!(&self.maze, MazeStrategy::Fixed { .. }) {
            return self
                .fixed_maze
                .as_ref()
                .expect("fixed maze compiled when GameConfig was built")
                .clone();
        }

        let mut rng: rand::rngs::StdRng =
            seed.map_or_else(rand::make_rng, SeedableRng::seed_from_u64);
        self.generate_maze_with_rng(&mut rng)
    }

    /// Create a `GameState` from this config.
    ///
    /// `seed` controls all random generation. `None` uses OS entropy. The
    /// one-shot path remains equivalent to generating a maze and then calling
    /// [`create_with_maze`](Self::create_with_maze) with the same explicit
    /// seed.
    ///
    /// # Errors
    /// Returns an error if cheese placement fails (e.g. too many cheese for
    /// the board, or odd symmetric cheese on an even board).
    pub fn create(&self, seed: Option<u64>) -> Result<GameState, String> {
        let mut rng: rand::rngs::StdRng =
            seed.map_or_else(rand::make_rng, SeedableRng::seed_from_u64);
        let maze = self.generate_maze_with_rng(&mut rng);
        self.create_dynamic(maze, &mut rng)
    }

    /// Create a game on an already generated maze.
    ///
    /// The supplied layout must have the same dimensions as this config.
    /// Player and cheese randomness follows the same parent RNG stream as
    /// [`create`](Self::create), so using the same explicit seed reproduces
    /// the one-shot game exactly.
    ///
    /// # Errors
    /// Returns an error when the layout dimensions differ from the config or
    /// cheese placement fails.
    pub fn create_with_maze(
        &self,
        maze: &MazeLayout,
        seed: Option<u64>,
    ) -> Result<GameState, String> {
        if maze.width() != self.width() || maze.height() != self.height() {
            return Err(format!(
                "Maze dimensions {}x{} do not match game config {}x{}",
                maze.width(),
                maze.height(),
                self.width(),
                self.height()
            ));
        }

        let mut rng: rand::rngs::StdRng =
            seed.map_or_else(rand::make_rng, SeedableRng::seed_from_u64);

        // A random-maze create consumes one draw to seed maze generation.
        // Preserve that stream position even though this layout is supplied.
        if matches!(&self.maze, MazeStrategy::Random(_)) {
            let _: u64 = rng.random();
        }

        self.create_dynamic(maze.clone(), &mut rng)
    }

    fn generate_maze_with_rng(&self, rng: &mut rand::rngs::StdRng) -> MazeLayout {
        match &self.maze {
            MazeStrategy::Fixed { .. } => self
                .fixed_maze
                .as_ref()
                .expect("fixed maze compiled when GameConfig was built")
                .clone(),
            MazeStrategy::Random(params) => {
                // Preserve the parent RNG stream even when the topology is predetermined.
                let maze_seed = rng.random();
                if params.wall_density == 0.0 && params.mud_density == 0.0 {
                    let move_table = MoveTable::open(self.width(), self.height());
                    let mud = MudMap::new();
                    let topology_hash =
                        zobrist::maze_hash(&move_table, &mud, self.width(), self.height());
                    MazeLayout::new(self.width(), self.height(), move_table, mud, topology_hash)
                } else {
                    let maze_config = MazeConfig {
                        width: self.width(),
                        height: self.height(),
                        target_density: params.wall_density,
                        connected: params.connected,
                        symmetry: params.symmetric,
                        mud_density: params.mud_density,
                        mud_range: params.mud_range,
                        seed: Some(maze_seed),
                    };
                    let (move_table, mud, topology_hash) =
                        MazeGenerator::new(maze_config).generate_runtime_topology();
                    MazeLayout::new(self.width(), self.height(), move_table, mud, topology_hash)
                }
            },
        }
    }

    fn create_dynamic(
        &self,
        maze: MazeLayout,
        rng: &mut rand::rngs::StdRng,
    ) -> Result<GameState, String> {
        // 1. Players
        let (p1, p2) = match &self.players {
            PlayerStrategy::Corners => (
                Coordinates::new(0, 0),
                Coordinates::new(self.width() - 1, self.height() - 1),
            ),
            PlayerStrategy::Random => generate_random_positions(self.width(), self.height(), rng),
            PlayerStrategy::Fixed(p1, p2) => (*p1, *p2),
        };

        // 2. Cheese
        let cheese_positions = match &self.cheese {
            CheeseStrategy::Fixed(positions) => positions.clone(),
            CheeseStrategy::Random { count, symmetric } => {
                let cheese_config = CheeseConfig {
                    count: *count,
                    symmetry: *symmetric,
                };
                let mut cheese_gen = CheeseGenerator::new(
                    cheese_config,
                    self.width(),
                    self.height(),
                    Some(rng.random()),
                );
                cheese_gen.generate(p1, p2)?
            },
        };

        // 3. Assemble
        Ok(GameState::new_with_layout(
            maze,
            &cheese_positions,
            p1,
            p2,
            self.max_turns(),
        ))
    }

    /// Look up a named preset configuration.
    ///
    /// Presets combine a **size** with a **maze type**:
    ///
    /// | Size     | Board  | Cheese | Turns | Maze type |
    /// |----------|--------|--------|-------|-----------|
    /// | `tiny`   | 11×9   | 13     | 150   | classic   |
    /// | `small`  | 15×13  | 21     | 200   | classic   |
    /// | `medium` | 21×15  | 41     | 300   | classic   |
    /// | `large`  | 31×21  | 85     | 400   | classic   |
    /// | `huge`   | 41×31  | 165    | 500   | classic   |
    /// | `open`   | 21×15  | 41     | 300   | open      |
    /// | `asymmetric` | 21×15 | 41  | 300   | classic (no symmetry) |
    ///
    /// **Maze types:** *classic* = 0.7 wall density, 0.1 mud density;
    /// *open* = no walls, no mud.
    pub fn preset(name: &str) -> Result<Self, String> {
        let (width, height, cheese, max_turns, symmetric, maze_params) = match name {
            "tiny" => (11, 9, 13, 150, true, MazeParams::classic()),
            "small" => (15, 13, 21, 200, true, MazeParams::classic()),
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
                    symmetric: false,
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
                symmetric,
                ..maze_params
            })
            .with_corner_positions()
            .with_random_cheese(cheese, symmetric)
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
    use crate::Direction;

    fn assert_same_game(left: &GameState, right: &GameState) {
        assert_eq!(left.width, right.width);
        assert_eq!(left.height, right.height);
        assert_eq!(left.max_turns, right.max_turns);
        assert_eq!(left.move_table.bytes(), right.move_table.bytes());

        let sorted_mud = |game: &GameState| {
            let mut entries: Vec<_> = game.mud.iter().collect();
            entries.sort_unstable();
            entries
        };
        assert_eq!(sorted_mud(left), sorted_mud(right));
        assert_eq!(left.player1_position(), right.player1_position());
        assert_eq!(left.player2_position(), right.player2_position());
        assert_eq!(left.player1_mud_turns(), right.player1_mud_turns());
        assert_eq!(left.player2_mud_turns(), right.player2_mud_turns());
        assert_eq!(left.player1_score(), right.player1_score());
        assert_eq!(left.player2_score(), right.player2_score());
        assert_eq!(left.cheese_positions(), right.cheese_positions());
        assert_eq!(left.turn, right.turn);
        assert_eq!(left.state_hash(), right.state_hash());
    }

    fn assert_same_trace(mut direct: GameState, mut reused: GameState) {
        let actions = [
            (Direction::Up, Direction::Left),
            (Direction::Right, Direction::Down),
            (Direction::Stay, Direction::Up),
            (Direction::Left, Direction::Right),
            (Direction::Down, Direction::Stay),
        ];

        assert_same_game(&direct, &reused);
        for turn in 0..24 {
            let (p1, p2) = actions[turn % actions.len()];
            let direct_result = direct.process_turn(p1, p2);
            let reused_result = reused.process_turn(p1, p2);

            assert_eq!(direct_result.p1_moved, reused_result.p1_moved);
            assert_eq!(direct_result.p2_moved, reused_result.p2_moved);
            assert_eq!(direct_result.game_over, reused_result.game_over);
            assert_eq!(direct_result.p1_score, reused_result.p1_score);
            assert_eq!(direct_result.p2_score, reused_result.p2_score);
            assert_eq!(
                direct_result.collected_cheese,
                reused_result.collected_cheese
            );
            assert_same_game(&direct, &reused);
        }
    }

    #[test]
    fn builder_chain_compiles_and_produces_config() {
        let config = GameBuilder::new(21, 15)
            .with_random_maze(MazeParams::default())
            .with_corner_positions()
            .with_random_cheese(41, true)
            .build();

        assert_eq!(config.width(), 21);
        assert_eq!(config.height(), 15);
        assert_eq!(config.max_turns(), 300);
    }

    #[test]
    fn with_max_turns_overrides_default() {
        let config = GameBuilder::new(21, 15)
            .with_max_turns(500)
            .with_random_maze(MazeParams::default())
            .with_corner_positions()
            .with_random_cheese(41, true)
            .build();

        assert_eq!(config.max_turns(), 500);
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
        let game1 = config.create(Some(1)).unwrap();
        let game2 = config.create(Some(999)).unwrap();

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

        let game1 = config.create(Some(42)).unwrap();
        let game2 = config.create(Some(42)).unwrap();

        let canonical_walls = |game: &GameState| {
            let mut walls: Vec<_> = game
                .wall_entries()
                .into_iter()
                .map(|wall| (wall.pos1, wall.pos2))
                .collect();
            walls.sort_unstable();
            walls
        };
        let canonical_mud = |game: &GameState| {
            let mut mud: Vec<_> = game
                .mud_positions()
                .iter()
                .map(|((first, second), cost)| (first, second, cost))
                .collect();
            mud.sort_unstable();
            mud
        };
        let canonical_cheese = |game: &GameState| {
            let mut cheese = game.cheese_positions();
            cheese.sort_unstable();
            cheese
        };

        assert_eq!(canonical_walls(&game1), canonical_walls(&game2));
        assert_eq!(canonical_mud(&game1), canonical_mud(&game2));
        assert_eq!(game1.player1_position(), game2.player1_position());
        assert_eq!(game1.player2_position(), game2.player2_position());
        assert_eq!(canonical_cheese(&game1), canonical_cheese(&game2));
        assert_eq!(game1.state_hash(), game2.state_hash());
    }

    #[test]
    fn generated_maze_path_matches_one_shot_creation() {
        let mut walls = HashMap::new();
        walls.insert(Coordinates::new(1, 1), vec![Coordinates::new(2, 1)]);
        walls.insert(Coordinates::new(2, 1), vec![Coordinates::new(1, 1)]);
        let mut mud = MudMap::new();
        mud.insert(Coordinates::new(2, 2), Coordinates::new(2, 3), 3);

        let configs = [
            GameBuilder::new(11, 9)
                .with_classic_maze()
                .with_corner_positions()
                .with_random_cheese(13, true)
                .build(),
            GameBuilder::new(8, 6)
                .with_open_maze()
                .with_random_positions()
                .with_random_cheese(10, false)
                .build(),
            GameBuilder::new(10, 8)
                .with_random_maze(MazeParams {
                    symmetric: false,
                    ..MazeParams::classic()
                })
                .with_custom_positions(Coordinates::new(1, 2), Coordinates::new(8, 5))
                .with_random_cheese(11, false)
                .build(),
            GameBuilder::new(5, 5)
                .with_custom_maze(walls, mud)
                .with_random_positions()
                .with_random_cheese(7, false)
                .build(),
        ];

        for config in configs {
            for seed in [0, 42, 0xA11C_E5E5] {
                let direct = config.create(Some(seed)).unwrap();
                let maze = config.generate_maze(Some(seed));
                let reused = config.create_with_maze(&maze, Some(seed)).unwrap();
                assert_same_trace(direct, reused);
            }
        }
    }

    #[test]
    fn generated_maze_path_preserves_create_errors() {
        let config = GameBuilder::new(3, 3)
            .with_open_maze()
            .with_corner_positions()
            .with_random_cheese(100, false)
            .build();
        let maze = config.generate_maze(Some(42));

        assert_eq!(
            config.create(Some(42)).unwrap_err().to_string(),
            config
                .create_with_maze(&maze, Some(42))
                .unwrap_err()
                .to_string()
        );
    }

    #[test]
    fn create_with_maze_rejects_dimension_mismatch() {
        let config = GameConfig::classic(11, 9, 13);
        let other_maze = GameConfig::classic(7, 5, 5).generate_maze(Some(42));

        let error = config.create_with_maze(&other_maze, Some(42)).unwrap_err();
        assert_eq!(error, "Maze dimensions 7x5 do not match game config 11x9");
    }

    #[test]
    fn games_share_topology_but_keep_mutation_and_undo_isolated() {
        let config = GameBuilder::new(5, 5)
            .with_open_maze()
            .with_custom_positions(Coordinates::new(0, 0), Coordinates::new(4, 4))
            .with_custom_cheese(vec![Coordinates::new(1, 0), Coordinates::new(3, 4)])
            .build();
        let maze = config.generate_maze(Some(91));
        let mut game_a = config.create_with_maze(&maze, Some(7)).unwrap();
        let game_b = config.create_with_maze(&maze, Some(7)).unwrap();

        assert!(game_a.move_table.shares_storage_with(maze.move_table()));
        assert!(game_b.move_table.shares_storage_with(&game_a.move_table));
        assert!(game_a.mud.shares_storage_with(maze.mud_positions()));
        assert!(game_b.mud.shares_storage_with(&game_a.mud));

        let untouched_hash = game_b.state_hash();
        let untouched_cheese = game_b.cheese_positions();
        let undo = game_a.make_move(Direction::Right, Direction::Stay);
        assert_ne!(game_a.state_hash(), untouched_hash);
        assert_eq!(game_b.state_hash(), untouched_hash);
        assert_eq!(game_b.cheese_positions(), untouched_cheese);
        game_a.unmake_move(undo);
        assert_same_game(&game_a, &game_b);

        game_a
            .mud
            .insert(Coordinates::new(0, 0), Coordinates::new(1, 0), 3);
        assert!(game_a
            .mud
            .contains(Coordinates::new(0, 0), Coordinates::new(1, 0)));
        assert!(!game_b
            .mud
            .contains(Coordinates::new(0, 0), Coordinates::new(1, 0)));
        assert!(!maze
            .mud_positions()
            .contains(Coordinates::new(0, 0), Coordinates::new(1, 0)));
        assert!(!game_a.mud.shares_storage_with(&game_b.mud));
        assert!(game_b.mud.shares_storage_with(maze.mud_positions()));
    }

    #[test]
    fn fixed_config_reuses_its_compiled_layout() {
        let mut mud = MudMap::new();
        mud.insert(Coordinates::new(1, 0), Coordinates::new(1, 1), 2);
        let config = GameBuilder::new(3, 3)
            .with_custom_maze(HashMap::new(), mud)
            .with_corner_positions()
            .with_custom_cheese(vec![Coordinates::new(1, 1)])
            .build();

        let maze_a = config.generate_maze(Some(1));
        let maze_b = config.generate_maze(Some(999));
        let game_a = config.create(Some(1)).unwrap();
        let game_b = config.create(Some(999)).unwrap();

        assert!(maze_a.shares_storage_with(&maze_b));
        assert!(game_a.move_table.shares_storage_with(maze_a.move_table()));
        assert!(game_b.move_table.shares_storage_with(&game_a.move_table));
        assert!(game_a.mud.shares_storage_with(maze_a.mud_positions()));
        assert!(game_b.mud.shares_storage_with(&game_a.mud));
    }

    #[test]
    fn one_layout_supports_identical_pairs_and_varied_dynamic_seeds() {
        let config = GameBuilder::new(9, 7)
            .with_classic_maze()
            .with_random_positions()
            .with_random_cheese(9, false)
            .build();
        let maze = config.generate_maze(Some(500));
        let pair_a = config.create_with_maze(&maze, Some(71)).unwrap();
        let pair_b = config.create_with_maze(&maze, Some(71)).unwrap();
        assert_same_game(&pair_a, &pair_b);

        let varied = config.create_with_maze(&maze, Some(72)).unwrap();
        assert!(pair_a.move_table.shares_storage_with(&varied.move_table));
        assert!(pair_a.mud.shares_storage_with(&varied.mud));
        assert_ne!(
            (
                pair_a.player1_position(),
                pair_a.player2_position(),
                pair_a.cheese_positions()
            ),
            (
                varied.player1_position(),
                varied.player2_position(),
                varied.cheese_positions()
            )
        );
    }

    #[test]
    fn different_seeds_different_games() {
        let config = GameBuilder::new(21, 15)
            .with_random_maze(MazeParams::default())
            .with_corner_positions()
            .with_random_cheese(41, true)
            .build();

        let game1 = config.create(Some(1)).unwrap();
        let game2 = config.create(Some(2)).unwrap();

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
                wall_density: 0.0,
                mud_density: 0.0,
                ..MazeParams::default()
            })
            .with_random_positions()
            .with_random_cheese(4, false)
            .build();

        for seed in 0..20 {
            let game = config.create(Some(seed)).unwrap();
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

        let game1 = config.create(Some(42)).unwrap();
        let game2 = config.create(Some(42)).unwrap();

        assert_eq!(game1.player1_position(), game2.player1_position());
        assert_eq!(game1.player2_position(), game2.player2_position());
    }

    #[test]
    fn open_seed_fixture_is_stable() {
        let config = GameBuilder::new(7, 5)
            .with_open_maze()
            .with_random_positions()
            .with_random_cheese(6, false)
            .build();

        let fixtures = [
            (
                0,
                Coordinates::new(3, 3),
                Coordinates::new(5, 0),
                vec![
                    Coordinates::new(4, 0),
                    Coordinates::new(6, 0),
                    Coordinates::new(5, 1),
                    Coordinates::new(5, 2),
                    Coordinates::new(6, 3),
                    Coordinates::new(1, 4),
                ],
                0xAFFC_C2CF_9E1D_D84E,
            ),
            (
                0xA11C_E5E5,
                Coordinates::new(3, 1),
                Coordinates::new(6, 3),
                vec![
                    Coordinates::new(5, 2),
                    Coordinates::new(1, 3),
                    Coordinates::new(2, 3),
                    Coordinates::new(0, 4),
                    Coordinates::new(5, 4),
                    Coordinates::new(6, 4),
                ],
                0xF27C_E9D7_0E7F_1C76,
            ),
        ];

        for (seed, player1, player2, cheese, state_hash) in fixtures {
            let game = config.create(Some(seed)).unwrap();
            assert!(game.wall_entries().is_empty(), "seed={seed:#x}");
            assert!(game.mud_positions().is_empty(), "seed={seed:#x}");
            assert_eq!(game.player1_position(), player1, "seed={seed:#x}");
            assert_eq!(game.player2_position(), player2, "seed={seed:#x}");
            assert_eq!(game.cheese_positions(), cheese, "seed={seed:#x}");
            assert_eq!(game.state_hash(), state_hash, "seed={seed:#x}");
        }
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
            let game = config.create(Some(42)).unwrap();
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
        assert_eq!(config.width(), 11);
        assert_eq!(config.height(), 9);
        assert_eq!(config.max_turns(), 150);

        let config = GameConfig::preset("medium").unwrap();
        assert_eq!(config.width(), 21);
        assert_eq!(config.height(), 15);
        assert_eq!(config.max_turns(), 300);
    }

    #[test]
    fn mixed_strategies() {
        // Random maze + fixed positions + random cheese
        let config = GameBuilder::new(11, 9)
            .with_random_maze(MazeParams::default())
            .with_custom_positions(Coordinates::new(2, 2), Coordinates::new(8, 6))
            .with_random_cheese(10, true)
            .build();

        let game = config.create(Some(42)).unwrap();
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

        let game = config.create(Some(42)).unwrap();
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
        let game = config.create(Some(42)).unwrap();
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

        let game = config.create(None).unwrap();
        assert_eq!(game.width, 3);
        assert_eq!(game.height, 3);
        assert_eq!(game.cheese.total_cheese(), 1);
        assert!(game.mud.is_empty());
    }

    #[test]
    fn config_with_max_turns_overrides() {
        let config = GameConfig::classic(11, 9, 7).with_max_turns(50);
        assert_eq!(config.max_turns(), 50);

        let config = GameConfig::preset("tiny").unwrap().with_max_turns(42);
        assert_eq!(config.max_turns(), 42);
    }

    #[test]
    #[should_panic(expected = "max_turns must be > 0")]
    fn config_with_max_turns_zero_panics() {
        let _ = GameConfig::classic(11, 9, 7).with_max_turns(0);
    }
}
