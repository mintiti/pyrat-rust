//! `Orchestrator::shutdown(self)` actually awaits the run-loop drain
//! instead of aborting the task. Pins fix #3: the previous
//! `AbortOnDropHandle` shape would let `shutdown()` resolve before the
//! per-match task published its terminal `MatchFailed { Cancelled }`.
//!
//! Drain `driver_rx` concurrently from a background task — `shutdown`
//! waits on the run-loop, the run-loop waits on per-match tasks to
//! drain, and per-match tasks publish their terminal through
//! `publish_lifecycle` which awaits `driver_tx.send`. Without a draining
//! consumer the bounded mpsc backpressure-stalls the terminal publish
//! and the test deadlocks.

mod common;

use std::sync::Arc;
use std::time::Duration;

use pyrat_orchestrator::{
    AdHocDescriptor, DriverEvent, FailureReason, NoOpSink, Orchestrator, OrchestratorConfig,
};
use tokio::time::timeout;

use common::{embedded_matchup, mock_factory, slow_factory};

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn shutdown_awaits_per_match_terminal_publication() {
    let cfg = OrchestratorConfig {
        max_parallel: 1,
        ..Default::default()
    };
    let sink: Arc<NoOpSink<AdHocDescriptor>> = Arc::new(NoOpSink::new());
    let (orch, mut driver_rx) = Orchestrator::new(cfg, sink);

    orch.submit(embedded_matchup(0, mock_factory(), slow_factory()))
        .await
        .expect("submit");

    // Background drain so `publish_lifecycle` from the per-match task
    // doesn't block on a full `driver_tx.send`. Returns the collected
    // events when the channel closes (run-loop drops `driver_tx` via
    // dropping `inner` once the orchestrator drops, which can only
    // happen after `shutdown` resolves).
    let drain = tokio::spawn(async move {
        let mut events = Vec::new();
        while let Some(ev) = driver_rx.recv().await {
            events.push(ev);
        }
        events
    });

    // Wait for the match to be in the playing phase before triggering
    // shutdown — gives the per-match task something to actually cancel.
    // Done by polling state until `running` is non-empty (or a brief
    // timeout). Imperfect but sufficient: even if shutdown fires before
    // playing starts, the test still pins the post-shutdown invariants.
    let _ = timeout(Duration::from_secs(3), async {
        let mut state_rx = orch.state();
        loop {
            if !state_rx.borrow().running.is_empty() {
                return;
            }
            if state_rx.changed().await.is_err() {
                return;
            }
        }
    })
    .await;

    timeout(Duration::from_secs(10), orch.shutdown())
        .await
        .expect("shutdown completes within 10s");

    let events = timeout(Duration::from_secs(5), drain)
        .await
        .expect("drain completes")
        .expect("drain task");

    let saw_cancelled_terminal = events.iter().any(|ev| {
        matches!(
            ev,
            DriverEvent::MatchFailed { failure }
                if matches!(failure.reason, FailureReason::Cancelled)
        )
    });
    assert!(
        saw_cancelled_terminal,
        "expected MatchFailed {{ Cancelled }} in drained events; got {events:?}",
    );
}
