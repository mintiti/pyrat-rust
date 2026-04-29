mod record;

use std::path::PathBuf;
use std::time::Duration;

use clap::Parser;
use tokio::net::TcpListener;
use tokio::sync::mpsc;
use tracing::{info, info_span, warn, Instrument};

use pyrat::game::builder::GameConfig;

use pyrat_host::game_loop::{build_match_config, launch_bots, BotConfig};
use pyrat_host::match_host::{Match, MatchEvent, MatchResult, PlayingConfig, SetupTiming};
use pyrat_host::player::{accept_players, EventSink, Player as PlayerTrait};
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
    #[arg(long, default_value_t = 5000)]
    configure_timeout_ms: u32,
    /// Named preset: tiny, small, medium, large, huge
    #[arg(long)]
    preset: Option<String>,
    /// Write game record JSON to this file
    #[arg(long)]
    output: Option<PathBuf>,
    /// Network grace period in ms added on top of the think deadline
    #[arg(long, default_value_t = 50)]
    network_grace_ms: u32,
}

fn build_game_config(cli: &Cli) -> Result<GameConfig, String> {
    if let Some(ref preset) = cli.preset {
        GameConfig::preset(preset)
    } else {
        Ok(GameConfig::classic(cli.width, cli.height, cli.cheese))
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
    let game_config = build_game_config(&cli)?;
    let game = game_config.create(cli.seed)?;
    info!(
        width = game.width(),
        height = game.height(),
        cheese = game.total_cheese(),
        max_turns = game.max_turns(),
        "game created"
    );
    let match_config = build_match_config(
        &game,
        TimingMode::Wait,
        cli.move_timeout_ms,
        cli.preprocessing_timeout_ms,
    );

    // Bind before launching bots — the listener's port is injected into each
    // child's `PYRAT_HOST_PORT` env var.
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let port = listener.local_addr()?.port();
    info!(port, "listening for bot connections");

    let (event_tx, mut event_rx) = mpsc::unbounded_channel::<MatchEvent>();

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
    let mut bot_processes = launch_bots(&bot_configs, port)?;
    let stderr_handles = bot_processes.take_stderr_handles();

    let match_span = info_span!(
        "match",
        width = game.width(),
        height = game.height(),
        cheese = game.total_cheese(),
        p1 = tracing::field::Empty,
        p2 = tracing::field::Empty,
    );

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
                    Ok(_) => {},
                    Err(_) => break,
                }
            }
        });
    }
    bot_processes.start_exit_monitor(match_span.clone());

    let expected = vec![
        (Player::Player1, "player1".to_string()),
        (Player::Player2, "player2".to_string()),
    ];
    let startup_timeout = Duration::from_millis(u64::from(cli.startup_timeout_ms));
    let accepted = accept_players(
        &listener,
        &expected,
        EventSink::new(event_tx.clone()),
        startup_timeout,
    )
    .instrument(match_span.clone())
    .await?;
    let [p1, p2] = accepted;
    let p1 = p1.ok_or("player1 did not connect")?;
    let p2 = p2.ok_or("player2 did not connect")?;

    // `Match` never sees Identify (the handshake is consumed by `accept_players`),
    // so it can't emit `BotIdentified`. Surface it here from the player identities
    // so JSON records and tracing spans get populated.
    for player in [&p1, &p2] {
        let id = PlayerTrait::identity(player);
        match id.slot {
            Player::Player1 => {
                match_span.record("p1", id.name.as_str());
            },
            Player::Player2 => {
                match_span.record("p2", id.name.as_str());
            },
            _ => {},
        }
        let _ = event_tx.send(MatchEvent::BotIdentified {
            player: id.slot,
            name: id.name.clone(),
            author: id.author.clone(),
        });
    }

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

    let match_runner = Match::new(
        game,
        [Box::new(p1), Box::new(p2)],
        match_config,
        [vec![], vec![]],
        SetupTiming {
            configure_timeout: Duration::from_millis(u64::from(cli.configure_timeout_ms)),
            preprocessing_timeout: Duration::from_millis(u64::from(cli.preprocessing_timeout_ms)),
        },
        PlayingConfig {
            move_timeout: Duration::from_millis(u64::from(cli.move_timeout_ms)),
            network_grace: Duration::from_millis(u64::from(cli.network_grace_ms)),
            ..Default::default()
        },
        Some(event_tx.clone()),
    );
    // `Match::run` does setup → play → close, consuming both players on its way out.
    let match_result = match_runner.run().instrument(match_span).await?;

    // Past this point, bot exits are expected; demote the exit monitor's warnings.
    bot_processes.mark_game_over();

    drop(event_tx);
    let events = event_consumer.await.expect("event consumer panicked");

    print_result(&match_result);
    if let Some(ref output_path) = cli.output {
        let record = record::build(cli.seed, events, &match_result);
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
