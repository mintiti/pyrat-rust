//! End-to-end test: an Optional `ReplaySink` paired with an erroring
//! `Required` sink must see `on_match_abandoned` so its per-match buffer
//! is released. Pins the leak case the composite's demotion-cleanup path
//! prevents.

mod common;

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use anyhow::anyhow;
use async_trait::async_trait;
use pyrat_host::match_host::MatchEvent;
use pyrat_host::player::PlayerIdentity;
use pyrat_orchestrator::{
    AdHocDescriptor, CompositeSink, DriverEvent, FailureReason, MatchFailure, MatchId,
    MatchOutcome, MatchSink, MemoryWriter, Orchestrator, OrchestratorConfig, ReplaySink, SinkError,
    SinkRole,
};
use tokio::time::timeout;

use common::mock_vs_mock;

/// Required sink that errors on the very first event. Mimics a store that
/// lost its DB handle mid-match.
struct EventErrorSink {
    events_seen: AtomicU64,
}

impl EventErrorSink {
    fn new() -> Self {
        Self {
            events_seen: AtomicU64::new(0),
        }
    }
}

#[async_trait]
impl MatchSink<AdHocDescriptor> for EventErrorSink {
    async fn on_match_started(
        &self,
        _: &AdHocDescriptor,
        _: &[PlayerIdentity; 2],
    ) -> Result<(), SinkError> {
        Ok(())
    }
    async fn on_match_event(&self, _: MatchId, _: &MatchEvent) -> Result<(), SinkError> {
        let n = self.events_seen.fetch_add(1, Ordering::SeqCst) + 1;
        if n == 1 {
            return Err(SinkError {
                source: anyhow!("event boom"),
            });
        }
        Ok(())
    }
    async fn on_match_finished(&self, _: &MatchOutcome<AdHocDescriptor>) -> Result<(), SinkError> {
        Ok(())
    }
    async fn on_match_failed(&self, _: &MatchFailure<AdHocDescriptor>) -> Result<(), SinkError> {
        Ok(())
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn required_event_error_releases_replay_buffer_via_abandoned() {
    let cfg = OrchestratorConfig {
        max_parallel: 1,
        ..Default::default()
    };

    let writer = Arc::new(MemoryWriter::new());
    let replay = Arc::new(ReplaySink::new(writer.clone()));
    let replay_dyn: Arc<dyn MatchSink<AdHocDescriptor>> = replay.clone();
    let store_dyn: Arc<dyn MatchSink<AdHocDescriptor>> = Arc::new(EventErrorSink::new());

    let composite: Arc<dyn MatchSink<AdHocDescriptor>> = Arc::new(CompositeSink::new(vec![
        (SinkRole::Required, store_dyn),
        (SinkRole::Optional, replay_dyn),
    ]));
    let (orch, mut driver_rx) = Orchestrator::new(cfg, composite);

    orch.submit(mock_vs_mock(0)).await.expect("submit");

    let failure = async {
        loop {
            match driver_rx.recv().await.expect("driver_rx") {
                DriverEvent::MatchFailed { failure } => break failure,
                DriverEvent::MatchFinished { .. } => {
                    panic!("expected sink-flush failure, got finish")
                },
                _ => {},
            }
        }
    };
    let failure = timeout(Duration::from_secs(10), failure)
        .await
        .expect("MatchFailed in time");

    assert!(
        matches!(failure.reason, FailureReason::SinkFlushError(_)),
        "expected SinkFlushError, got {:?}",
        failure.reason
    );

    // Replay sink received `on_match_abandoned` → its per-match HashMap
    // should be empty for this match (and overall), and no replay file
    // was written.
    assert!(
        !replay.has_buffer(MatchId(0)),
        "replay sink should have released the buffer for the failed match"
    );
    assert_eq!(replay.buffer_count(), 0);
    assert_eq!(writer.count(), 0, "no replay file should be written");
}
