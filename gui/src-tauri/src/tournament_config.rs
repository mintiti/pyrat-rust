//! Pinned V1 tournament conditions for the GUI.
//!
//! These mirror the committed ladder (`botpack/ladder.toml`) so GUI ratings
//! stay comparable to the CI ladder and alpharat — the one deliberate
//! divergence is `MAX_PARALLEL` (the ladder uses 2 for its CI runtime budget;
//! an interactive machine gets 4). They are fixed constants, not user inputs:
//! the launch screen quotes them ("tiny preset · 200 ms/move · 4 concurrent")
//! but never lets the user edit them in V1. Keeping them here means a drift in
//! CLI / GUI-match defaults can never silently change a GUI tournament.

use std::time::Duration;

use pyrat::game::builder::GameConfig;
use pyrat_eval::SeatPolicy;
use pyrat_eval_store::EloOptions;
use pyrat_host::match_host::{PlayingConfig, SetupTiming};
use pyrat_host::wire::TimingMode;
use pyrat_orchestrator::{OrchestratorConfig, Timing};

/// Game shape: the `tiny` preset (11×9, 13 cheese, 150 max turns).
pub const PRESET: &str = "tiny";

// Timing (milliseconds). Tight budgets are the pinned "ladder conditions":
// search bots deepen until the per-turn budget runs out, so it drives runtime.
pub const MOVE_TIMEOUT_MS: u32 = 200;
pub const PREPROCESSING_TIMEOUT_MS: u32 = 2_000;
pub const CONFIGURE_TIMEOUT_MS: u32 = 5_000;
pub const NETWORK_GRACE_MS: u32 = 50;
/// Large because botpack bots cold-build `cargo run --release` on first launch.
pub const STARTUP_TIMEOUT_MS: u32 = 120_000;

/// Paired, seat-debiased schedule: each maze is played both seatings, so the
/// matchup runs `2 × MAZES_PER_MATCHUP` games. 8 mazes (16 games) is
/// like-for-like with the prior single-seat 15, but de-biases seat per maze
/// so GUI ratings are demo-defensible. Pinned (no launch control) like the
/// other V1 conditions.
pub const MAZES_PER_MATCHUP: u32 = 8;
pub const TARGET_GAMES_PER_MATCHUP: u32 = 2 * MAZES_PER_MATCHUP;
pub const SEAT_POLICY: SeatPolicy = SeatPolicy::Paired;
pub const MAX_FAILURES_PER_PAIR: u32 = 1;

/// The one resource value — and the deliberate divergence from the ladder's 2.
pub const MAX_PARALLEL: u32 = 4;

/// Elo baseline. `greedy` at 1000 matches alpharat's benchmark convention.
pub const ANCHOR_ELO: f64 = 1000.0;
pub const BASELINE_ANCHOR_ID: &str = "greedy";

/// Whether a player id is the conventional baseline. Matches both the ladder's
/// bare `"greedy"` and discovery's namespaced `"pyrat/greedy"` (the GUI uses
/// agent_ids as player ids).
fn is_baseline(id: &str) -> bool {
    id == BASELINE_ANCHOR_ID || id.rsplit('/').next() == Some(BASELINE_ANCHOR_ID)
}

/// Build the pinned game config.
pub fn game_config() -> Result<GameConfig, String> {
    GameConfig::preset(PRESET)
}

/// Per-match timing handed to the planner config (distinct from the
/// orchestrator's setup/playing timing below).
pub fn per_match_timing() -> Timing {
    Timing {
        mode: TimingMode::Wait,
        move_timeout_ms: MOVE_TIMEOUT_MS,
        preprocessing_timeout_ms: PREPROCESSING_TIMEOUT_MS,
    }
}

/// Orchestrator config with the pinned setup/playing timeouts and concurrency.
pub fn orchestrator_config() -> OrchestratorConfig {
    OrchestratorConfig {
        max_parallel: MAX_PARALLEL.max(1) as usize,
        setup_timing: SetupTiming {
            configure_timeout: Duration::from_millis(u64::from(CONFIGURE_TIMEOUT_MS)),
            preprocessing_timeout: Duration::from_millis(u64::from(PREPROCESSING_TIMEOUT_MS)),
        },
        playing_config: PlayingConfig {
            move_timeout: Duration::from_millis(u64::from(MOVE_TIMEOUT_MS)),
            network_grace: Duration::from_millis(u64::from(NETWORK_GRACE_MS)),
            ..Default::default()
        },
        handshake_timeout: Duration::from_millis(u64::from(STARTUP_TIMEOUT_MS)),
        ..Default::default()
    }
}

/// Pick the Elo anchor: the fixed pool baseline, never the measured bot.
///
/// Prefer `greedy` if it's in the pool (the ladder convention, so the dashed
/// line at 1000 means the same thing everywhere); otherwise the first
/// non-target player by slot order. The target is excluded so the measured
/// bot floats relative to the baseline rather than being pinned at 1000.
/// `player_ids` must be in slot order. Returns `None` only if the pool is
/// empty after excluding the target (caller guarantees ≥2 players).
pub fn derive_anchor(player_ids: &[String], target: Option<&str>) -> Option<String> {
    let eligible = || player_ids.iter().filter(|id| Some(id.as_str()) != target);
    eligible()
        .find(|id| is_baseline(id))
        .or_else(|| eligible().next())
        .cloned()
}

/// Elo options anchored on the derived baseline at 1000.
pub fn elo_options(anchor: &str) -> EloOptions {
    EloOptions::new(anchor).anchor_elo(ANCHOR_ELO)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tiny_preset_builds() {
        assert!(game_config().is_ok());
    }

    #[test]
    fn anchor_prefers_greedy_when_present() {
        let pool = vec![
            "my-bot".to_string(),
            "greedy".to_string(),
            "search".to_string(),
        ];
        // Gauntlet: my-bot is the target and must never be the anchor.
        assert_eq!(
            derive_anchor(&pool, Some("my-bot")).as_deref(),
            Some("greedy")
        );
    }

    #[test]
    fn anchor_falls_back_to_first_non_target() {
        let pool = vec![
            "my-bot".to_string(),
            "search".to_string(),
            "bfs".to_string(),
        ];
        assert_eq!(
            derive_anchor(&pool, Some("my-bot")).as_deref(),
            Some("search")
        );
    }

    #[test]
    fn anchor_round_robin_no_target() {
        let pool = vec!["alpha".to_string(), "greedy".to_string()];
        assert_eq!(derive_anchor(&pool, None).as_deref(), Some("greedy"));
        let pool2 = vec!["alpha".to_string(), "beta".to_string()];
        assert_eq!(derive_anchor(&pool2, None).as_deref(), Some("alpha"));
    }

    #[test]
    fn anchor_matches_namespaced_greedy() {
        // Discovery uses agent_ids like "pyrat/greedy" as player ids.
        let pool = vec![
            "my-bot".to_string(),
            "pyrat/search".to_string(),
            "pyrat/greedy".to_string(),
        ];
        assert_eq!(
            derive_anchor(&pool, Some("my-bot")).as_deref(),
            Some("pyrat/greedy")
        );
    }
}
