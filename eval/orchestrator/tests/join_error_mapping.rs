//! `JoinError::is_panic()` from a panicked per-match task synthesises
//! `MatchFailed { reason: Panic, durable_record: false }`. Pins fix #4
//! and the mapping row at `plan.md:337`.
//!
//! Trigger: an `EmbeddedBotFactory` whose closure panics on invocation.
//! The factory is called inside `run_match`'s `BoxedEmbeddedBot(factory())`
//! site (i.e. inside the per-match task body), so the panic propagates
//! up through the per-match task and reaches `JoinSet::join_next_with_id`
//! as `JoinError::is_panic() == true`. A panic inside `EmbeddedBot::think`
//! would be caught by `EmbeddedPlayer`'s `spawn_blocking` and converted to
//! `PlayerError`, which is *not* what we want to test for the panic-path
//! mapping.

mod common;

use std::sync::Arc;
use std::time::Duration;

use pyrat_orchestrator::{
    AdHocDescriptor, DriverEvent, FailureReason, NoOpSink, Orchestrator, OrchestratorConfig,
};
use tokio::time::timeout;

use common::{embedded_matchup, mock_factory, panicking_factory};

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn factory_panic_synthesises_match_failed_panic() {
    let cfg = OrchestratorConfig {
        max_parallel: 1,
        ..Default::default()
    };
    let sink: Arc<NoOpSink<AdHocDescriptor>> = Arc::new(NoOpSink::new());
    let (orch, mut driver_rx) = Orchestrator::new(cfg, sink);

    orch.submit(embedded_matchup(0, panicking_factory(), mock_factory()))
        .await
        .expect("submit");

    let failure = timeout(Duration::from_secs(5), async {
        loop {
            match driver_rx.recv().await.expect("driver_rx") {
                DriverEvent::MatchFailed { failure } => break failure,
                DriverEvent::MatchFinished { .. } => panic!("unexpected finish"),
                _ => {},
            }
        }
    })
    .await
    .expect("MatchFailed in time");

    assert!(
        matches!(failure.reason, FailureReason::Panic),
        "expected FailureReason::Panic, got {:?}",
        failure.reason,
    );
    assert!(
        !failure.durable_record,
        "panic-stranded matches must have durable_record=false",
    );
    assert!(
        failure.started_at.is_none(),
        "synthesised panic terminal lacks started_at by design",
    );
    assert!(
        failure.players.is_none(),
        "synthesised panic terminal lacks players by design",
    );

    // Subsequent submits should still work — the run-loop didn't exit.
    // (Drop driver_rx after this last submit would exit; we don't test
    // that here.)
    drop(driver_rx);
}
