mod check;
mod manifest;

use std::path::PathBuf;
use std::process::ExitCode;

use clap::Parser;

use check::{CheckReport, PhaseStatus};

#[derive(Parser)]
#[command(name = "pyrat-check", about = "Smoke test a PyRat bot")]
struct Cli {
    /// Path to bot directory containing bot.toml
    #[arg(default_value = ".")]
    path: PathBuf,

    /// Output results as JSON
    #[arg(long)]
    json: bool,
}

#[tokio::main]
async fn main() -> ExitCode {
    let cli = Cli::parse();
    let report = check::run_check(&cli.path).await;

    if cli.json {
        print_json(&report);
    } else {
        print_human(&report);
    }

    if report.passed {
        ExitCode::from(0)
    } else {
        ExitCode::from(1)
    }
}

fn print_human(report: &CheckReport) {
    println!("pyrat-check: {} ({})\n", report.bot_name, report.agent_id);

    let mut total_ms: u64 = 0;
    for phase in &report.phases {
        total_ms += phase.duration_ms;
        let (tag, detail) = match &phase.status {
            PhaseStatus::Pass { detail } => ("PASS", detail.as_str()),
            PhaseStatus::Warn { detail } => ("WARN", detail.as_str()),
            PhaseStatus::Fail { detail } => ("FAIL", detail.as_str()),
            PhaseStatus::Skip { detail } => ("SKIP", detail.as_str()),
        };

        let duration = if phase.duration_ms > 0 {
            format!(" ({}ms)", phase.duration_ms)
        } else {
            String::new()
        };

        println!("  [{tag}] {:<12} {detail}{duration}", phase.name);
    }

    println!();
    if report.passed {
        println!("  All checks passed ({:.1}s)", total_ms as f64 / 1000.0);
    } else {
        println!("  Check failed ({:.1}s)", total_ms as f64 / 1000.0);
    }
}

fn print_json(report: &CheckReport) {
    let json = serde_json::to_string_pretty(report).expect("JSON serialization failed");
    println!("{json}");
}
