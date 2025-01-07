use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion};
use pyrat::{Coordinates, Direction, GameState};
use rand::{random, Rng};
use std::collections::HashMap;

// Board size configurations
const BENCHMARK_BOARD_SIZES: [u8; 5] = [8, 16, 32, 64, 200];

// Game parameters
const MAX_TURNS: u16 = 300;
const MOVES_PER_TEST: u8 = 10;
const SAMPLE_SIZE_STANDARD: usize = 50;
const SAMPLE_SIZE_FULL_GAME: usize = 100;

// Density configurations
const WALL_DENSITY: f32 = 0.25; // 25% walls
const CHEESE_DENSITY: f32 = 0.25; // 25% cheese coverage
const HIGH_CHEESE_DENSITY: f32 = 0.5; // 50% cheese coverage
const MUD_DENSITY: f32 = 0.125; // 12.5% mud coverage
const HIGH_MUD_DENSITY: f32 = 0.25; // 25% mud coverage

// Mud configuration
const MUD_TIMER_RANGE: [u8; 3] = [1, 2, 3];

/// Creates a benchmark game state with random walls, cheese, and mud
fn create_benchmark_game(size: u8, cheese_count: u16, mud_count: usize) -> GameState {
    let mut rng = rand::thread_rng();
    let mut walls = HashMap::new();

    // Create random walls (25% density)
    for x in 0..size {
        for y in 0..size {
            if rng.gen_bool(WALL_DENSITY as f64) {
                let pos = Coordinates::new(x, y);
                let next_x = x.saturating_add(1);
                if next_x < size {
                    walls
                        .entry(pos)
                        .or_insert_with(Vec::new)
                        .push(Coordinates::new(next_x, y));
                }
            }
        }
    }

    // Create initial game state
    let mut game = GameState::new(size, size, walls, MAX_TURNS);

    // Add random cheese
    let mut cheese_added = 0;
    while cheese_added < cheese_count {
        let x = rng.gen_range(0..size);
        let y = rng.gen_range(0..size);
        let pos = Coordinates::new(x, y);
        if game.cheese.place_cheese(pos) {
            cheese_added += 1;
        }
    }

    // Add random mud patches
    for _ in 0..mud_count {
        let x1 = rng.gen_range(0..size);
        let y1 = rng.gen_range(0..size);
        let x2 = x1.saturating_add(1);
        let y2 = y1.saturating_add(1);
        if x2 < size && y2 < size {
            game.mud.insert(
                Coordinates::new(x1, y1),
                Coordinates::new(x2, y2),
                rng.gen_range(1..=3),
            );
        }
    }

    game
}

/// Benchmarks game state creation with different board sizes.
/// Tests the performance of generating random mazes, placing cheese and mud.
/// Board sizes tested: 8x8, 16x16, 32x32, 64x64, 200x200
/// - Cheese density: 25% of board size
/// - Mud density: 12.5% of board size
fn bench_game_creation(c: &mut Criterion) {
    let mut group = c.benchmark_group("game_creation");

    for size in BENCHMARK_BOARD_SIZES.iter() {
        group.bench_with_input(BenchmarkId::from_parameter(size), size, |b, &size| {
            b.iter(|| {
                create_benchmark_game(
                    size,
                    ((size as u16 * size as u16) as f32 * CHEESE_DENSITY) as u16, // 25% cheese coverage
                    ((size as usize * size as usize) as f32 * MUD_DENSITY) as usize, // 12.5% mud coverage
                )
            })
        });
    }
    group.finish();
}

/// Benchmarks the performance of processing random moves.
/// Tests how efficiently the game can handle basic movement operations.
/// For each board size, processes 10 consecutive random moves.
/// Uses increased sample size (50) for more stable results.
fn bench_move_processing(c: &mut Criterion) {
    let mut group = c.benchmark_group("move_processing");
    group.sample_size(SAMPLE_SIZE_STANDARD); // Increase sample size for more stable results

    for size in BENCHMARK_BOARD_SIZES.iter() {
        group.bench_with_input(BenchmarkId::new("random_moves", size), size, |b, &size| {
            let game = create_benchmark_game(
                size,
                ((size as u16 * size as u16) as f32 * CHEESE_DENSITY) as u16,
                ((size as usize * size as usize) as f32 * MUD_DENSITY) as usize,
            );

            b.iter(|| {
                let mut game_copy = game.clone();
                // Process 10 random moves
                for _ in 0..MOVES_PER_TEST {
                    let p1_move =
                        Direction::try_from(rand::random::<u8>() % 5).unwrap_or(Direction::Stay);
                    let p2_move =
                        Direction::try_from(rand::random::<u8>() % 5).unwrap_or(Direction::Stay);
                    black_box(game_copy.process_turn(p1_move, p2_move));
                }
            });
        });
    }
    group.finish();
}

/// Benchmarks cheese collection mechanics.
/// Tests the performance of cheese collection logic with high cheese density (50%).
/// Places players adjacent to cheese pieces to test collection performance.
/// No mud is used to isolate cheese collection performance.
fn bench_cheese_collection(c: &mut Criterion) {
    let mut group = c.benchmark_group("cheese_collection");

    for size in BENCHMARK_BOARD_SIZES.iter() {
        group.bench_with_input(BenchmarkId::from_parameter(size), size, |b, &size| {
            // Create game with high cheese density
            let mut game = create_benchmark_game(
                size,
                ((size as u16 * size as u16) as f32 * HIGH_CHEESE_DENSITY) as u16, // 50% cheese coverage
                0, // No mud for this test
            );

            // Find a cheese piece and position players adjacent to it
            if let Some(&cheese_pos) = game.cheese.get_all_cheese_positions().first() {
                let p1_pos = Coordinates::new(cheese_pos.x.saturating_sub(1), cheese_pos.y);
                let p2_pos = Coordinates::new(cheese_pos.x.saturating_add(1), cheese_pos.y);

                game =
                    GameState::new_with_positions(size, size, HashMap::new(), 300, p1_pos, p2_pos);
            }

            b.iter(|| {
                let mut game_copy = game.clone();
                black_box(game_copy.process_turn(Direction::Right, Direction::Left))
            });
        });
    }
    group.finish();
}

/// Benchmarks complete game simulations.
/// Runs full games from start to finish with random moves.
/// Uses symmetric board generation and 25% cheese density.
/// Measures performance of the entire game loop including win condition checks.
fn bench_full_game(c: &mut Criterion) {
    let mut group = c.benchmark_group("full_game");
    group.sample_size(SAMPLE_SIZE_FULL_GAME); // Reduce sample size as full games take longer

    for &size in BENCHMARK_BOARD_SIZES.iter() {
        group.bench_with_input(BenchmarkId::from_parameter(size), &size, |b, &size| {
            b.iter_with_setup(
                || {
                    // Create symmetric game with size × size dimensions
                    GameState::new_symmetric(
                        Some(size),
                        Some(size),
                        Some(((size as u16 * size as u16) as f32 * CHEESE_DENSITY) as u16), // Use 25% of cells for cheese
                        Some(random::<u64>()), // New random seed each time
                    )
                },
                |mut game| {
                    while !black_box(game.process_turn(
                        unsafe { std::mem::transmute(rand::random::<u8>() % 5) },
                        unsafe { std::mem::transmute(rand::random::<u8>() % 5) },
                    ))
                    .game_over
                    {}
                },
            );
        });
    }
    group.finish();
}

/// Benchmarks movement through mud tiles.
/// Tests performance impact of mud mechanics with 25% mud coverage.
/// No cheese is placed to isolate mud movement performance.
/// Processes 5 consecutive moves to ensure mud interaction.
fn bench_mud_movement(c: &mut Criterion) {
    let mut group = c.benchmark_group("mud_movement");

    for size in BENCHMARK_BOARD_SIZES.iter() {
        group.bench_with_input(BenchmarkId::from_parameter(size), size, |b, &size| {
            // Create game with high mud density
            let game = create_benchmark_game(
                size,
                0,                                                                    // No cheese
                ((size as usize * size as usize) as f32 * HIGH_MUD_DENSITY) as usize, // 25% mud coverage
            );

            b.iter(|| {
                let mut game_copy = game.clone();
                // Process several moves to ensure mud interaction
                for _ in 0..5 {
                    black_box(game_copy.process_turn(Direction::Right, Direction::Left));
                }
            });
        });
    }
    group.finish();
}

/// Benchmarks movement in straight lines.
/// Tests both horizontal and vertical movement separately to identify
/// any performance differences in axis-aligned movement.
/// Uses empty boards (no walls/mud) to isolate movement performance.
fn bench_process_moves_straight_line(c: &mut Criterion) {
    let mut group = c.benchmark_group("process_moves_straight_line");

    for &size in BENCHMARK_BOARD_SIZES.iter() {
        // Start in the middle
        let start_pos = Coordinates::new(size / 2, size / 2);
        let other_pos = Coordinates::new(0, 0); // Other player far away

        // No walls, no mud for this test
        let game_state =
            GameState::new_with_positions(size, size, HashMap::new(), 300, start_pos, other_pos);

        // Horizontal (Right)
        group.bench_with_input(
            BenchmarkId::new("right", size),
            &game_state,
            |b, game_state| {
                b.iter(|| {
                    let mut game_copy = game_state.clone();
                    for _ in 0..(size / 2) {
                        // Go right until hitting the wall
                        black_box(game_copy.process_moves(Direction::Right, Direction::Stay));
                    }
                });
            },
        );

        // Vertical (Down) - Expect this to be potentially slower
        group.bench_with_input(
            BenchmarkId::new("down", size),
            &game_state,
            |b, game_state| {
                b.iter(|| {
                    let mut game_copy = game_state.clone();
                    for _ in 0..(size / 2) {
                        // Go down until hitting the wall
                        black_box(game_copy.process_moves(Direction::Down, Direction::Stay));
                    }
                });
            },
        );
    }
    group.finish();
}

/// Benchmarks basic movement from different starting positions.
/// Tests all possible directions (Up, Down, Left, Right, Stay) from:
/// - Corner positions (0,0)
/// - Center position (size/2, size/2)
/// - Edge positions (size-1, size-1)
fn bench_process_moves_basic_movement(c: &mut Criterion) {
    let mut group = c.benchmark_group("process_moves_basic");
    group.sample_size(50); // Increase sample size for more stable results

    for &size in BENCHMARK_BOARD_SIZES.iter() {
        let start_positions = [
            Coordinates::new(0, 0),
            Coordinates::new(size / 2, size / 2),
            Coordinates::new(size - 1, size - 1),
        ];
        let directions = [
            Direction::Up,
            Direction::Down,
            Direction::Left,
            Direction::Right,
            Direction::Stay,
        ];

        for &start_pos in &start_positions {
            for &direction in &directions {
                let game_state = GameState::new_with_positions(
                    size,
                    size,
                    HashMap::new(), // No walls
                    MAX_TURNS,
                    start_pos,
                    Coordinates::new(0, 0), // Opponent far away
                );

                group.bench_with_input(
                    BenchmarkId::new(
                        format!("{:?}/{:?}", direction, start_pos),
                        format!("{}x{}", size, size),
                    ),
                    &(&game_state, direction),
                    |b, &(game_state, direction)| {
                        b.iter(|| {
                            let mut game_copy = game_state.clone();
                            black_box(game_copy.process_moves(direction, Direction::Stay));
                            // Opponent stays
                        });
                    },
                );
            }
        }
    }
    group.finish();
}

/// Benchmarks collision detection with walls.
/// Tests performance of wall collision handling by placing walls
/// directly in the path of movement for each direction.
/// Ensures proper handling of movement constraints.
fn bench_process_moves_wall_collisions(c: &mut Criterion) {
    let mut group = c.benchmark_group("process_moves_wall_collisions");
    group.sample_size(50);

    for &size in BENCHMARK_BOARD_SIZES.iter() {
        let start_pos = Coordinates::new(size / 2, size / 2);
        let directions = [
            Direction::Up,
            Direction::Down,
            Direction::Left,
            Direction::Right,
        ];

        for &direction in &directions {
            // Create a wall in the direction of movement
            let mut walls = HashMap::new();
            let wall_pos = direction.apply_to(start_pos);
            walls.insert(start_pos, vec![wall_pos]);
            walls.insert(wall_pos, vec![start_pos]);

            let game_state = GameState::new_with_positions(
                size,
                size,
                walls,
                MAX_TURNS,
                start_pos,
                Coordinates::new(0, 0), // Opponent far away
            );

            group.bench_with_input(
                BenchmarkId::new(format!("{:?}", direction), format!("{}x{}", size, size)),
                &(&game_state, direction),
                |b, &(game_state, direction)| {
                    b.iter(|| {
                        let mut game_copy = game_state.clone();
                        black_box(game_copy.process_moves(direction, Direction::Stay));
                        // Opponent stays
                    });
                },
            );
        }
    }
    group.finish();
}

/// Benchmarks mud movement with different mud timers.
/// Tests movement through mud tiles with varying timer values (1-3).
/// Measures performance impact of mud state tracking and updates.
fn bench_process_moves_mud_movement(c: &mut Criterion) {
    let mut group = c.benchmark_group("process_moves_mud_movement");
    group.sample_size(50);

    for &size in BENCHMARK_BOARD_SIZES.iter() {
        let start_pos = Coordinates::new(size / 2, size / 2);
        let directions = [
            Direction::Up,
            Direction::Down,
            Direction::Left,
            Direction::Right,
        ];

        for &direction in &directions {
            for &mud_timer in &MUD_TIMER_RANGE {
                // Create mud in the direction of movement
                let mut mud = std::collections::HashMap::new();
                let mud_pos = direction.apply_to(start_pos);
                mud.insert((start_pos, mud_pos), mud_timer);

                let game_state = GameState::new_with_config(
                    size,
                    size,
                    HashMap::new(), // No walls
                    mud,
                    &[], // No cheese
                    start_pos,
                    Coordinates::new(0, 0), // Opponent far away
                    MAX_TURNS,
                );

                group.bench_with_input(
                    BenchmarkId::new(
                        format!("{:?}/mud_timer={}", direction, mud_timer),
                        format!("{}x{}", size, size),
                    ),
                    &(&game_state, direction),
                    |b, &(game_state, direction)| {
                        b.iter(|| {
                            let mut game_copy = game_state.clone();
                            black_box(game_copy.process_moves(direction, Direction::Stay));
                            // Opponent stays
                        });
                    },
                );
            }
        }
    }
    group.finish();
}

/// Benchmarks cheese collection mechanics in detail.
/// Tests both single-player and simultaneous collection scenarios.
/// Measures performance of score updates and board state changes.
fn bench_process_cheese_collection(c: &mut Criterion) {
    let mut group = c.benchmark_group("process_cheese_collection");
    group.sample_size(SAMPLE_SIZE_STANDARD);

    for &size in BENCHMARK_BOARD_SIZES.iter() {
        // Basic collection (single player)
        group.bench_with_input(
            BenchmarkId::new("single_player", size),
            &size,
            |b, &size| {
                let mut game = GameState::new(size, size, HashMap::new(), MAX_TURNS);
                let p1_pos = Coordinates::new(1, 1);
                game = GameState::new_with_positions(
                    size,
                    size,
                    HashMap::new(),
                    MAX_TURNS,
                    p1_pos,
                    Coordinates::new(0, 0),
                );
                game.cheese.place_cheese(p1_pos); // Place cheese at player position

                b.iter(|| {
                    let mut game_copy = game.clone();
                    black_box(game_copy.process_cheese_collection())
                });
            },
        );

        // Simultaneous collection
        group.bench_with_input(BenchmarkId::new("simultaneous", size), &size, |b, &size| {
            let mut game = GameState::new(size, size, HashMap::new(), MAX_TURNS);
            let shared_pos = Coordinates::new(1, 1);
            game = GameState::new_with_positions(
                size,
                size,
                HashMap::new(),
                MAX_TURNS,
                shared_pos,
                shared_pos, // Both players on same spot
            );
            game.cheese.place_cheese(shared_pos);

            b.iter(|| {
                let mut game_copy = game.clone();
                black_box(game_copy.process_cheese_collection())
            });
        });
    }
    group.finish();
}

criterion_group!(
    name = benches;
    config = Criterion::default()
        .warm_up_time(std::time::Duration::from_secs(1))
        .measurement_time(std::time::Duration::from_secs(1));
    targets = bench_game_creation,
        bench_move_processing,
        bench_cheese_collection,
        bench_mud_movement,
        bench_full_game,
        bench_process_moves_straight_line,
        bench_process_moves_basic_movement,
        bench_process_moves_wall_collisions,
        bench_process_moves_mud_movement,
        bench_process_cheese_collection,
);
criterion_main!(benches);
