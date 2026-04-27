use std::collections::HashMap;
use std::path::PathBuf;
use std::time::Duration;

use clap::Parser;
use serde::Serialize;
use tokio::net::TcpListener;
use tokio::sync::mpsc;
use tracing::{info, info_span, warn, Instrument};

use pyrat::game::builder::GameConfig;

use pyrat_host::game_loop::{
    accept_connections, build_match_config, launch_bots, run_playing, run_setup, BotConfig,
    MatchEvent, MatchResult, MatchSetup, PlayerEntry, PlayingConfig, SetupTiming,
};
use pyrat_host::session::messages::SessionMsg;
use pyrat_host::session::SessionConfig;
use pyrat_host::wire::{GameResult, Player, TimingMode};

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
    /// Think margin: how much a bot's self-reported think_ms may exceed move_timeout (fraction, e.g. 0.10 = 10%)
    #[arg(long, default_value_t = 0.10)]
    think_margin: f32,
    /// Network grace period in ms added on top of the think deadline
    #[arg(long, default_value_t = 50)]
    network_grace_ms: u32,
}

fn build_game_config(cli: &Cli) -> Result<GameConfig, String> {
    if let Some(ref preset) = cli.preset {
        let config = GameConfig::preset(preset)?;
        // TODO: allow CLI flags to override preset fields
        Ok(config)
    } else {
        Ok(GameConfig::classic(cli.width, cli.height, cli.cheese))
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
    p1_think_ms: u32,
    p2_think_ms: u32,
}

#[derive(Serialize)]
struct ResultRecord {
    winner: String,
    player1_score: f32,
    player2_score: f32,
    turns_played: u16,
}

fn result_label(result: GameResult) -> &'static str {
    match result {
        GameResult::Player1 => "Player1",
        GameResult::Player2 => "Player2",
        GameResult::Draw => "Draw",
        unknown => {
            warn!(?unknown, "unexpected GameResult variant");
            "Draw"
        },
    }
}

fn build_game_record(
    seed: Option<u64>,
    events: Vec<MatchEvent>,
    match_result: &MatchResult,
) -> GameRecord {
    let mut players = Vec::new();
    let mut turns = Vec::new();
    let mut width: u8 = 0;
    let mut height: u8 = 0;
    let mut max_turns: u16 = 0;

    for event in events {
        match event {
            MatchEvent::MatchStarted { config } => {
                width = config.width;
                height = config.height;
                max_turns = config.max_turns;
            },
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
                state,
                p1_action,
                p2_action,
                p1_think_ms,
                p2_think_ms,
            } => {
                turns.push(TurnRecord {
                    turn: state.turn,
                    p1_action: p1_action as u8,
                    p2_action: p2_action as u8,
                    p1_position: (state.player1_position.x, state.player1_position.y),
                    p2_position: (state.player2_position.x, state.player2_position.y),
                    p1_score: state.player1_score,
                    p2_score: state.player2_score,
                    cheese_remaining: state.cheese.len(),
                    p1_think_ms,
                    p2_think_ms,
                });
            },
            _ => {},
        }
    }

    GameRecord {
        width,
        height,
        max_turns,
        seed,
        players,
        turns,
        result: ResultRecord {
            winner: result_label(match_result.result).to_string(),
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

    if let Err(e) = run(cli).await {
        eprintln!("Error: {e}");
        std::process::exit(1);
    }
}

async fn run(cli: Cli) -> Result<(), Box<dyn std::error::Error>> {
    // 1. Build game
    let game_config = build_game_config(&cli)?;
    let mut game = game_config.create(cli.seed)?;

    info!(
        width = game.width(),
        height = game.height(),
        cheese = game.total_cheese(),
        max_turns = game.max_turns(),
        "game created"
    );

    // 2. Build match config
    let match_config = build_match_config(
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
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let port = listener.local_addr().unwrap().port();
    info!(port, "listening for bot connections");

    // 5. Create event channel
    let (event_tx, mut event_rx) = mpsc::unbounded_channel::<MatchEvent>();

    // 6. Launch bots
    let mut bot_processes = launch_bots(&bot_configs, port)?;
    let stderr_handles = bot_processes.take_stderr_handles();

    // 7. Accept connections
    let (game_tx, mut game_rx) = mpsc::channel::<SessionMsg>(64);
    let session_config = SessionConfig::default();
    tokio::spawn(accept_connections(listener, game_tx, session_config));

    // 8. Spawn event consumer
    let event_consumer = tokio::spawn(async move {
        let mut events = Vec::new();
        let mut last_p1_score: f32 = 0.0;
        let mut last_p2_score: f32 = 0.0;
        while let Some(event) = event_rx.recv().await {
            if let MatchEvent::TurnPlayed { ref state, .. } = event {
                if state.player1_score != last_p1_score || state.player2_score != last_p2_score {
                    last_p1_score = state.player1_score;
                    last_p2_score = state.player2_score;
                    info!(
                        turn = state.turn,
                        p1_score = state.player1_score,
                        p2_score = state.player2_score,
                        cheese = state.cheese.len(),
                        "score update"
                    );
                }
            }
            events.push(event);
        }
        events
    });

    // 9. Match span — covers setup + playing
    let match_span = info_span!(
        "match",
        width = game.width(),
        height = game.height(),
        cheese = game.total_cheese(),
        p1 = tracing::field::Empty,
        p2 = tracing::field::Empty,
    );

    // 9a. Start bot process monitoring
    for (agent_id, stderr) in stderr_handles {
        let span = match_span.clone();
        tokio::task::spawn_blocking(move || {
            use std::io::BufRead;
            let _guard = span.enter();
            let reader = std::io::BufReader::new(stderr);
            let mut count = 0usize;
            const MAX_LINES: usize = 200;
            for line in reader.lines() {
                match line {
                    Ok(text) if count < MAX_LINES => {
                        warn!(%agent_id, "{text}");
                        count += 1;
                    },
                    Ok(_) if count == MAX_LINES => {
                        warn!(%agent_id, "stderr output truncated after {MAX_LINES} lines");
                        count += 1;
                    },
                    Ok(_) => {}, // silent drain to prevent pipe backpressure
                    Err(_) => break,
                }
            }
        });
    }
    bot_processes.start_exit_monitor(match_span.clone());

    // 10. Run setup
    let setup_result = run_setup(&setup, &mut game_rx, Some(&event_tx))
        .instrument(match_span.clone())
        .await?;

    // Record bot names into the match span.
    for s in &setup_result.sessions {
        for &p in &s.controlled_players {
            match p {
                Player::Player1 => {
                    match_span.record("p1", &s.name);
                },
                Player::Player2 => {
                    match_span.record("p2", &s.name);
                },
                _ => {},
            }
        }
    }

    // 11. Run playing
    let playing_config = PlayingConfig {
        move_timeout: Duration::from_millis(u64::from(cli.move_timeout_ms)),
        think_margin: cli.think_margin,
        network_grace: Duration::from_millis(u64::from(cli.network_grace_ms)),
    };
    let match_result = run_playing(
        &mut game,
        &setup_result.sessions,
        &mut game_rx,
        &playing_config,
        Some(&event_tx),
    )
    .instrument(match_span)
    .await?;

    // 11a. Mark game over for exit monitor
    bot_processes.mark_game_over();

    // 12. Shutdown sessions and drain disconnect messages
    let session_count = setup_result.sessions.len();
    for s in &setup_result.sessions {
        let _ = s
            .cmd_tx
            .send(pyrat_host::session::messages::HostCommand::Shutdown)
            .await;
    }

    let mut disconnected = 0usize;
    let drain_deadline = tokio::time::Instant::now() + Duration::from_secs(2);
    while disconnected < session_count {
        tokio::select! {
            msg = game_rx.recv() => {
                match msg {
                    Some(SessionMsg::Disconnected { .. }) => { disconnected += 1; }
                    Some(_) => {}
                    None => break,
                }
            }
            _ = tokio::time::sleep_until(drain_deadline) => {
                warn!(
                    remaining = session_count - disconnected,
                    "shutdown drain timed out"
                );
                break;
            }
        }
    }

    // 13. Collect events
    drop(event_tx);
    let events = event_consumer.await.expect("event consumer panicked");

    // 14. Print result
    print_result(&match_result);

    // 15. Write game record
    if let Some(ref output_path) = cli.output {
        let record = build_game_record(cli.seed, events, &match_result);
        let json = serde_json::to_string_pretty(&record).expect("JSON serialization failed");
        std::fs::write(output_path, json)?;
        info!(path = %output_path.display(), "game record written");
    }

    Ok(())
}

fn print_result(result: &MatchResult) {
    let label = match result.result {
        GameResult::Player1 => "Player 1 wins!",
        GameResult::Player2 => "Player 2 wins!",
        GameResult::Draw => "Draw!",
        unknown => {
            warn!(?unknown, "unexpected GameResult variant");
            "Draw!"
        },
    };
    println!("{label}");
    println!(
        "Score: {:.1} - {:.1} ({} turns)",
        result.player1_score, result.player2_score, result.turns_played
    );
}
