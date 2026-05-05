//! When a per-match task panics *after* `sink.on_match_started` has
//! populated state in stateful child sinks (e.g. `ReplaySink`), the
//! run-loop's panic-recovery path calls `inner.sink.on_match_abandoned`.
//! The top-level sink in real configurations is a `CompositeSink`, so
//! the call must dispatch to recorded children — otherwise the trait
//! default no-op leaks the per-match buffer.
//!
//! Test shape:
//! - Required sink that succeeds on `on_match_started` but panics on
//!   `on_match_event`. The panic propagates out of the per-match task as
//!   `JoinError::is_panic() == true`.
//! - Optional `ReplaySink` whose `on_match_started` populated a buffer.
//! - Composite (Required, Optional) wired as `inner.sink`.
//!
//! After the panic, assert: `MatchFailed { Panic }` published; ReplaySink
//! buffer for the match is empty (released via composite's
//! `on_match_abandoned -> dispatch_abandoned -> child.on_match_abandoned`);
//! no replay file written.

mod common;

use std::sync::Arc;
use std::time::Duration;

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

/// Required sink: Ok on started; on the *first* `on_match_event`,
/// panic. Subsequent calls (defensive, won't be reached after panic)
/// also Ok.
struct PanicOnEventSink;

#[async_trait]
impl MatchSink<AdHocDescriptor> for PanicOnEventSink {
    async fn on_match_started(
        &self,
        _: &AdHocDescriptor,
        _: &[PlayerIdentity; 2],
    ) -> Result<(), SinkError> {
        Ok(())
    }
    async fn on_match_event(&self, _: MatchId, _: &MatchEvent) -> Result<(), SinkError> {
        panic!("test sink panic during on_match_event");
    }
    async fn on_match_finished(&self, _: &MatchOutcome<AdHocDescriptor>) -> Result<(), SinkError> {
        Ok(())
    }
    async fn on_match_failed(&self, _: &MatchFailure<AdHocDescriptor>) -> Result<(), SinkError> {
        Ok(())
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn panic_after_started_releases_composite_replay_buffer() {
    let cfg = OrchestratorConfig {
        max_parallel: 1,
        ..Default::default()
    };

    let writer = Arc::new(MemoryWriter::new());
    let replay = Arc::new(ReplaySink::new(writer.clone()));
    let replay_dyn: Arc<dyn MatchSink<AdHocDescriptor>> = replay.clone();
    let bad_dyn: Arc<dyn MatchSink<AdHocDescriptor>> = Arc::new(PanicOnEventSink);

    let composite: Arc<dyn MatchSink<AdHocDescriptor>> = Arc::new(CompositeSink::new(vec![
        (SinkRole::Required, bad_dyn),
        (SinkRole::Optional, replay_dyn),
    ]));
    let (orch, mut driver_rx) = Orchestrator::new(cfg, composite);

    orch.submit(mock_vs_mock(0)).await.expect("submit");

    let failure = timeout(Duration::from_secs(10), async {
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

    // The panic-recovery path must have routed through composite's
    // on_match_abandoned to the ReplaySink. Buffer released, no file.
    assert!(
        !replay.has_buffer(MatchId(0)),
        "replay sink buffer must be released after panic",
    );
    assert_eq!(replay.buffer_count(), 0);
    assert_eq!(writer.count(), 0, "no replay file should be written");
}
