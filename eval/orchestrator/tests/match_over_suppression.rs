//! Broadcast subscribers must never observe `MatchEvent::MatchOver`.
//! The host's terminal is suppressed inside `run_match`; terminal signals
//! on the broadcast are `MatchFinished` / `MatchFailed` only.

mod common;

use std::sync::Arc;
use std::time::Duration;

use pyrat_host::match_host::MatchEvent;
use pyrat_orchestrator::{
    AdHocDescriptor, DriverEvent, NoOpSink, Orchestrator, OrchestratorConfig, OrchestratorEvent,
};
use tokio::time::timeout;

use common::mock_vs_mock;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn broadcast_never_emits_match_over_per_turn_event() {
    let cfg = OrchestratorConfig {
        max_parallel: 1,
        ..Default::default()
    };
    let sink: Arc<NoOpSink<AdHocDescriptor>> = Arc::new(NoOpSink::new());
    let (orch, mut driver_rx) = Orchestrator::new(cfg, sink);

    // Subscribe BEFORE submitting so no events are missed.
    let mut events = orch.events();

    orch.submit(mock_vs_mock(0)).await.expect("submit");

    // Drain driver_rx in the background until MatchFinished, while the
    // primary loop scans the broadcast for any MatchEvent::MatchOver.
    let driver_task = tokio::spawn(async move {
        loop {
            match driver_rx.recv().await.expect("driver_rx") {
                DriverEvent::MatchFinished { .. } | DriverEvent::MatchFailed { .. } => {
                    break;
                },
                _ => {},
            }
        }
    });

    let mut collected: Vec<OrchestratorEvent<AdHocDescriptor>> = Vec::new();
    let collect = async {
        // Read until we see a terminal lifecycle event on the broadcast.
        loop {
            match events.recv().await {
                Ok(ev) => {
                    let is_terminal = matches!(
                        ev,
                        OrchestratorEvent::MatchFinished { .. }
                            | OrchestratorEvent::MatchFailed { .. }
                    );
                    collected.push(ev);
                    if is_terminal {
                        break;
                    }
                },
                Err(broadcast::error::RecvError::Lagged(_)) => {
                    // Slow consumer; re-resync. Shouldn't happen in this
                    // small match, but tolerate.
                    continue;
                },
                Err(broadcast::error::RecvError::Closed) => break,
            }
        }
    };
    use tokio::sync::broadcast;
    timeout(Duration::from_secs(10), collect)
        .await
        .expect("collected events in time");
    driver_task.await.expect("driver_task");

    // Assertion: not a single OrchestratorEvent::MatchEvent variant
    // wraps a MatchEvent::MatchOver.
    for ev in &collected {
        if let OrchestratorEvent::MatchEvent { event, .. } = ev {
            assert!(
                !matches!(event, MatchEvent::MatchOver { .. }),
                "broadcast leaked MatchEvent::MatchOver; suppression broken"
            );
        }
    }

    // Sanity: there IS a terminal MatchFinished, and at least one
    // non-MatchOver MatchEvent (BotIdentified or PreprocessingStarted etc.).
    let saw_finished = collected
        .iter()
        .any(|e| matches!(e, OrchestratorEvent::MatchFinished { .. }));
    assert!(saw_finished, "expected MatchFinished on broadcast");
    let saw_some_per_turn = collected
        .iter()
        .any(|e| matches!(e, OrchestratorEvent::MatchEvent { .. }));
    assert!(
        saw_some_per_turn,
        "expected at least one per-turn MatchEvent on broadcast"
    );
}
