//! `pyrat-eval` evaluation CLI.
//!
//! Subcommands:
//! - `run-one`: runs a single match between two subprocess bots, optionally
//!   writing a JSON game record.

use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::Arc;
use std::time::Duration;

use clap::{Parser, Subcommand};
use tracing::info;
use tracing_subscriber::EnvFilter;

use pyrat::game::builder::GameConfig;

use pyrat_eval::LegacyRecordSink;
use pyrat_host::match_host::{MatchResult, PlayingConfig, SetupTiming};
use pyrat_host::wire::{GameResult, TimingMode};
use pyrat_orchestrator::{
    AdHocDescriptor, DriverEvent, MatchSink, Matchup, NoOpSink, Orchestrator, OrchestratorConfig,
    PlayerSpec, Timing,
};

// ── CLI ──────────────────────────────────────────────

#[derive(Parser)]
#[command(name = "pyrat-eval", about = "PyRat evaluation CLI")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Run a single match between two bot commands.
    RunOne(RunOneArgs),
}

#[derive(Parser)]
struct RunOneArgs {
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

fn build_game_config(args: &RunOneArgs) -> Result<GameConfig, String> {
    if let Some(ref preset) = args.preset {
        GameConfig::preset(preset)
    } else {
        Ok(GameConfig::classic(args.width, args.height, args.cheese))
    }
}

fn build_orchestrator_config(args: &RunOneArgs) -> OrchestratorConfig {
    OrchestratorConfig {
        max_parallel: 1,
        setup_timing: SetupTiming {
            configure_timeout: Duration::from_millis(u64::from(args.configure_timeout_ms)),
            preprocessing_timeout: Duration::from_millis(u64::from(args.preprocessing_timeout_ms)),
        },
        playing_config: PlayingConfig {
            move_timeout: Duration::from_millis(u64::from(args.move_timeout_ms)),
            network_grace: Duration::from_millis(u64::from(args.network_grace_ms)),
            ..Default::default()
        },
        handshake_timeout: Duration::from_millis(u64::from(args.startup_timeout_ms)),
        ..Default::default()
    }
}

fn build_matchup(
    args: &RunOneArgs,
    game_config: GameConfig,
    seed: u64,
    orch: &Orchestrator<AdHocDescriptor>,
) -> Result<Matchup<AdHocDescriptor>, String> {
    let cwd = std::env::current_dir().map_err(|e| format!("cannot read cwd: {e}"))?;
    let descriptor = AdHocDescriptor {
        match_id: orch.allocate_id(),
        seed,
        planned_at: std::time::SystemTime::now(),
    };
    Ok(Matchup {
        descriptor,
        game_config,
        players: [
            PlayerSpec::Subprocess {
                agent_id: "player1".into(),
                command: args.player1_cmd.clone(),
                working_dir: Some(cwd.clone()),
            },
            PlayerSpec::Subprocess {
                agent_id: "player2".into(),
                command: args.player2_cmd.clone(),
                working_dir: Some(cwd),
            },
        ],
        timing: Timing {
            mode: TimingMode::Wait,
            move_timeout_ms: args.move_timeout_ms,
            preprocessing_timeout_ms: args.preprocessing_timeout_ms,
        },
    })
}

fn print_result(result: &MatchResult) {
    let label = match result.result {
        GameResult::Player1 => "Player 1 wins!",
        GameResult::Player2 => "Player 2 wins!",
        GameResult::Draw => "Draw!",
        unknown => {
            tracing::warn!(?unknown, "unexpected GameResult variant");
            "Draw!"
        },
    };
    println!("{label}");
    println!(
        "Score: {:.1} - {:.1} ({} turns)",
        result.player1_score, result.player2_score, result.turns_played
    );
}

// ── Main ─────────────────────────────────────────────

#[tokio::main]
async fn main() -> ExitCode {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();

    let cli = Cli::parse();
    let result = match cli.command {
        Command::RunOne(args) => run_one(args).await,
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("Error: {e}");
            ExitCode::FAILURE
        },
    }
}

async fn run_one(args: RunOneArgs) -> Result<(), Box<dyn std::error::Error>> {
    let seed = args.seed.unwrap_or_else(rand::random::<u64>);
    let game_config = build_game_config(&args)?;
    info!(
        width = game_config.width(),
        height = game_config.height(),
        max_turns = game_config.max_turns(),
        seed,
        "game configured"
    );

    let sink: Arc<dyn MatchSink<AdHocDescriptor>> = match args.output.clone() {
        Some(path) => Arc::new(LegacyRecordSink::new(path)),
        None => Arc::new(NoOpSink::<AdHocDescriptor>::new()),
    };

    let orch_config = build_orchestrator_config(&args);
    let (orch, mut driver_rx) = Orchestrator::<AdHocDescriptor>::new(orch_config, sink);
    let matchup = build_matchup(&args, game_config, seed, &orch)?;
    orch.submit(matchup).await?;

    let terminal = loop {
        match driver_rx.recv().await {
            Some(DriverEvent::MatchFinished { outcome }) => break Ok(outcome),
            Some(DriverEvent::MatchFailed { failure }) => break Err(failure),
            Some(event) => tracing::debug!(?event, "non-terminal driver event"),
            None => return Err("orchestrator closed driver channel before terminal".into()),
        }
    };
    orch.shutdown().await;

    match terminal {
        Ok(outcome) => {
            print_result(&outcome.result);
            if let Some(ref path) = args.output {
                info!(path = %path.display(), "game record written");
            }
            Ok(())
        },
        Err(failure) => Err(format!("Match failed: {:?}", failure.reason).into()),
    }
}
