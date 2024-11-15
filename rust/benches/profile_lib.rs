//! Profiling utilities for PyRat engine performance analysis
use pyrat::{Direction, GameState};
use rand::Rng;
use std::time::Instant; // Adjust the crate name as needed

/// Configuration for profiling runs
#[derive(Clone, Debug)]
pub struct ProfilingConfig {
    /// Number of games to simulate
    pub num_games: u32,
    /// Maximum moves per game
    pub max_moves: u16,
    /// Width of the maze
    pub width: u8,
    /// Height of the maze
    pub height: u8,
    /// Number of cheese pieces
    pub cheese_count: u16,
    /// Whether to use symmetric mazes
    pub symmetric: bool,
}

impl Default for ProfilingConfig {
    fn default() -> Self {
        Self {
            num_games: 50_000,
            max_moves: 300,
            width: GameState::DEFAULT_WIDTH,
            height: GameState::DEFAULT_HEIGHT,
            cheese_count: GameState::DEFAULT_CHEESE_COUNT,
            symmetric: true,
        }
    }
}

/// Statistics collected during profiling
#[derive(Debug, Default)]
pub struct ProfilingStats {
    pub total_moves: u64,
    pub total_time_ms: u128,
    pub moves_per_second: f64,
    pub avg_game_length: f64,
    pub min_game_length: u16,
    pub max_game_length: u16,
}

/// Run profiling with the given configuration
pub fn run_profiling(config: ProfilingConfig) -> ProfilingStats {
    let mut rng = rand::thread_rng();
    let mut stats = ProfilingStats {
        min_game_length: u16::MAX,
        ..Default::default()
    };

    let start_time = Instant::now();

    for game_num in 0..config.num_games {
        // Create a new game with a different seed for each run
        let seed = game_num as u64;
        let mut game = if config.symmetric {
            GameState::new_symmetric(
                Some(config.width),
                Some(config.height),
                Some(config.cheese_count),
                Some(seed),
            )
        } else {
            GameState::new_asymmetric(
                Some(config.width),
                Some(config.height),
                Some(config.cheese_count),
                Some(seed),
            )
        };

        // Play random moves until game ends
        let mut moves = 0;
        while !game
            .process_turn(random_direction(&mut rng), random_direction(&mut rng))
            .game_over
        {
            moves += 1;
        }

        // Update statistics
        stats.total_moves += moves as u64;
        stats.min_game_length = stats.min_game_length.min(moves);
        stats.max_game_length = stats.max_game_length.max(moves);
    }

    // Calculate final statistics
    stats.total_time_ms = start_time.elapsed().as_millis();
    stats.moves_per_second = (stats.total_moves as f64 * 1000.0) / stats.total_time_ms as f64;
    stats.avg_game_length = stats.total_moves as f64 / config.num_games as f64;

    stats
}

/// Generate a random direction
#[inline]
fn random_direction(rng: &mut impl Rng) -> Direction {
    match rng.gen_range(0..5) {
        0 => Direction::Up,
        1 => Direction::Right,
        2 => Direction::Down,
        3 => Direction::Left,
        _ => Direction::Stay,
    }
}

/// Print profiling results in a formatted way
pub fn print_profiling_results(name: &str, stats: &ProfilingStats) {
    println!("\n=== PyRat Engine Profile: {} ===", name);
    println!("Total moves processed: {}", stats.total_moves);
    println!("Total time: {:.2}s", stats.total_time_ms as f64 / 1000.0);
    println!("Moves per second: {:.2}", stats.moves_per_second);
    println!("Average game length: {:.2}", stats.avg_game_length);
    println!("Min game length: {}", stats.min_game_length);
    println!("Max game length: {}", stats.max_game_length);
    println!("=====================================");
}
