//! Executor smoke: many submissions, bounded parallelism, all complete.

mod common;

use std::sync::Arc;
use std::time::Duration;

use pyrat_orchestrator::{
    AdHocDescriptor, DriverEvent, NoOpSink, Orchestrator, OrchestratorConfig,
};
use tokio::time::timeout;

use common::mock_vs_mock;

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn five_matches_max_parallel_two_all_finish() {
    let cfg = OrchestratorConfig {
        max_parallel: 2,
        ..Default::default()
    };
    let sink: Arc<NoOpSink<AdHocDescriptor>> = Arc::new(NoOpSink::new());
    let (orch, mut driver_rx) = Orchestrator::new(cfg, sink);

    for i in 0..5 {
        orch.submit(mock_vs_mock(i)).await.expect("submit");
    }

    let mut finished = 0_u64;
    let drain = async {
        while finished < 5 {
            match driver_rx.recv().await.expect("driver_rx") {
                DriverEvent::MatchFinished { .. } => finished += 1,
                DriverEvent::MatchFailed { failure } => {
                    panic!("unexpected match failure: {:?}", failure.reason)
                },
                _ => {},
            }
        }
    };
    timeout(Duration::from_secs(20), drain)
        .await
        .expect("drain in time");

    assert_eq!(finished, 5);

    // The watch update happens *after* the lifecycle mpsc send by design,
    // so wait for state to converge rather than read it eagerly.
    let mut state_rx = orch.state();
    let converge = async {
        loop {
            let s = state_rx.borrow_and_update().clone();
            if s.finished == 5 && s.running.is_empty() && s.queued == 0 {
                return s;
            }
            state_rx.changed().await.expect("state watch closed");
        }
    };
    let final_state = timeout(Duration::from_secs(5), converge)
        .await
        .expect("state convergence");

    assert_eq!(final_state.finished, 5);
    assert_eq!(final_state.failed, 0);
    assert!(final_state.running.is_empty());
    assert_eq!(final_state.queued, 0);
}
