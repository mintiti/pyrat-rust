mod support;

use criterion::{criterion_group, criterion_main, BatchSize, BenchmarkId, Criterion, Throughput};
use pyrat::bench_scenarios::{BoardSize, FeatureCombo, SIZES};
use pyrat::game::maze_generation::{CheeseGenerator, MazeGenerator};
use pyrat::game::zobrist;
use pyrat::{GameState, MoveTable};
use std::hint::black_box;
use support::{
    prepared_scenarios, PreparedScenario, FIXED_CREATE_SEED, SEED_TAPE_LEN, TURN_BATCH_LEN,
};

fn bench_id(size: &BoardSize, combo: &FeatureCombo) -> BenchmarkId {
    BenchmarkId::new(
        combo.name,
        format!("{}/{}x{}", size.name, size.width, size.height),
    )
}

fn size_bench_id(size: &BoardSize) -> BenchmarkId {
    BenchmarkId::new(
        "symmetric",
        format!("{}/{}x{}", size.name, size.width, size.height),
    )
}

fn bench_random_create(c: &mut Criterion, scenarios: &[PreparedScenario]) {
    let mut group = c.benchmark_group("random_create");
    group.sample_size(20);
    group.throughput(Throughput::Elements(SEED_TAPE_LEN as u64));

    for scenario in scenarios {
        group.bench_function(
            bench_id(scenario.spec.size, scenario.spec.combo),
            |bencher| {
                bencher.iter_batched(
                    || (),
                    |()| {
                        let games: [GameState; SEED_TAPE_LEN] = std::array::from_fn(|index| {
                            scenario
                                .random_config
                                .create(Some(scenario.create_seeds[index]))
                                .unwrap()
                        });
                        games
                    },
                    BatchSize::LargeInput,
                );
            },
        );
    }
    group.finish();
}

fn bench_maze_generation(c: &mut Criterion, scenarios: &[PreparedScenario]) {
    let mut group = c.benchmark_group("maze_generation");
    group.sample_size(20);
    group.throughput(Throughput::Elements(SEED_TAPE_LEN as u64));

    for scenario in scenarios {
        group.bench_function(
            bench_id(scenario.spec.size, scenario.spec.combo),
            |bencher| {
                bencher.iter_batched(
                    || {
                        let generators: [MazeGenerator; SEED_TAPE_LEN] =
                            std::array::from_fn(|index| {
                                MazeGenerator::new(
                                    scenario.spec.maze_config(scenario.maze_seeds[index]),
                                )
                            });
                        generators
                    },
                    |generators| generators.map(|mut generator| generator.generate()),
                    BatchSize::LargeInput,
                );
            },
        );
    }
    group.finish();
}

fn bench_disconnected_maze_create_and_generate(c: &mut Criterion, scenarios: &[PreparedScenario]) {
    let mut group = c.benchmark_group("disconnected_maze_create_and_generate");
    group.sample_size(20);
    group.throughput(Throughput::Elements(SEED_TAPE_LEN as u64));

    for scenario in scenarios {
        group.bench_function(
            bench_id(scenario.spec.size, scenario.spec.combo),
            |bencher| {
                bencher.iter_batched(
                    || {
                        let configs: [_; SEED_TAPE_LEN] = std::array::from_fn(|index| {
                            let mut config = scenario.spec.maze_config(scenario.maze_seeds[index]);
                            config.connected = false;
                            config
                        });
                        configs
                    },
                    |configs| {
                        configs.map(|config| {
                            let mut generator = MazeGenerator::new(config);
                            generator.generate()
                        })
                    },
                    BatchSize::LargeInput,
                );
            },
        );
    }
    group.finish();
}

fn bench_cheese_generation(c: &mut Criterion, scenarios: &[PreparedScenario]) {
    let mut group = c.benchmark_group("cheese_generation");
    group.throughput(Throughput::Elements(SEED_TAPE_LEN as u64));

    for size in SIZES {
        let scenario = scenarios
            .iter()
            .find(|scenario| {
                scenario.spec.size.name == size.name && scenario.spec.combo.name == "classic"
            })
            .expect("every benchmark size has a prepared classic scenario");
        let (player1, player2) = scenario.spec.corner_positions();
        group.bench_function(size_bench_id(size), |bencher| {
            bencher.iter_batched(
                || {
                    let generators: [CheeseGenerator; SEED_TAPE_LEN] =
                        std::array::from_fn(|index| {
                            CheeseGenerator::new(
                                scenario.spec.cheese_config(),
                                size.width,
                                size.height,
                                Some(scenario.cheese_seeds[index]),
                            )
                        });
                    generators
                },
                |generators| {
                    generators.map(|mut generator| generator.generate(player1, player2).unwrap())
                },
                BatchSize::LargeInput,
            );
        });
    }
    group.finish();
}

fn bench_topology_compilation(c: &mut Criterion, scenarios: &[PreparedScenario]) {
    let mut group = c.benchmark_group("topology_compilation");
    group.throughput(Throughput::Elements(1));

    for scenario in scenarios {
        group.bench_function(
            bench_id(scenario.spec.size, scenario.spec.combo),
            |bencher| {
                bencher.iter_batched(
                    || (),
                    |()| {
                        let move_table = MoveTable::new(
                            scenario.spec.size.width,
                            scenario.spec.size.height,
                            &scenario.walls,
                        );
                        let topology_hash = zobrist::maze_hash(
                            &move_table,
                            &scenario.mud,
                            scenario.spec.size.width,
                            scenario.spec.size.height,
                        );
                        (move_table, topology_hash)
                    },
                    BatchSize::LargeInput,
                );
            },
        );
    }
    group.finish();
}

fn bench_fixed_create(c: &mut Criterion, scenarios: &[PreparedScenario]) {
    let mut group = c.benchmark_group("fixed_create");
    group.throughput(Throughput::Elements(1));

    for scenario in scenarios {
        group.bench_function(
            bench_id(scenario.spec.size, scenario.spec.combo),
            |bencher| {
                bencher.iter_batched(
                    || (),
                    |()| {
                        scenario
                            .fixed_config
                            .create(Some(FIXED_CREATE_SEED))
                            .unwrap()
                    },
                    BatchSize::LargeInput,
                );
            },
        );
    }
    group.finish();
}

fn bench_reused_maze_create(c: &mut Criterion, scenarios: &[PreparedScenario]) {
    let mut group = c.benchmark_group("reused_maze_create");
    group.sample_size(20);
    group.throughput(Throughput::Elements(SEED_TAPE_LEN as u64));

    for scenario in scenarios {
        group.bench_function(
            bench_id(scenario.spec.size, scenario.spec.combo),
            |bencher| {
                bencher.iter_batched(
                    || (),
                    |()| {
                        let games: [GameState; SEED_TAPE_LEN] = std::array::from_fn(|index| {
                            scenario
                                .random_config
                                .create_with_maze(
                                    &scenario.reused_maze,
                                    Some(scenario.create_seeds[index]),
                                )
                                .unwrap()
                        });
                        games
                    },
                    BatchSize::LargeInput,
                );
            },
        );
    }
    group.finish();
}

fn bench_paired_maze_setup(c: &mut Criterion, scenarios: &[PreparedScenario]) {
    let mut group = c.benchmark_group("paired_maze_setup");
    group.sample_size(20);
    // Each element is one two-game, same-maze setup.
    group.throughput(Throughput::Elements(SEED_TAPE_LEN as u64));

    for scenario in scenarios {
        group.bench_function(
            bench_id(scenario.spec.size, scenario.spec.combo),
            |bencher| {
                bencher.iter_batched(
                    || (),
                    |()| {
                        let pairs: [(GameState, GameState); SEED_TAPE_LEN] =
                            std::array::from_fn(|index| {
                                let seed = Some(scenario.create_seeds[index]);
                                (
                                    scenario
                                        .random_config
                                        .create_with_maze(&scenario.reused_maze, seed)
                                        .unwrap(),
                                    scenario
                                        .random_config
                                        .create_with_maze(&scenario.reused_maze, seed)
                                        .unwrap(),
                                )
                            });
                        pairs
                    },
                    BatchSize::LargeInput,
                );
            },
        );
    }
    group.finish();
}

fn bench_process_turn(c: &mut Criterion, scenarios: &[PreparedScenario]) {
    let mut group = c.benchmark_group("process_turn");
    group.sample_size(50);
    group.throughput(Throughput::Elements(TURN_BATCH_LEN as u64));

    for scenario in scenarios {
        group.bench_function(
            bench_id(scenario.spec.size, scenario.spec.combo),
            |bencher| {
                bencher.iter_batched_ref(
                    || scenario.initial_game.clone(),
                    |game| {
                        for action in &scenario.turn_actions {
                            black_box(game.process_turn(action.p1, action.p2));
                        }
                        black_box(&*game);
                    },
                    BatchSize::LargeInput,
                );
            },
        );
    }
    group.finish();
}

fn bench_make_unmake(c: &mut Criterion, scenarios: &[PreparedScenario]) {
    let mut group = c.benchmark_group("make_unmake");
    group.sample_size(50);
    group.throughput(Throughput::Elements(TURN_BATCH_LEN as u64));

    for scenario in scenarios {
        group.bench_function(
            bench_id(scenario.spec.size, scenario.spec.combo),
            |bencher| {
                bencher.iter_batched_ref(
                    || scenario.initial_game.clone(),
                    |game| {
                        for action in &scenario.turn_actions {
                            let undo = black_box(game.make_move(action.p1, action.p2));
                            game.unmake_move(undo);
                        }
                        black_box(&*game);
                    },
                    BatchSize::LargeInput,
                );
            },
        );
    }
    group.finish();
}

fn bench_make_unmake_no_collection(c: &mut Criterion, scenarios: &[PreparedScenario]) {
    let mut group = c.benchmark_group("make_unmake_no_collection");
    group.sample_size(50);
    group.throughput(Throughput::Elements(TURN_BATCH_LEN as u64));

    for scenario in scenarios {
        let mut initial_game = scenario.initial_game.clone();
        let mut collected_positions = Vec::new();

        // Every pair is undone to the same root. Remove only cheese reachable
        // by this tape so movement and game-over work remain representative.
        for action in &scenario.turn_actions {
            let undo = initial_game.make_move(action.p1, action.p2);
            collected_positions.extend_from_slice(&undo.collected_cheese);
            initial_game.unmake_move(undo);
        }
        for position in collected_positions {
            initial_game.cheese.take_cheese(position);
        }
        initial_game.recompute_state_hash();
        for action in &scenario.turn_actions {
            let undo = initial_game.make_move(action.p1, action.p2);
            assert!(undo.collected_cheese.is_empty());
            initial_game.unmake_move(undo);
        }

        group.bench_function(
            bench_id(scenario.spec.size, scenario.spec.combo),
            |bencher| {
                bencher.iter_batched_ref(
                    || initial_game.clone(),
                    |game| {
                        for action in &scenario.turn_actions {
                            let undo = black_box(game.make_move(action.p1, action.p2));
                            game.unmake_move(undo);
                        }
                        black_box(&*game);
                    },
                    BatchSize::LargeInput,
                );
            },
        );
    }
    group.finish();
}

fn bench_full_episode(c: &mut Criterion, scenarios: &[PreparedScenario]) {
    let mut group = c.benchmark_group("full_episode");
    group.sample_size(50);

    for scenario in scenarios {
        group.throughput(Throughput::Elements(scenario.episode_len as u64));
        group.bench_function(
            bench_id(scenario.spec.size, scenario.spec.combo),
            |bencher| {
                bencher.iter_batched_ref(
                    || scenario.initial_game.clone(),
                    |game| {
                        for action in scenario.episode_actions.iter().take(scenario.episode_len) {
                            let result = game.process_turn(action.p1, action.p2);
                            let game_over = result.game_over;
                            black_box(result);
                            if game_over {
                                break;
                            }
                        }
                        black_box(&*game);
                    },
                    BatchSize::LargeInput,
                );
            },
        );
    }
    group.finish();
}

fn bench_rust_matrix(c: &mut Criterion) {
    let scenarios = prepared_scenarios()
        .unwrap_or_else(|error| panic!("failed to prepare benchmark scenarios: {error}"));

    bench_random_create(c, &scenarios);
    bench_maze_generation(c, &scenarios);
    bench_disconnected_maze_create_and_generate(c, &scenarios);
    bench_cheese_generation(c, &scenarios);
    bench_topology_compilation(c, &scenarios);
    bench_fixed_create(c, &scenarios);
    bench_reused_maze_create(c, &scenarios);
    bench_paired_maze_setup(c, &scenarios);
    bench_process_turn(c, &scenarios);
    bench_make_unmake(c, &scenarios);
    bench_make_unmake_no_collection(c, &scenarios);
    bench_full_episode(c, &scenarios);
}

criterion_group!(
    name = benches;
    config = Criterion::default()
        .warm_up_time(std::time::Duration::from_secs(1))
        .measurement_time(std::time::Duration::from_secs(2));
    targets = bench_rust_matrix
);
criterion_main!(benches);
