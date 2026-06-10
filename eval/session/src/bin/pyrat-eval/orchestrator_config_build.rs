//! Shared orchestrator-config builder between `run-one` and `tournament run`.
//!
//! Takes resolved timing + `max_parallel` and produces an
//! `OrchestratorConfig`. Both subcommands compose the same per-match
//! `PlayingConfig` and `SetupTiming` from these knobs.

use std::time::Duration;

use pyrat_host::match_host::{PlayingConfig, SetupTiming};
use pyrat_orchestrator::OrchestratorConfig;

use crate::tournament_resolve::ResolvedTiming;

pub fn build_orchestrator_config(timing: &ResolvedTiming, max_parallel: u32) -> OrchestratorConfig {
    OrchestratorConfig {
        max_parallel: max_parallel.max(1) as usize,
        setup_timing: SetupTiming {
            configure_timeout: Duration::from_millis(u64::from(timing.configure_timeout_ms)),
            preprocessing_timeout: Duration::from_millis(u64::from(
                timing.preprocessing_timeout_ms,
            )),
        },
        playing_config: PlayingConfig {
            move_timeout: Duration::from_millis(u64::from(timing.move_timeout_ms)),
            network_grace: Duration::from_millis(u64::from(timing.network_grace_ms)),
            ..Default::default()
        },
        handshake_timeout: Duration::from_millis(u64::from(timing.startup_timeout_ms)),
        ..Default::default()
    }
}
