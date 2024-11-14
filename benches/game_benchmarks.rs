use criterion::{black_box, criterion_group, criterion_main, Criterion, BenchmarkId};
use pyrat::{GameState, Coordinates, Direction};
use std::collections::HashMap;
use rand::{random, Rng};

// Helper function to generate random moves
fn generate_random_moves(count: usize) -> Vec<(Direction, Direction)> {
    let mut rng = rand::thread_rng();
    let mut moves = Vec::with_capacity(count);
    for _ in 0..count {
        let p1_move = unsafe { std::mem::transmute(rng.gen_range(0..5u8)) };
        let p2_move = unsafe { std::mem::transmute(rng.gen_range(0..5u8)) };
        moves.push((p1_move, p2_move));
    }
    moves
}
/// Creates a benchmark game state with random walls, cheese, and mud
fn create_benchmark_game(size: u8, cheese_count: u16, mud_count: usize) -> GameState {
    let mut rng = rand::thread_rng();
    let mut walls = HashMap::new();

    // Create random walls (25% density)
    for x in 0..size {
        for y in 0..size {
            if rng.gen_ratio(1, 4) {
                let pos = Coordinates::new(x, y);
                let next_x = x.saturating_add(1);
                if next_x < size {
                    walls.entry(pos)
                        .or_insert_with(Vec::new)
                        .push(Coordinates::new(next_x, y));
                }
            }
        }
    }

    // Create initial game state
    let mut game = GameState::new(size, size, walls, 300);

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
                (Coordinates::new(x1, y1), Coordinates::new(x2, y2)),
                rng.gen_range(1..=3),
            );
        }
    }

    game
}

fn bench_game_creation(c: &mut Criterion) {
    let mut group = c.benchmark_group("game_creation");

    for size in [8u8, 16, 32, 64,200].iter() {
        group.bench_with_input(
            BenchmarkId::from_parameter(size),
            size,
            |b, &size| {
                b.iter(|| {
                    create_benchmark_game(
                        size,
                        (size as u16 * size as u16) / 4,  // 25% cheese coverage
                        (size as usize * size as usize) / 8 // 12.5% mud coverage
                    )
                })
            }
        );
    }
    group.finish();
}

fn bench_move_processing(c: &mut Criterion) {
    let mut group = c.benchmark_group("move_processing");
    group.sample_size(50); // Increase sample size for more stable results

    for size in [8u8, 16, 32,64,200].iter() {
        group.bench_with_input(
            BenchmarkId::new("random_moves", size),
            size,
            |b, &size| {
                let game = create_benchmark_game(
                    size,
                    (size as u16 * size as u16) / 4,
                    (size as usize * size as usize) / 8
                );

                b.iter(|| {
                    let mut game_copy = game.clone();
                    // Process 10 random moves
                    for _ in 0..10 {
                        let p1_move = unsafe { std::mem::transmute(rand::random::<u8>() % 5) };
                        let p2_move = unsafe { std::mem::transmute(rand::random::<u8>() % 5) };
                        black_box(game_copy.process_turn(p1_move, p2_move));
                    }
                });
            }
        );
    }
    group.finish();
}

fn bench_cheese_collection(c: &mut Criterion) {
    let mut group = c.benchmark_group("cheese_collection");

    for size in [8u8, 16, 32,64,200].iter() {
        group.bench_with_input(
            BenchmarkId::from_parameter(size),
            size,
            |b, &size| {
                // Create game with high cheese density
                let mut game = create_benchmark_game(
                    size,
                    (size as u16 * size as u16) / 2, // 50% cheese coverage
                    0  // No mud for this test
                );

                // Find a cheese piece and position players adjacent to it
                if let Some(&cheese_pos) = game.cheese.get_all_cheese_positions().first() {
                    let p1_pos = Coordinates::new(
                        cheese_pos.x.saturating_sub(1),
                        cheese_pos.y
                    );
                    let p2_pos = Coordinates::new(
                        cheese_pos.x.saturating_add(1),
                        cheese_pos.y
                    );

                    game = GameState::new_with_positions(
                        size, size,
                        HashMap::new(),
                        300,
                        p1_pos,
                        p2_pos
                    );
                }

                b.iter(|| {
                    let mut game_copy = game.clone();
                    black_box(game_copy.process_turn(Direction::Right, Direction::Left))
                });
            }
        );
    }
    group.finish();
}

fn bench_full_game(c: &mut Criterion) {
    let mut group = c.benchmark_group("full_game");
    group.sample_size(100); // Reduce sample size as full games take longer

    for &size in [8u8, 16, 32,64, 200].iter() {
        group.bench_with_input(
            BenchmarkId::from_parameter(size),
            &size,
            |b, &size| {
                b.iter_with_setup(
                    || {
                        // Create symmetric game with size × size dimensions
                        GameState::new_symmetric(
                            Some(size),
                            Some(size),
                            Some((size as u16 * size as u16) / 4), // Use 25% of cells for cheese
                            Some(random::<u64>()) // New random seed each time
                        )
                    },
                    |mut game| {
                        while !black_box(game.process_turn(
                            unsafe { std::mem::transmute(rand::random::<u8>() % 5) },
                            unsafe { std::mem::transmute(rand::random::<u8>() % 5) }
                        )).game_over {}
                    }
                );
            }
        );
    }
    group.finish();
}

fn bench_mud_movement(c: &mut Criterion) {
    let mut group = c.benchmark_group("mud_movement");

    for size in [8u8, 16, 32,64,200].iter() {
        group.bench_with_input(
            BenchmarkId::from_parameter(size),
            size,
            |b, &size| {
                // Create game with high mud density
                let game = create_benchmark_game(
                    size,
                    0,  // No cheese
                    (size as usize * size as usize) / 4  // 25% mud coverage
                );

                b.iter(|| {
                    let mut game_copy = game.clone();
                    // Process several moves to ensure mud interaction
                    for _ in 0..5 {
                        black_box(game_copy.process_turn(
                            Direction::Right,
                            Direction::Left
                        ));
                    }
                });
            }
        );
    }
    group.finish();
}

criterion_group!(
    name = benches;
    config = Criterion::default()
        .warm_up_time(std::time::Duration::from_secs(1))
        .measurement_time(std::time::Duration::from_secs(5));
    targets = bench_game_creation,
              bench_move_processing,
              bench_cheese_collection,
              bench_mud_movement,
              bench_full_game,
);
criterion_main!(benches);