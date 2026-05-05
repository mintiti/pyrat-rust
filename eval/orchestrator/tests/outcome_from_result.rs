//! `MatchOutcome.result` must equal what `Match::run` returned. The match
//! is deterministic (small open maze, two MockBots staying put, max_turns=5),
//! so the result is `Draw` with both scores `0.0` and `turns_played == 5`.
//!
//! Pins the contract: outcome is built from `Match::run()`'s `MatchResult`,
//! never from a buffered `MatchEvent::MatchOver`.

mod common;

use std::sync::Arc;
use std::time::Duration;

use pyrat_host::wire::GameResult;
use pyrat_orchestrator::{
    AdHocDescriptor, DriverEvent, NoOpSink, Orchestrator, OrchestratorConfig,
};
use tokio::time::timeout;

use common::mock_vs_mock;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn outcome_result_matches_deterministic_run() {
    let cfg = OrchestratorConfig {
        max_parallel: 1,
        ..Default::default()
    };
    let sink: Arc<NoOpSink<AdHocDescriptor>> = Arc::new(NoOpSink::new());
    let (orch, mut driver_rx) = Orchestrator::new(cfg, sink);

    orch.submit(mock_vs_mock(0)).await.expect("submit");

    let outcome = async {
        loop {
            match driver_rx.recv().await.expect("driver_rx") {
                DriverEvent::MatchFinished { outcome } => break outcome,
                DriverEvent::MatchFailed { failure } => {
                    panic!("unexpected failure: {:?}", failure.reason)
                },
                _ => {},
            }
        }
    };
    let outcome = timeout(Duration::from_secs(10), outcome)
        .await
        .expect("finish in time");

    assert_eq!(outcome.result.result, GameResult::Draw);
    assert_eq!(outcome.result.player1_score, 0.0);
    assert_eq!(outcome.result.player2_score, 0.0);
    assert_eq!(outcome.result.turns_played, 5);
}
