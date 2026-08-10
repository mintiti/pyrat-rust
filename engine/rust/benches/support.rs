use pyrat::bench_scenarios::{BoardSize, FeatureCombo, COMBOS, SIZES};
use pyrat::game::maze_generation::{CheeseGenerator, MazeGenerator, WallMap};
use pyrat::{
    CheeseConfig, Coordinates, Direction, GameBuilder, GameConfig, GameState, MazeConfig,
    MazeLayout, MazeParams, MudMap,
};
use rand::{Rng, RngExt, SeedableRng};

pub(crate) const TURN_BATCH_LEN: usize = 64;
pub(crate) const SEED_TAPE_LEN: usize = 4;
pub(crate) const FIXED_CREATE_SEED: u64 = 0x5059_5241_545f_4649;

const ROOT_SEED: u64 = 0x5059_5241_545f_4245;
const GAME_STREAM: u64 = 0x4741_4d45;
const MAZE_STREAM: u64 = 0x4d41_5a45;
const CHEESE_STREAM: u64 = 0x4348_4545_5345;
const TURN_STREAM: u64 = 0x5455_524e;
const EPISODE_STREAM: u64 = 0x0045_5049_534f_4445;

pub(crate) type SeedTape = [u64; SEED_TAPE_LEN];

#[derive(Clone, Copy, Debug)]
pub(crate) struct ActionPair {
    pub(crate) p1: Direction,
    pub(crate) p2: Direction,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct ScenarioSeeds {
    pub(crate) game: u64,
    pub(crate) maze: u64,
    pub(crate) cheese: u64,
    pub(crate) turn_actions: u64,
    pub(crate) episode_actions: u64,
}

#[derive(Clone, Copy)]
pub(crate) struct ScenarioSpec {
    pub(crate) size: &'static BoardSize,
    pub(crate) combo: &'static FeatureCombo,
    pub(crate) seeds: ScenarioSeeds,
}

pub(crate) struct PreparedScenario {
    pub(crate) spec: ScenarioSpec,
    pub(crate) random_config: GameConfig,
    pub(crate) fixed_config: GameConfig,
    pub(crate) reused_maze: MazeLayout,
    pub(crate) walls: WallMap,
    pub(crate) mud: MudMap,
    pub(crate) initial_game: GameState,
    pub(crate) create_seeds: SeedTape,
    pub(crate) maze_seeds: SeedTape,
    pub(crate) cheese_seeds: SeedTape,
    pub(crate) turn_actions: Vec<ActionPair>,
    pub(crate) episode_actions: Vec<ActionPair>,
    pub(crate) episode_len: usize,
}

impl ScenarioSpec {
    pub(crate) fn random_config(self) -> GameConfig {
        GameBuilder::new(self.size.width, self.size.height)
            .with_max_turns(self.size.max_turns)
            .with_random_maze(MazeParams {
                wall_density: self.combo.wall_density,
                mud_density: self.combo.mud_density,
                ..MazeParams::default()
            })
            .with_corner_positions()
            .with_random_cheese(self.size.cheese, true)
            .build()
    }

    pub(crate) const fn maze_config(self, seed: u64) -> MazeConfig {
        MazeConfig {
            width: self.size.width,
            height: self.size.height,
            target_density: self.combo.wall_density,
            connected: true,
            symmetry: true,
            mud_density: self.combo.mud_density,
            mud_range: 3,
            seed: Some(seed),
        }
    }

    pub(crate) const fn cheese_config(self) -> CheeseConfig {
        CheeseConfig {
            count: self.size.cheese,
            symmetry: true,
        }
    }

    pub(crate) const fn corner_positions(self) -> (Coordinates, Coordinates) {
        (
            Coordinates::new(0, 0),
            Coordinates::new(self.size.width - 1, self.size.height - 1),
        )
    }

    pub(crate) const fn create_seed_tape(self) -> SeedTape {
        seed_tape(self.seeds.game)
    }

    pub(crate) const fn maze_seed_tape(self) -> SeedTape {
        seed_tape(self.seeds.maze)
    }

    pub(crate) const fn cheese_seed_tape(self) -> SeedTape {
        seed_tape(self.seeds.cheese)
    }

    pub(crate) fn turn_action_tape(self) -> Vec<ActionPair> {
        action_tape(self.seeds.turn_actions, TURN_BATCH_LEN)
    }

    pub(crate) fn episode_action_tape(self) -> Vec<ActionPair> {
        action_tape(self.seeds.episode_actions, usize::from(self.size.max_turns))
    }
}

impl PreparedScenario {
    fn new(spec: ScenarioSpec) -> Result<Self, String> {
        let random_config = spec.random_config();
        let reused_maze = random_config.generate_maze(Some(spec.seeds.game));

        let mut maze_generator = MazeGenerator::new(spec.maze_config(spec.seeds.maze));
        let (walls, mud) = maze_generator.generate();

        let (player1, player2) = spec.corner_positions();
        let mut cheese_generator = CheeseGenerator::new(
            spec.cheese_config(),
            spec.size.width,
            spec.size.height,
            Some(spec.seeds.cheese),
        );
        let cheese_positions = cheese_generator.generate(player1, player2)?;

        let fixed_config = GameBuilder::new(spec.size.width, spec.size.height)
            .with_max_turns(spec.size.max_turns)
            .with_custom_maze(walls.clone(), mud.clone())
            .with_custom_positions(player1, player2)
            .with_custom_cheese(cheese_positions)
            .build();
        let initial_game = fixed_config.create(Some(FIXED_CREATE_SEED))?;

        let turn_actions = spec.turn_action_tape();
        let mut turn_game = initial_game.clone();
        for (turn, action) in turn_actions.iter().enumerate() {
            if turn_game.process_turn(action.p1, action.p2).game_over {
                return Err(format!(
                    "{}/{} action seed {:#018x} reached game_over at turn {} of {}",
                    spec.size.name,
                    spec.combo.name,
                    spec.seeds.turn_actions,
                    turn + 1,
                    TURN_BATCH_LEN
                ));
            }
        }

        let episode_actions = spec.episode_action_tape();
        let mut episode_game = initial_game.clone();
        let episode_len = episode_actions
            .iter()
            .position(|action| episode_game.process_turn(action.p1, action.p2).game_over)
            .map_or(0, |index| index + 1);
        if episode_len == 0 {
            return Err(format!(
                "{}/{} episode seed {:#018x} did not reach game_over in {} turns",
                spec.size.name, spec.combo.name, spec.seeds.episode_actions, spec.size.max_turns
            ));
        }

        Ok(Self {
            spec,
            random_config,
            fixed_config,
            reused_maze,
            walls,
            mud,
            initial_game,
            create_seeds: spec.create_seed_tape(),
            maze_seeds: spec.maze_seed_tape(),
            cheese_seeds: spec.cheese_seed_tape(),
            turn_actions,
            episode_actions,
            episode_len,
        })
    }
}

pub(crate) fn scenario_specs() -> Vec<ScenarioSpec> {
    SIZES
        .iter()
        .flat_map(|size| {
            COMBOS.iter().map(move |combo| ScenarioSpec {
                size,
                combo,
                seeds: ScenarioSeeds {
                    game: scenario_seed(size.name, combo.name, GAME_STREAM),
                    maze: scenario_seed(size.name, combo.name, MAZE_STREAM),
                    cheese: size_seed(size.name, CHEESE_STREAM),
                    turn_actions: scenario_seed(size.name, combo.name, TURN_STREAM),
                    episode_actions: scenario_seed(size.name, combo.name, EPISODE_STREAM),
                },
            })
        })
        .collect()
}

pub(crate) fn prepared_scenarios() -> Result<Vec<PreparedScenario>, String> {
    scenario_specs()
        .into_iter()
        .map(PreparedScenario::new)
        .collect()
}

fn random_direction(rng: &mut impl Rng) -> Direction {
    match rng.random_range(0u8..5) {
        0 => Direction::Up,
        1 => Direction::Right,
        2 => Direction::Down,
        3 => Direction::Left,
        _ => Direction::Stay,
    }
}

fn action_tape(seed: u64, len: usize) -> Vec<ActionPair> {
    let mut rng = rand::rngs::StdRng::seed_from_u64(seed);
    (0..len)
        .map(|_| ActionPair {
            p1: random_direction(&mut rng),
            p2: random_direction(&mut rng),
        })
        .collect()
}

const fn seed_tape(seed: u64) -> SeedTape {
    let mut seeds = [0; SEED_TAPE_LEN];
    let mut index = 0;
    while index < SEED_TAPE_LEN {
        seeds[index] = splitmix64(seed.wrapping_add(index as u64));
        index += 1;
    }
    seeds
}

fn scenario_seed(size_name: &str, combo_name: &str, stream: u64) -> u64 {
    named_seed(
        size_name
            .bytes()
            .chain(std::iter::once(b'/'))
            .chain(combo_name.bytes()),
        stream,
    )
}

fn size_seed(size_name: &str, stream: u64) -> u64 {
    named_seed(size_name.bytes(), stream)
}

fn named_seed(bytes: impl Iterator<Item = u8>, stream: u64) -> u64 {
    let mut value = ROOT_SEED ^ stream;
    for byte in bytes {
        value ^= u64::from(byte);
        value = value.wrapping_mul(0x0000_0100_0000_01b3);
    }
    splitmix64(value)
}

const fn splitmix64(mut value: u64) -> u64 {
    value = value.wrapping_add(0x9e37_79b9_7f4a_7c15);
    value = (value ^ (value >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    value = (value ^ (value >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    value ^ (value >> 31)
}
