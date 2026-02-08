use pyrat::{Direction, GameState};
use rand::Rng;
use std::time::{Duration, Instant};

// ---------------------------------------------------------------------------
// Scenario matrix (same as criterion benchmarks)
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

/// Run a scenario for `duration`, returns (total_turns, total_games).
fn run_scenario(size: &BoardSize, combo: &FeatureCombo, duration: Duration) -> (u64, u64) {
    let mut rng = rand::thread_rng();
    let mut total_turns: u64 = 0;
    let mut total_games: u64 = 0;
    let mut seed: u64 = 0;

    let deadline = Instant::now() + duration;
    while Instant::now() < deadline {
        seed = seed.wrapping_add(1);
        let mut game = create_game(size, combo, seed);
        while !game
            .process_turn(random_direction(&mut rng), random_direction(&mut rng))
            .game_over
        {
            total_turns += 1;
        }
        total_turns += 1; // count the final turn
        total_games += 1;
    }
    (total_turns, total_games)
}

// ---------------------------------------------------------------------------
// Modes
// ---------------------------------------------------------------------------

/// Runs all 20 scenarios and prints an aligned throughput table.
fn run_all() {
    let duration = Duration::from_secs(2);

    println!(
        "{:<12} {:<12} {:>8} {:>14} {:>14}",
        "size", "combo", "board", "turns/sec", "games/sec"
    );
    println!("{}", "-".repeat(62));

    for size in SIZES {
        for combo in COMBOS {
            let board = format!("{}x{}", size.width, size.height);
            let (turns, games) = run_scenario(size, combo, duration);
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
        eprintln!("  e.g. default/default, large/walls_only");
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

    let mut rng = rand::thread_rng();
    let mut seed: u64 = 0;
    loop {
        seed = seed.wrapping_add(1);
        let mut game = create_game(size, combo, seed);
        while !game
            .process_turn(random_direction(&mut rng), random_direction(&mut rng))
            .game_over
        {}
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() > 1 {
        run_single(&args[1]);
    } else {
        run_all();
    }
}
