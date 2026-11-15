//! Development tool for the tournament ETA evidence loop.
//!
//! Production sessions emit raw timing traces through `pyrat-eval
//! tournament run --timing-trace`. This tool owns the deliberately separate
//! experiment layer: deterministic synthetic cases and a temporal M0/M1/M1P
//! comparison report. It never changes the live estimator or dispatch order.

mod report;
mod synthetic;

use std::path::PathBuf;

use clap::{Parser, Subcommand};

#[derive(Debug, Parser)]
#[command(name = "eta-evidence")]
#[command(about = "Build and inspect tournament ETA evidence")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Write deterministic edge-case timing traces.
    Synthetic {
        /// Directory that receives the three JSON trace files.
        #[arg(long)]
        output_dir: PathBuf,
    },
    /// Render a standalone temporal comparison from a corpus manifest.
    Report {
        /// Corpus manifest; trace paths inside it are relative to this file.
        #[arg(long)]
        manifest: PathBuf,
        /// Standalone HTML output path.
        #[arg(long)]
        output: PathBuf,
        /// Replay seed reserved for later stochastic candidates.
        #[arg(long, default_value_t = 0)]
        simulation_seed: u64,
    },
}

fn main() -> anyhow::Result<()> {
    match Cli::parse().command {
        Command::Synthetic { output_dir } => synthetic::write_cases(&output_dir),
        Command::Report {
            manifest,
            output,
            simulation_seed,
        } => report::write_report(&manifest, &output, simulation_seed),
    }
}
