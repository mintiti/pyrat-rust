//! Required-sink failure on `on_match_started` aborts before
//! `Match::new`. Player handles are explicitly closed (subprocess /
//! dispatcher cleanup), terminal is `MatchFailed { SinkFlushError,
//! durable_record: false, started_at: None }`.

mod common;

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

struct StartedFailureSink;

#[async_trait]
impl MatchSink<AdHocDescriptor> for StartedFailureSink {
    async fn on_match_started(
        &self,
        _: &AdHocDescriptor,
        _: &[PlayerIdentity; 2],
    ) -> Result<(), SinkError> {
        Err(SinkError {
            source: anyhow!("started boom"),
        })
    }
    async fn on_match_event(&self, _: MatchId, _: &MatchEvent) -> Result<(), SinkError> {
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
async fn required_sink_started_error_demotes_with_started_at_none() {
    let cfg = OrchestratorConfig {
        max_parallel: 1,
        ..Default::default()
    };
    let bad: Arc<dyn MatchSink<AdHocDescriptor>> = Arc::new(StartedFailureSink);
    let composite: Arc<dyn MatchSink<AdHocDescriptor>> =
        Arc::new(CompositeSink::new(vec![(SinkRole::Required, bad)]));
    let (orch, mut driver_rx) = Orchestrator::new(cfg, composite);

    orch.submit(mock_vs_mock(0)).await.expect("submit");

    let failure = async {
        loop {
            match driver_rx.recv().await.expect("driver_rx") {
                DriverEvent::MatchFailed { failure } => break failure,
                DriverEvent::MatchFinished { .. } => panic!("unexpected finish"),
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
    assert!(
        failure.started_at.is_none(),
        "started_at must be None when sink rejected the pre-Match callback"
    );
    // Player identities ARE known at this stage (post-handshake).
    assert!(failure.players.is_some());
}
