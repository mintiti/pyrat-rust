//! Driver-drop is fatal to the orchestrator. Pinned in `plan.md:129` and
//! `executor.rs`'s module doc: when `driver_rx` is dropped, the next
//! `publish_lifecycle` call cancels root, the run-loop unwinds, drains,
//! and exits, and subsequent `submit()` calls return `ShutDown`.
//!
//! Test shape: tiny mpsc capacities so backpressure surfaces immediately,
//! a slow_factory match so the per-match task is alive when we drop the
//! receiver, then poll `submit()` until it returns `ShutDown` (the
//! reliable signal that the run-loop has exited and `submit_rx` was
//! dropped). We avoid asserting on `state().changed()` because
//! `publish_lifecycle` returns on `DriverDropped` *before* mutating
//! state, so no state tick fires from the drop itself.

mod common;

use std::sync::Arc;
use std::time::Duration;

use pyrat_orchestrator::{
    AdHocDescriptor, NoOpSink, Orchestrator, OrchestratorConfig, OrchestratorError,
};
use tokio::time::{sleep, timeout};

use common::{embedded_matchup, mock_factory, mock_vs_mock, slow_factory};

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn dropped_driver_rx_makes_orchestrator_shut_down() {
    let cfg = OrchestratorConfig {
        max_parallel: 1,
        submit_capacity: 1,
        driver_events_capacity: 1,
        ..Default::default()
    };
    let sink: Arc<NoOpSink<AdHocDescriptor>> = Arc::new(NoOpSink::new());
    let (orch, driver_rx) = Orchestrator::new(cfg, sink);

    // Submit a slow_factory match so a per-match task will be alive and
    // attempting to publish lifecycle events when we drop the driver.
    orch.submit(embedded_matchup(0, mock_factory(), slow_factory()))
        .await
        .expect("first submit");

    // Give the per-match task a moment to publish MatchQueued (which
    // drains the first slot of driver_events_capacity=1) so the next
    // publish will block on send. Not strictly required for the test to
    // pass — the polling loop below tolerates either ordering — but
    // makes the failure mode clear: drop_driver triggers DriverDropped on
    // the in-flight task's *next* publish.
    sleep(Duration::from_millis(50)).await;
    drop(driver_rx);

    // Poll submit() until it returns ShutDown. Any new attempt sees
    // either backpressure (run-loop hasn't exited yet) or ShutDown
    // (run-loop exited and dropped submit_rx). The first state where
    // submit_rx is gone is the reliable signal we want.
    let became_shut_down = timeout(Duration::from_secs(5), async {
        loop {
            match orch.submit(mock_vs_mock(1)).await {
                Ok(()) => {
                    // Submit accepted before the run-loop noticed the
                    // drop. Wait briefly and retry.
                    sleep(Duration::from_millis(20)).await;
                },
                Err(OrchestratorError::ShutDown) => return,
            }
        }
    })
    .await;

    assert!(
        became_shut_down.is_ok(),
        "submit() never returned ShutDown after driver_rx was dropped",
    );
}
