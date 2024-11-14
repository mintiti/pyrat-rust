use crate::benches::profile_lib::{ProfilingConfig, run_profiling, print_profiling_results};
use pyrat::{GameState, Direction};
use rand::Rng;

#[path = "../mod.rs"]
mod benches;

fn generate_random_moves(count: usize) -> Vec<(Direction, Direction)> {
    let mut rng = rand::thread_rng();
    let mut moves = Vec::with_capacity(count);

    for _ in 0..count {
        let p1_move = match rng.gen_range(0..5) {
            0 => Direction::Up,
            1 => Direction::Right,
            2 => Direction::Down,
            3 => Direction::Left,
            _ => Direction::Stay,
        };
        let p2_move = match rng.gen_range(0..5) {
            0 => Direction::Up,
            1 => Direction::Right,
            2 => Direction::Down,
            3 => Direction::Left,
            _ => Direction::Stay,
        };
        moves.push((p1_move, p2_move));
    }
    moves
}

fn main() {
    // First create a single game instance outside the profiling
    let game = GameState::new_symmetric(
        Some(21),
        Some(15),
        Some(41),
        Some(42),  // Fixed seed for reproducibility
    );

    // Pre-generate random moves
    let max_moves = 300; // Maximum moves per game
    let iterations = 100_000; // Number of games to simulate
    println!("Generating {} random moves...", max_moves * iterations);
    let random_moves = generate_random_moves(max_moves * iterations);

    // Now profile just the game loop
    println!("Profiling game loop performance...");
    let start = std::time::Instant::now();
    let mut total_moves = 0;
    let mut move_idx = 0;

    // Start perf sampling here
    for _ in 0..iterations {
        let mut game_copy = game.clone();
        let mut game_moves = 0;

        while !game_copy.process_turn(
            random_moves[move_idx].0,
            random_moves[move_idx].1,
        ).game_over {
            game_moves += 1;
            move_idx += 1;
            if move_idx >= random_moves.len() {
                move_idx = 0; // Wrap around if we run out of moves
            }
        }
        total_moves += game_moves;
    }

    let elapsed = start.elapsed();
    let moves_per_sec = (total_moves as f64) / elapsed.as_secs_f64();

    println!("\n=== Game Loop Performance ===");
    println!("Games simulated: {}", iterations);
    println!("Total moves: {}", total_moves);
    println!("Time: {:.2}s", elapsed.as_secs_f64());
    println!("Moves per second: {:.2}", moves_per_sec);
    println!("Average game length: {:.2}", total_moves as f64 / iterations as f64);
    println!("============================");
}