use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion};
use pyrat::{Direction, GameState};
use rand::Rng;

// ---------------------------------------------------------------------------
// Scenario matrix
// ---------------------------------------------------------------------------

struct BoardSize {
    name: &'static str,
    width: u8,
    height: u8,
    cheese: u16,
    max_turns: u16,
}

const SIZES: &[BoardSize] = &[
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

struct FeatureCombo {
    name: &'static str,
    wall_density: f32,
    mud_density: f32,
}

const COMBOS: &[FeatureCombo] = &[
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

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

#[inline]
fn random_direction(rng: &mut impl Rng) -> Direction {
    match rng.gen_range(0u8..5) {
        0 => Direction::Up,
        1 => Direction::Right,
        2 => Direction::Down,
        3 => Direction::Left,
        _ => Direction::Stay,
    }
}

fn create_game(size: &BoardSize, combo: &FeatureCombo, seed: u64) -> GameState {
    let mut game = GameState::new_symmetric(
        Some(size.width),
        Some(size.height),
        Some(size.cheese),
        Some(seed),
        Some(combo.wall_density),
        Some(combo.mud_density),
    );
    game.max_turns = size.max_turns;
    game
}

fn bench_id(size: &BoardSize, combo: &FeatureCombo) -> BenchmarkId {
    BenchmarkId::new(
        combo.name,
        format!("{}/{}x{}", size.name, size.width, size.height),
    )
}

// ---------------------------------------------------------------------------
// Benchmark groups
// ---------------------------------------------------------------------------

fn bench_game_init(c: &mut Criterion) {
    let mut group = c.benchmark_group("game_init");

    for size in SIZES {
        for combo in COMBOS {
            group.bench_function(bench_id(size, combo), |b| {
                let mut seed: u64 = 0;
                b.iter(|| {
                    seed = seed.wrapping_add(1);
                    black_box(create_game(size, combo, seed));
                });
            });
        }
    }
    group.finish();
}

fn bench_full_game(c: &mut Criterion) {
    let mut group = c.benchmark_group("full_game");
    group.sample_size(50);

    for size in SIZES {
        for combo in COMBOS {
            group.bench_function(bench_id(size, combo), |b| {
                let mut rng = rand::thread_rng();
                let mut seed: u64 = 0;
                b.iter_with_setup(
                    || {
                        seed = seed.wrapping_add(1);
                        create_game(size, combo, seed)
                    },
                    |mut game| {
                        while !black_box(
                            game.process_turn(
                                random_direction(&mut rng),
                                random_direction(&mut rng),
                            ),
                        )
                        .game_over
                        {}
                    },
                );
            });
        }
    }
    group.finish();
}

criterion_group!(
    name = benches;
    config = Criterion::default()
        .warm_up_time(std::time::Duration::from_secs(1))
        .measurement_time(std::time::Duration::from_secs(2));
    targets = bench_game_init, bench_full_game
);
criterion_main!(benches);
