//! Required-sink failure on `on_match_finished` demotes to
//! `MatchFailed { SinkFlushError, durable_record: false }` instead of a
//! clean `MatchFinished`.

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

struct FlushErrorSink {
    finished_calls: AtomicU64,
}

impl FlushErrorSink {
    fn new() -> Self {
        Self {
            finished_calls: AtomicU64::new(0),
        }
    }
}

#[async_trait]
impl MatchSink<AdHocDescriptor> for FlushErrorSink {
    async fn on_match_started(
        &self,
        _: &AdHocDescriptor,
        _: &[PlayerIdentity; 2],
    ) -> Result<(), SinkError> {
        Ok(())
    }
    async fn on_match_event(&self, _: MatchId, _: &MatchEvent) -> Result<(), SinkError> {
        Ok(())
    }
    async fn on_match_finished(&self, _: &MatchOutcome<AdHocDescriptor>) -> Result<(), SinkError> {
        self.finished_calls.fetch_add(1, Ordering::SeqCst);
        Err(SinkError {
            source: anyhow!("flush boom"),
        })
    }
    async fn on_match_failed(&self, _: &MatchFailure<AdHocDescriptor>) -> Result<(), SinkError> {
        Ok(())
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn required_sink_finished_error_demotes_to_sink_flush_error() {
    let cfg = OrchestratorConfig {
        max_parallel: 1,
        ..Default::default()
    };
    let sink_inner = Arc::new(FlushErrorSink::new());
    let composite_child: Arc<dyn MatchSink<AdHocDescriptor>> = sink_inner.clone();
    let composite: Arc<dyn MatchSink<AdHocDescriptor>> = Arc::new(CompositeSink::new(vec![(
        SinkRole::Required,
        composite_child,
    )]));
    let (orch, mut driver_rx) = Orchestrator::new(cfg, composite);

    orch.submit(mock_vs_mock(0)).await.expect("submit");

    let failure = async {
        loop {
            match driver_rx.recv().await.expect("driver_rx") {
                DriverEvent::MatchFailed { failure } => break failure,
                DriverEvent::MatchFinished { .. } => {
                    panic!("expected failure due to sink flush error")
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
    assert!(!failure.durable_record);
    assert_eq!(
        sink_inner.finished_calls.load(Ordering::SeqCst),
        1,
        "sink.on_match_finished should have been called exactly once"
    );
}
