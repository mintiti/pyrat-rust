#[allow(dead_code)]
#[path = "../support.rs"]
mod support;

use std::time::{Duration, Instant};
use support::{scenario_specs, ActionPair, ScenarioSpec, SeedTape};

/// Run a scenario for `duration`, returning total turns, total games, and
/// actual elapsed time.
///
/// When `hash` is true, calls `state_hash()` once per turn (simulating a
/// transposition table lookup in MCTS).
fn run_scenario(spec: ScenarioSpec, duration: Duration, hash: bool) -> (u64, u64, Duration) {
    let config = spec.random_config();
    let game_seeds = spec.create_seed_tape();
    let actions = spec.episode_action_tape();
    let mut seed_cursor = 0;
    let mut total_turns: u64 = 0;
    let mut total_games: u64 = 0;

    let started = Instant::now();
    let deadline = started + duration;
    while Instant::now() < deadline {
        let seed = next_seed(&game_seeds, &mut seed_cursor);
        let mut game = config.create(Some(seed)).unwrap();
        play_episode(&mut game, &actions, hash, &mut total_turns);
        total_games += 1;
    }

    (total_turns, total_games, started.elapsed())
}

fn play_episode(
    game: &mut pyrat::GameState,
    actions: &[ActionPair],
    hash: bool,
    total_turns: &mut u64,
) {
    for action in actions {
        if hash {
            std::hint::black_box(game.state_hash());
        }
        *total_turns += 1;
        if game.process_turn(action.p1, action.p2).game_over {
            return;
        }
    }

    panic!(
        "prepared episode action tape did not reach game_over in {} turns",
        actions.len()
    );
}

fn next_seed(seeds: &SeedTape, cursor: &mut usize) -> u64 {
    let seed = seeds[*cursor];
    *cursor = (*cursor + 1) % seeds.len();
    seed
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

    for spec in scenario_specs() {
        let board = format!("{}x{}", spec.size.width, spec.size.height);
        let (turns, games, elapsed) = run_scenario(spec, duration, hash);
        let elapsed_seconds = elapsed.as_secs_f64();
        println!(
            "{:<12} {:<12} {:>8} {:>14.0} {:>14.0}",
            spec.size.name,
            spec.combo.name,
            board,
            turns as f64 / elapsed_seconds,
            games as f64 / elapsed_seconds,
        );
    }
}

/// Runs a single scenario in a tight loop (never returns).
/// Attach a profiler and Ctrl+C to stop.
fn run_single(spec: ScenarioSpec) -> ! {
    eprintln!(
        "Running {}/{} ({}x{}) — Ctrl+C to stop",
        spec.size.name, spec.combo.name, spec.size.width, spec.size.height
    );

    let config = spec.random_config();
    let game_seeds = spec.create_seed_tape();
    let actions = spec.episode_action_tape();
    let mut seed_cursor = 0;
    loop {
        let seed = next_seed(&game_seeds, &mut seed_cursor);
        let mut game = config.create(Some(seed)).unwrap();
        let mut turns = 0;
        play_episode(&mut game, &actions, true, &mut turns);
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let hash = args.iter().any(|argument| argument == "--hash");
    let positional: Vec<&str> = args[1..]
        .iter()
        .filter(|argument| !argument.starts_with('-'))
        .map(String::as_str)
        .collect();

    if let Some(scenario) = positional.first() {
        let Some((size_name, combo_name)) = scenario.split_once('/') else {
            eprintln!("Usage: profile_game <size>/<combo>");
            eprintln!("  e.g. medium/classic, large/walls_only");
            std::process::exit(1);
        };
        let Some(spec) = scenario_specs()
            .into_iter()
            .find(|spec| spec.size.name == size_name && spec.combo.name == combo_name)
        else {
            eprintln!("Unknown scenario: {scenario}");
            eprintln!(
                "Scenarios: {}",
                scenario_specs()
                    .into_iter()
                    .map(|spec| format!("{}/{}", spec.size.name, spec.combo.name))
                    .collect::<Vec<_>>()
                    .join(", ")
            );
            std::process::exit(1);
        };
        run_single(spec);
    } else {
        run_all(hash);
    }
}
