//! Cancel-mid-match: `Match::run` is dropped from the cancel arm; the
//! per-match task emits `MatchFailed { Cancelled, durable_record: false }`.

mod common;

use std::sync::Arc;
use std::time::Duration;

use pyrat_orchestrator::{
    AdHocDescriptor, DriverEvent, FailureReason, NoOpSink, Orchestrator, OrchestratorConfig,
};
use tokio::time::timeout;

use common::{embedded_matchup, mock_factory, slow_factory};

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn cancel_after_match_started_yields_cancelled_failure() {
    let cfg = OrchestratorConfig {
        max_parallel: 1,
        ..Default::default()
    };
    let sink: Arc<NoOpSink<AdHocDescriptor>> = Arc::new(NoOpSink::new());
    let (orch, mut driver_rx) = Orchestrator::new(cfg, sink);

    // Mock vs Slow: Slow blocks forever in `think` until the host signals
    // stop. Forces the run-loop to sit in the playing phase when we cancel.
    orch.submit(embedded_matchup(0, mock_factory(), slow_factory()))
        .await
        .expect("submit");

    // Wait for MatchStarted so we know we're past setup.
    let wait_started = async {
        loop {
            match driver_rx.recv().await.expect("driver_rx") {
                DriverEvent::MatchStarted { .. } => break,
                DriverEvent::MatchFinished { .. } => {
                    panic!("match finished before we could cancel")
                },
                DriverEvent::MatchFailed { failure } => {
                    panic!("match failed before we could cancel: {:?}", failure.reason)
                },
                _ => {},
            }
        }
    };
    timeout(Duration::from_secs(5), wait_started)
        .await
        .expect("MatchStarted in time");

    // Now cancel and observe the terminal.
    orch.abort();

    let term = async {
        loop {
            match driver_rx.recv().await.expect("driver_rx after abort") {
                DriverEvent::MatchFailed { failure } => break failure,
                DriverEvent::MatchFinished { .. } => panic!("unexpected finish after cancel"),
                _ => {},
            }
        }
    };
    let failure = timeout(Duration::from_secs(5), term)
        .await
        .expect("MatchFailed in time after cancel");

    assert!(
        matches!(failure.reason, FailureReason::Cancelled),
        "expected Cancelled, got {:?}",
        failure.reason
    );
    assert!(
        !failure.durable_record,
        "cancelled match must have durable_record=false"
    );
}
