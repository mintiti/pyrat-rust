//! Required-sink failure during `on_match_event` demotes to
//! `MatchFailed { SinkFlushError, durable_record: false }`. The engine
//! loop drains cleanly; broadcast forwarding stops; the failed sink is
//! not called again.

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
    MatchOutcome, MatchSink, Orchestrator, OrchestratorConfig, SinkError, SinkRole,
};
use tokio::time::timeout;

use common::mock_vs_mock;

/// Sink that errors on `on_match_event` after the Nth call. Counts every
/// callback so the test can assert the failed sink isn't re-invoked.
struct ErrorAfterNthEvent {
    error_after: u64,
    events_seen: AtomicU64,
    finished_calls: AtomicU64,
    failed_calls: AtomicU64,
    abandoned_calls: AtomicU64,
}

impl ErrorAfterNthEvent {
    fn new(error_after: u64) -> Self {
        Self {
            error_after,
            events_seen: AtomicU64::new(0),
            finished_calls: AtomicU64::new(0),
            failed_calls: AtomicU64::new(0),
            abandoned_calls: AtomicU64::new(0),
        }
    }
}

#[async_trait]
impl MatchSink<AdHocDescriptor> for ErrorAfterNthEvent {
    async fn on_match_started(
        &self,
        _: &AdHocDescriptor,
        _: &[PlayerIdentity; 2],
    ) -> Result<(), SinkError> {
        Ok(())
    }
    async fn on_match_event(&self, _: MatchId, _: &MatchEvent) -> Result<(), SinkError> {
        let n = self.events_seen.fetch_add(1, Ordering::SeqCst) + 1;
        if n >= self.error_after {
            return Err(SinkError {
                source: anyhow!("event boom (n={n})"),
            });
        }
        Ok(())
    }
    async fn on_match_finished(&self, _: &MatchOutcome<AdHocDescriptor>) -> Result<(), SinkError> {
        self.finished_calls.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }
    async fn on_match_failed(&self, _: &MatchFailure<AdHocDescriptor>) -> Result<(), SinkError> {
        self.failed_calls.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }
    async fn on_match_abandoned(&self, _: MatchId) -> Result<(), SinkError> {
        self.abandoned_calls.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn required_sink_event_error_demotes_to_sink_flush_error() {
    let cfg = OrchestratorConfig {
        max_parallel: 1,
        ..Default::default()
    };
    let bad = Arc::new(ErrorAfterNthEvent::new(1)); // error on the very first event
    let bad_arc: Arc<dyn MatchSink<AdHocDescriptor>> = bad.clone();
    let composite: Arc<CompositeSink<AdHocDescriptor>> =
        Arc::new(CompositeSink::new(vec![(SinkRole::Required, bad_arc)]));
    let sink: Arc<dyn MatchSink<AdHocDescriptor>> = composite;
    let (orch, mut driver_rx) = Orchestrator::new(cfg, sink);

    orch.submit(mock_vs_mock(0)).await.expect("submit");

    let failure = async {
        loop {
            match driver_rx.recv().await.expect("driver_rx") {
                DriverEvent::MatchFailed { failure } => break failure,
                DriverEvent::MatchFinished { .. } => {
                    panic!("expected failure, got MatchFinished")
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
    assert!(!failure.durable_record, "broken-sink path = no durable row");

    // The failed sink must NOT have been called again on a terminal
    // (composite skipped subsequent terminals; demotion happened inside
    // `on_match_event`).
    assert_eq!(
        bad.finished_calls.load(Ordering::SeqCst),
        0,
        "broken sink was called for on_match_finished"
    );
    assert_eq!(
        bad.failed_calls.load(Ordering::SeqCst),
        0,
        "broken sink was called for on_match_failed"
    );
}
