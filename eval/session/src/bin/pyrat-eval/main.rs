//! `pyrat-eval` evaluation CLI.
//!
//! Subcommands:
//! - `run-one`: runs a single match between two subprocess bots, optionally
//!   writing a JSON game record.
//! - `tournament run`: round-robin or gauntlet between N bots, driven by
//!   flags or a TOML config, with Elo standings out (see
//!   `tournament_resolve` for the precedence rules).

mod game_config_build;
mod orchestrator_config_build;
mod tournament_config;
mod tournament_resolve;
mod tournament_run;
mod tournament_save;

use clap::{Args, Parser, Subcommand};
use std::num::NonZeroU16;
use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::Arc;
use tracing::info;
use tracing_subscriber::EnvFilter;

use pyrat::game::builder::GameConfig;

use pyrat_eval::LegacyRecordSink;
use pyrat_host::match_host::MatchResult;
use pyrat_host::wire::{GameResult, TimingMode};
use pyrat_orchestrator::{
    AdHocDescriptor, DriverEvent, MatchSink, Matchup, NoOpSink, Orchestrator, OrchestratorConfig,
    PlayerSpec, Timing,
};

use crate::game_config_build::{GameShape, ResolvedGame};
use crate::tournament_resolve::ResolvedTiming;

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
    /// Tournament operations.
    Tournament(Box<TournamentArgs>),
}

#[derive(Args)]
struct TournamentArgs {
    #[command(subcommand)]
    command: TournamentSubcommand,
}

#[derive(Subcommand)]
enum TournamentSubcommand {
    /// Run a tournament: round-robin or gauntlet.
    Run(RunArgs),
}

/// CLI args for `pyrat-eval tournament run`.
///
/// Every override field is `Option<T>` with no clap default. The
/// distinction between "user passed --games 5" and "user did not pass
/// --games" is what lets the resolver layer precedence (defaults → config
/// → flags). If clap filled defaults here, the resolver would silently
/// shadow config values.
///
/// `--bot id=working_dir` is shorthand: command defaults to `cargo run
/// --release`. For arbitrary commands (Python, env vars, custom flags),
/// use a TOML config with `command = "..."` instead.
#[derive(Args)]
pub(crate) struct RunArgs {
    /// Bot shorthand: `id=working_dir`. Defaults command to `cargo run --release`.
    /// Repeatable. Replaces the config's [[players]] list entirely (no merging).
    /// For arbitrary commands, use --config with a TOML.
    #[arg(long = "bot", value_parser = parse_bot_arg)]
    pub(crate) bots: Vec<BotArg>,

    /// `round-robin` or `gauntlet`. Underscore form also accepted. Default: round-robin.
    #[arg(long)]
    pub(crate) format: Option<String>,

    /// Target games per matchup (round-robin) or per opponent (gauntlet). Default: 5.
    #[arg(long)]
    pub(crate) games: Option<u32>,

    /// Max consecutive failures per matchup before the planner stops retrying. Default: 1.
    #[arg(long)]
    pub(crate) max_failures: Option<u32>,

    /// Max matches in flight at once. Default: 2.
    #[arg(long)]
    pub(crate) max_parallel: Option<u32>,

    /// Deterministic tournament seed. Implicit (random) by default.
    #[arg(long)]
    pub(crate) seed: Option<u64>,

    /// Load a TOML config file. Flags override config values.
    #[arg(long)]
    pub(crate) config: Option<PathBuf>,

    /// Materialize the resolved tournament back to a TOML file before running.
    /// Mutually exclusive with `--resume`.
    #[arg(long, conflicts_with = "resume")]
    pub(crate) save_as: Option<PathBuf>,

    /// Resume an existing tournament by id. Pass the same --config/flags the
    /// tournament was created with — the store carries results, not the spec.
    /// Mutually exclusive with `--save-as`.
    #[arg(long)]
    pub(crate) resume: Option<i64>,

    /// Write a Level-A results JSON summary to this path after the tournament finishes.
    #[arg(long)]
    pub(crate) results_json: Option<PathBuf>,

    /// SQLite store path. Default: `<config-stem>.db` next to --config, else `ratings.db` in CWD.
    #[arg(long)]
    pub(crate) store_path: Option<PathBuf>,

    /// Directory to write per-match replay JSON. Default: no replay sink.
    #[arg(long)]
    pub(crate) replay_dir: Option<PathBuf>,

    // ── Game config (mutually exclusive: preset OR width+height+cheese) ──
    /// Named preset: tiny, small, medium, large, huge, open, asymmetric.
    /// Default: tiny (when no game flags or [game] section are given).
    #[arg(long)]
    pub(crate) preset: Option<String>,
    /// Board width (use with --height and --cheese; excludes --preset).
    #[arg(long)]
    pub(crate) width: Option<u8>,
    /// Board height (use with --width and --cheese; excludes --preset).
    #[arg(long)]
    pub(crate) height: Option<u8>,
    /// Cheese count (use with --width and --height; excludes --preset).
    #[arg(long)]
    pub(crate) cheese: Option<u16>,
    /// Symmetric maze (only valid with --width/--height/--cheese; presets pin their own).
    #[arg(long)]
    pub(crate) symmetric: Option<bool>,
    /// Override max_turns (defaults to preset's value, or 300 for custom dims).
    #[arg(long)]
    pub(crate) max_turns: Option<NonZeroU16>,

    // ── Timing overrides ──
    /// Per-move think budget in ms. Default: 1000.
    #[arg(long)]
    pub(crate) move_timeout_ms: Option<u32>,
    /// Preprocessing budget in ms before turn 1. Default: 10000.
    #[arg(long)]
    pub(crate) preprocessing_timeout_ms: Option<u32>,
    /// How long a bot may take to start and connect, in ms. Default: 30000.
    #[arg(long)]
    pub(crate) startup_timeout_ms: Option<u32>,
    /// Configure-phase handshake budget in ms. Default: 5000.
    #[arg(long)]
    pub(crate) configure_timeout_ms: Option<u32>,
    /// Network grace in ms added on top of the think deadline. Default: 50.
    #[arg(long)]
    pub(crate) network_grace_ms: Option<u32>,

    // ── Gauntlet selection ──
    /// Challenger id (gauntlet format only).
    #[arg(long)]
    pub(crate) challenger: Option<String>,
    /// Opponent id (gauntlet format only). Repeatable.
    #[arg(long = "opponent")]
    pub(crate) opponents: Vec<String>,

    // ── Elo anchor overrides ──
    /// Anchor player id for Elo. Defaults to first player (round-robin) or challenger (gauntlet).
    #[arg(long)]
    pub(crate) anchor: Option<String>,
    /// Anchor Elo rating. Default: 1000.0.
    #[arg(long)]
    pub(crate) anchor_elo: Option<f64>,
}

#[derive(Clone, Debug)]
pub(crate) struct BotArg {
    pub(crate) id: String,
    pub(crate) working_dir: PathBuf,
}

fn parse_bot_arg(raw: &str) -> Result<BotArg, String> {
    let (id, working_dir) = raw
        .split_once('=')
        .ok_or_else(|| format!("expected `id=working_dir`, got `{raw}`"))?;
    if id.is_empty() {
        return Err(format!("empty bot id in `{raw}`"));
    }
    if working_dir.is_empty() {
        return Err(format!("empty working_dir in `{raw}`"));
    }
    Ok(BotArg {
        id: id.into(),
        working_dir: PathBuf::from(working_dir),
    })
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
    /// Override max_turns (defaults to the preset's value, or 300 for --width/--height).
    #[arg(long)]
    max_turns: Option<NonZeroU16>,
    #[arg(long, default_value_t = tournament_resolve::DEFAULT_MOVE_TIMEOUT_MS)]
    move_timeout_ms: u32,
    #[arg(long, default_value_t = tournament_resolve::DEFAULT_PREP_TIMEOUT_MS)]
    preprocessing_timeout_ms: u32,
    #[arg(long, default_value_t = tournament_resolve::DEFAULT_STARTUP_TIMEOUT_MS)]
    startup_timeout_ms: u32,
    #[arg(long, default_value_t = tournament_resolve::DEFAULT_CONFIGURE_TIMEOUT_MS)]
    configure_timeout_ms: u32,
    /// Named preset: tiny, small, medium, large, huge, open, asymmetric
    #[arg(long)]
    preset: Option<String>,
    /// Write game record JSON to this file
    #[arg(long)]
    output: Option<PathBuf>,
    /// Network grace period in ms added on top of the think deadline
    #[arg(long, default_value_t = tournament_resolve::DEFAULT_NETWORK_GRACE_MS)]
    network_grace_ms: u32,
}

fn build_game_config(args: &RunOneArgs) -> Result<GameConfig, String> {
    let shape = match &args.preset {
        Some(preset) => GameShape::Preset {
            name: preset.clone(),
        },
        None => GameShape::Custom {
            width: args.width,
            height: args.height,
            cheese: args.cheese,
            symmetric: true,
        },
    };
    game_config_build::build_game_config(&ResolvedGame {
        shape,
        max_turns: args.max_turns,
    })
}

fn build_orchestrator_config(args: &RunOneArgs) -> OrchestratorConfig {
    let timing = ResolvedTiming {
        move_timeout_ms: args.move_timeout_ms,
        preprocessing_timeout_ms: args.preprocessing_timeout_ms,
        startup_timeout_ms: args.startup_timeout_ms,
        configure_timeout_ms: args.configure_timeout_ms,
        network_grace_ms: args.network_grace_ms,
    };
    orchestrator_config_build::build_orchestrator_config(&timing, 1)
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
    // Diagnostics go to stderr: stdout carries results (run-one score
    // lines, tournament standings) that scripts parse, and tournament
    // mode interleaves enough per-match warns to corrupt it otherwise.
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .with_writer(std::io::stderr)
        .init();

    let cli = Cli::parse();
    let result = match cli.command {
        Command::RunOne(args) => run_one(args).await.map(|()| ExitCode::SUCCESS),
        Command::Tournament(args) => match args.command {
            TournamentSubcommand::Run(args) => run_tournament(args).await,
        },
    };
    match result {
        Ok(code) => code,
        Err(e) => {
            eprintln!("Error: {e}");
            ExitCode::FAILURE
        },
    }
}

async fn run_one(args: RunOneArgs) -> Result<(), Box<dyn std::error::Error>> {
    let seed = args.seed.unwrap_or_else(rand::random::<u64>);
    let game_config = build_game_config(&args)?;
    // Fail fast before launching bots: surface invalid game configs (e.g.
    // too many cheese for the board) without spawning subprocesses. The
    // orchestrator re-runs `create()` with the same seed inside run_match;
    // duplicated work is one maze/cheese gen.
    let total_cheese = game_config
        .create(Some(seed))
        .map_err(|e| format!("invalid game config: {e}"))?
        .total_cheese();
    info!(
        width = game_config.width(),
        height = game_config.height(),
        max_turns = game_config.max_turns(),
        cheese = total_cheese,
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
        Err(failure) => Err(format!("Match failed: {}", failure.reason).into()),
    }
}

/// Tournament-run entry. Resolves args → runs the tournament. Standings
/// rendering happens inside `run_tournament_main`; the exit code is
/// derived from the attempt counts it returns.
async fn run_tournament(args: RunArgs) -> Result<ExitCode, Box<dyn std::error::Error>> {
    let mut seed_gen = masked_random_seed;
    let resolved = tournament_resolve::resolve(args, &mut seed_gen)?;
    let counts = tournament_run::run_tournament_main(resolved).await?;
    // A tournament that "finished" with zero successful games (every
    // matchup exhausted its retry budget — typically bots that fail to
    // start) must not look like success to scripts: `pyrat-eval ... &&
    // publish` would silently publish an empty ladder.
    Ok(if counts.success == 0 {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    })
}

/// Default seed generator. Mirrors `plan::matchup_seed`'s 63-bit mask so
/// the value fits SQLite's signed INTEGER column.
fn masked_random_seed() -> u64 {
    rand::random::<u64>() & (i64::MAX as u64)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_bot_arg_splits_on_first_equals() {
        let parsed = parse_bot_arg("greedy=botpack/greedy").unwrap();
        assert_eq!(parsed.id, "greedy");
        assert_eq!(parsed.working_dir, PathBuf::from("botpack/greedy"));
    }

    #[test]
    fn parse_bot_arg_allows_equals_in_working_dir() {
        let parsed = parse_bot_arg("bot=/tmp/path=with=equals").unwrap();
        assert_eq!(parsed.id, "bot");
        assert_eq!(parsed.working_dir, PathBuf::from("/tmp/path=with=equals"));
    }

    #[test]
    fn parse_bot_arg_rejects_missing_equals() {
        let err = parse_bot_arg("just-an-id").unwrap_err();
        assert!(err.contains("expected `id=working_dir`"));
    }

    #[test]
    fn parse_bot_arg_rejects_empty_id_or_dir() {
        assert!(parse_bot_arg("=botpack/x")
            .unwrap_err()
            .contains("empty bot id"));
        assert!(parse_bot_arg("a=")
            .unwrap_err()
            .contains("empty working_dir"));
    }
}
