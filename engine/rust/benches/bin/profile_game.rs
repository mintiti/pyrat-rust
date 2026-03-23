use pyrat::bench_scenarios::{create_game, random_direction, COMBOS, SIZES};
use std::time::{Duration, Instant};

/// Run a scenario for `duration`, returns (total_turns, total_games).
///
/// When `hash` is true, calls `state_hash()` once per turn (simulating a
/// transposition table lookup in MCTS).
fn run_scenario(
    size: &pyrat::bench_scenarios::BoardSize,
    combo: &pyrat::bench_scenarios::FeatureCombo,
    duration: Duration,
    hash: bool,
) -> (u64, u64) {
    let mut rng = rand::rng();
    let mut total_turns: u64 = 0;
    let mut total_games: u64 = 0;
    let mut seed: u64 = 0;

    let deadline = Instant::now() + duration;
    while Instant::now() < deadline {
        seed = seed.wrapping_add(1);
        let mut game = create_game(size, combo, seed);
        loop {
            if hash {
                std::hint::black_box(game.state_hash());
            }
            if game
                .process_turn(random_direction(&mut rng), random_direction(&mut rng))
                .game_over
            {
                break;
            }
            total_turns += 1;
        }
        total_turns += 1; // count the final turn
        total_games += 1;
    }
    (total_turns, total_games)
}

/// Runs all 20 scenarios and prints an aligned throughput table.
fn run_all(hash: bool) {
    let duration = Duration::from_secs(2);

    if hash {
        eprintln!("(hashing once per turn)");
    }

    println!(
        "{:<12} {:<12} {:>8} {:>14} {:>14}",
        "size", "combo", "board", "turns/sec", "games/sec"
    );
    println!("{}", "-".repeat(62));

    for size in SIZES {
        for combo in COMBOS {
            let board = format!("{}x{}", size.width, size.height);
            let (turns, games) = run_scenario(size, combo, duration, hash);
            let elapsed = duration.as_secs_f64();
            println!(
                "{:<12} {:<12} {:>8} {:>14.0} {:>14.0}",
                size.name,
                combo.name,
                board,
                turns as f64 / elapsed,
                games as f64 / elapsed,
            );
        }
    }
}

/// Runs a single scenario in a tight loop (never returns).
/// Attach a profiler and Ctrl+C to stop.
fn run_single(scenario: &str) -> ! {
    let parts: Vec<&str> = scenario.split('/').collect();
    if parts.len() != 2 {
        eprintln!("Usage: profile_game <size>/<combo>");
        eprintln!("  e.g. medium/classic, large/walls_only");
        std::process::exit(1);
    }

    let size = SIZES.iter().find(|s| s.name == parts[0]);
    let combo = COMBOS.iter().find(|c| c.name == parts[1]);

    let (Some(size), Some(combo)) = (size, combo) else {
        eprintln!("Unknown scenario: {scenario}");
        eprintln!(
            "Sizes: {}",
            SIZES.iter().map(|s| s.name).collect::<Vec<_>>().join(", ")
        );
        eprintln!(
            "Combos: {}",
            COMBOS.iter().map(|c| c.name).collect::<Vec<_>>().join(", ")
        );
        std::process::exit(1);
    };

    eprintln!(
        "Running {}/{} ({}x{}) — Ctrl+C to stop",
        size.name, combo.name, size.width, size.height
    );

    let mut rng = rand::rng();
    let mut seed: u64 = 0;
    loop {
        seed = seed.wrapping_add(1);
        let mut game = create_game(size, combo, seed);
        loop {
            std::hint::black_box(game.state_hash());
            if game
                .process_turn(random_direction(&mut rng), random_direction(&mut rng))
                .game_over
            {
                break;
            }
        }
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let hash = args.iter().any(|a| a == "--hash");
    let positional: Vec<&str> = args[1..]
        .iter()
        .filter(|a| !a.starts_with('-'))
        .map(|s| s.as_str())
        .collect();

    if let Some(scenario) = positional.first() {
        run_single(scenario);
    } else {
        run_all(hash);
    }
}
