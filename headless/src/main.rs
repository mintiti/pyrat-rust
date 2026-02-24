use std::collections::HashMap;
use std::path::PathBuf;
use std::time::Duration;

use clap::Parser;
use serde::Serialize;
use tokio::net::TcpListener;
use tokio::sync::mpsc;
use tracing::info;

use pyrat::game::builder::GameConfig;
use pyrat::game::game_logic::GameState;

use pyrat_host::game_loop::{
    accept_connections, build_owned_match_config, launch_bots, run_playing, run_setup, BotConfig,
    MatchEvent, MatchResult, MatchSetup, PlayerEntry, PlayingConfig, SetupTiming,
};
use pyrat_host::session::messages::SessionMsg;
use pyrat_host::session::SessionConfig;
use pyrat_host::wire::{Player, TimingMode};

// ── CLI ──────────────────────────────────────────────

#[derive(Parser)]
#[command(name = "pyrat-headless", about = "Run a headless PyRat match")]
struct Cli {
    /// Command to run player 1's bot
    player1_cmd: String,
    /// Command to run player 2's bot
    player2_cmd: String,

    #[arg(long, default_value_t = 21)]
    width: u8,
    #[arg(long, default_value_t = 15)]
    height: u8,
    #[arg(long, default_value_t = 41)]
    cheese: u16,
    #[arg(long)]
    seed: Option<u64>,
    #[arg(long, default_value_t = 300)]
    max_turns: u16,
    #[arg(long, default_value_t = 1000)]
    move_timeout_ms: u32,
    #[arg(long, default_value_t = 10000)]
    preprocessing_timeout_ms: u32,
    #[arg(long, default_value_t = 30000)]
    startup_timeout_ms: u32,
    /// Named preset: tiny, small, medium, large, huge
    #[arg(long)]
    preset: Option<String>,
    /// Write game record JSON to this file
    #[arg(long)]
    output: Option<PathBuf>,
}

impl Cli {
    fn build_game_config(&self) -> GameConfig {
        if let Some(ref preset) = self.preset {
            let mut config = GameConfig::preset(preset).unwrap_or_else(|e| {
                eprintln!("Error: {e}");
                std::process::exit(1);
            });
            // Explicit flags override preset
            // We can't mutate a GameConfig directly, so we rebuild with overrides
            // For now, trust the preset; users who want full control skip --preset
            let _ = &mut config;
            config
        } else {
            GameConfig::classic(self.width, self.height, self.cheese)
        }
    }
}

// ── Game record ──────────────────────────────────────

#[derive(Serialize)]
struct GameRecord {
    width: u8,
    height: u8,
    max_turns: u16,
    seed: Option<u64>,
    players: Vec<PlayerRecord>,
    turns: Vec<TurnRecord>,
    result: ResultRecord,
}

#[derive(Serialize)]
struct PlayerRecord {
    player: String,
    name: String,
    author: String,
}

#[derive(Serialize)]
struct TurnRecord {
    turn: u16,
    p1_action: u8,
    p2_action: u8,
    p1_position: (u8, u8),
    p2_position: (u8, u8),
    p1_score: f32,
    p2_score: f32,
    cheese_remaining: usize,
}

#[derive(Serialize)]
struct ResultRecord {
    winner: String,
    player1_score: f32,
    player2_score: f32,
    turns_played: u16,
}

fn build_game_record(game: &GameState, seed: Option<u64>, events: Vec<MatchEvent>) -> GameRecord {
    let mut players = Vec::new();
    let mut turns = Vec::new();
    let mut result = None;

    for event in events {
        match event {
            MatchEvent::BotIdentified {
                player,
                name,
                author,
            } => {
                let player_name = if player == Player::Player1 {
                    "Player1"
                } else {
                    "Player2"
                };
                players.push(PlayerRecord {
                    player: player_name.to_string(),
                    name,
                    author,
                });
            },
            MatchEvent::TurnPlayed {
                turn,
                state,
                p1_action,
                p2_action,
            } => {
                turns.push(TurnRecord {
                    turn,
                    p1_action: p1_action.0,
                    p2_action: p2_action.0,
                    p1_position: state.player1_position,
                    p2_position: state.player2_position,
                    p1_score: state.player1_score,
                    p2_score: state.player2_score,
                    cheese_remaining: state.cheese.len(),
                });
            },
            MatchEvent::MatchOver { result: r } => {
                result = Some(r);
            },
            _ => {},
        }
    }

    let match_result = result.expect("MatchOver event missing");
    let winner = match match_result.result {
        pyrat_host::wire::GameResult::Player1 => "Player1",
        pyrat_host::wire::GameResult::Player2 => "Player2",
        _ => "Draw",
    };

    GameRecord {
        width: game.width(),
        height: game.height(),
        max_turns: game.max_turns(),
        seed,
        players,
        turns,
        result: ResultRecord {
            winner: winner.to_string(),
            player1_score: match_result.player1_score,
            player2_score: match_result.player2_score,
            turns_played: match_result.turns_played,
        },
    }
}

// ── Main ─────────────────────────────────────────────

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    let cli = Cli::parse();

    // 1. Build game
    let game_config = cli.build_game_config();
    let mut game = game_config.create(cli.seed).unwrap_or_else(|e| {
        eprintln!("Error creating game: {e}");
        std::process::exit(1);
    });

    info!(
        width = game.width(),
        height = game.height(),
        cheese = game.total_cheese(),
        max_turns = game.max_turns(),
        "game created"
    );

    // 2. Build match config
    let match_config = build_owned_match_config(
        &game,
        TimingMode::Wait,
        cli.move_timeout_ms,
        cli.preprocessing_timeout_ms,
    );

    // 3. Build setup
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let bot_configs = vec![
        BotConfig {
            run_command: cli.player1_cmd.clone(),
            working_dir: cwd.clone(),
            agent_id: "player1".into(),
        },
        BotConfig {
            run_command: cli.player2_cmd.clone(),
            working_dir: cwd,
            agent_id: "player2".into(),
        },
    ];

    let setup = MatchSetup {
        players: vec![
            PlayerEntry {
                player: Player::Player1,
                agent_id: "player1".into(),
            },
            PlayerEntry {
                player: Player::Player2,
                agent_id: "player2".into(),
            },
        ],
        match_config,
        bot_options: HashMap::new(),
        timing: SetupTiming {
            startup_timeout: Duration::from_millis(u64::from(cli.startup_timeout_ms)),
            preprocessing_timeout: Duration::from_millis(u64::from(cli.preprocessing_timeout_ms)),
        },
    };

    // 4. Bind TCP listener
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("failed to bind TCP listener");
    let port = listener.local_addr().unwrap().port();
    info!(port, "listening for bot connections");

    // 5. Create event channel
    let (event_tx, mut event_rx) = mpsc::unbounded_channel::<MatchEvent>();

    // 6. Launch bots
    let _bot_processes = launch_bots(&bot_configs, port).unwrap_or_else(|e| {
        eprintln!("Error launching bots: {e}");
        std::process::exit(1);
    });

    // 7. Accept connections
    let (game_tx, mut game_rx) = mpsc::channel::<SessionMsg>(64);
    let session_config = SessionConfig::default();
    tokio::spawn(accept_connections(listener, game_tx, session_config));

    // 8. Spawn event consumer
    let event_consumer = tokio::spawn(async move {
        let mut events = Vec::new();
        while let Some(event) = event_rx.recv().await {
            events.push(event);
        }
        events
    });

    // 9. Run setup
    let setup_result = run_setup(&setup, &mut game_rx, Some(&event_tx))
        .await
        .unwrap_or_else(|e| {
            eprintln!("Setup failed: {e}");
            std::process::exit(1);
        });

    // 10. Run playing
    let playing_config = PlayingConfig {
        move_timeout: Duration::from_millis(u64::from(cli.move_timeout_ms)),
    };
    let match_result = run_playing(
        &mut game,
        &setup_result.sessions,
        &mut game_rx,
        &playing_config,
        Some(&event_tx),
    )
    .await
    .unwrap_or_else(|e| {
        eprintln!("Playing failed: {e}");
        std::process::exit(1);
    });

    // 11. Shutdown sessions
    for s in &setup_result.sessions {
        let _ = s
            .cmd_tx
            .send(pyrat_host::session::messages::HostCommand::Shutdown)
            .await;
    }
    tokio::time::sleep(Duration::from_millis(100)).await;

    // 12. Collect events
    drop(event_tx);
    let events = event_consumer.await.expect("event consumer panicked");

    // 13. Print result
    print_result(&match_result);

    // 14. Write game record
    if let Some(ref output_path) = cli.output {
        let record = build_game_record(&game, cli.seed, events);
        let json = serde_json::to_string_pretty(&record).expect("JSON serialization failed");
        std::fs::write(output_path, json).unwrap_or_else(|e| {
            eprintln!("Error writing game record: {e}");
            std::process::exit(1);
        });
        info!(path = %output_path.display(), "game record written");
    }
}

fn print_result(result: &MatchResult) {
    let winner = match result.result {
        pyrat_host::wire::GameResult::Player1 => "Player 1 wins!",
        pyrat_host::wire::GameResult::Player2 => "Player 2 wins!",
        _ => "Draw!",
    };
    println!("{winner}");
    println!(
        "Score: {:.1} - {:.1} ({} turns)",
        result.player1_score, result.player2_score, result.turns_played
    );
}
