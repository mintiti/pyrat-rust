use pyrat::{Direction, GameState};
use std::hint::black_box;

fn main() {
    // Fixed sequence of moves
    let moves = [
        (Direction::Right, Direction::Left),
        (Direction::Down, Direction::Up),
        (Direction::Left, Direction::Right),
        (Direction::Up, Direction::Down),
        (Direction::Stay, Direction::Stay),
    ];
    
    // Run many games with the same move pattern
    for _ in 0..100_000 {  // 10k complete games
        let mut game = GameState::new_symmetric(
            Some(16),    // Standard game size
            Some(16),
            Some(40),    // Realistic cheese count
            Some(42),    // Same seed each time
        );

        let mut move_idx = 0;
        while !black_box(game.process_turn(moves[move_idx % moves.len()].0, moves[move_idx % moves.len()].1)).game_over {
            move_idx += 1;
        }
    }
}