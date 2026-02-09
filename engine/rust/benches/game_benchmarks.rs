use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion};
use pyrat::bench_scenarios::{create_game, random_direction, COMBOS, SIZES};

fn bench_id(
    size: &pyrat::bench_scenarios::BoardSize,
    combo: &pyrat::bench_scenarios::FeatureCombo,
) -> BenchmarkId {
    BenchmarkId::new(
        combo.name,
        format!("{}/{}x{}", size.name, size.width, size.height),
    )
}

fn bench_game_init(c: &mut Criterion) {
    let mut group = c.benchmark_group("game_init");
    group.throughput(criterion::Throughput::Elements(1));

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

fn bench_process_turn(c: &mut Criterion) {
    let mut group = c.benchmark_group("process_turn");
    group.sample_size(50);

    for size in SIZES {
        let turns = u64::from(size.max_turns / 2);
        group.throughput(criterion::Throughput::Elements(turns));

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
                        for _ in 0..turns {
                            black_box(game.process_turn(
                                random_direction(&mut rng),
                                random_direction(&mut rng),
                            ));
                        }
                    },
                );
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
    targets = bench_game_init, bench_process_turn, bench_full_game
);
criterion_main!(benches);
