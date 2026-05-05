//! Setup failures must call `sink.on_match_failed` with
//! `durable_record: true`. Pins fix #1 and the contract at
//! `plan.md:160` (schema NULL-`started_at` for spawn-failures) and
//! `plan.md:440` (the "honest invariant" lists kill-9 / required-flush /
//! cancellation / panic as the only `durable_record: false` cases).
//!
//! Trigger: a `Subprocess` slot pointing at a nonexistent `working_dir`.
//! `launch_bots` calls `Command::current_dir(...).spawn()`, which fails
//! at spawn time when the directory is missing — the deterministic path
//! to `SetupError::Launch -> FailureReason::SpawnFailed`. (A
//! "nonexistent binary" command would launch successfully via `sh -c`
//! and surface as `HandshakeTimeout` instead.)
//!
//! Scope: this is the *single* subprocess test in this crate; it
//! exercises the launch setup-failure path. TCP accept/handshake and
//! mid-match disconnect coverage stay in `pyrat-host`'s integration
//! tests per the existing scope choice.

mod common;

use std::sync::Arc;
use std::time::Duration;

use pyrat_orchestrator::{
    AdHocDescriptor, CompositeSink, DriverEvent, FailureReason, MatchSink, Orchestrator,
    OrchestratorConfig, SinkRole,
};
use tokio::time::timeout;

use common::{subprocess_matchup_with_bad_workdir, RecordedCall, RecordingSink};

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn subprocess_spawn_failure_calls_on_match_failed_durably() {
    let cfg = OrchestratorConfig {
        max_parallel: 1,
        ..Default::default()
    };
    let recorder = Arc::new(RecordingSink::new());
    let recorder_dyn: Arc<dyn MatchSink<AdHocDescriptor>> = recorder.clone();
    let composite: Arc<dyn MatchSink<AdHocDescriptor>> =
        Arc::new(CompositeSink::new(vec![(SinkRole::Required, recorder_dyn)]));
    let (orch, mut driver_rx) = Orchestrator::new(cfg, composite);

    orch.submit(subprocess_matchup_with_bad_workdir(0))
        .await
        .expect("submit");

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
        matches!(failure.reason, FailureReason::SpawnFailed),
        "expected SpawnFailed, got {:?}",
        failure.reason,
    );
    assert!(
        failure.durable_record,
        "setup failures must be durable per plan.md:440 honest invariant",
    );
    assert!(
        failure.started_at.is_none(),
        "spawn failure never started — started_at must be None",
    );

    // Sink saw exactly one on_match_failed call, and it carried the
    // durable flag. No on_match_started precedes it (setup failed before
    // we'd have called it). No on_match_finished.
    let calls = recorder.snapshot();
    let started_count = calls
        .iter()
        .filter(|c| matches!(c, RecordedCall::Started { .. }))
        .count();
    let finished_count = calls
        .iter()
        .filter(|c| matches!(c, RecordedCall::Finished { .. }))
        .count();
    let failed_calls: Vec<_> = calls
        .iter()
        .filter(|c| matches!(c, RecordedCall::Failed { .. }))
        .collect();

    assert_eq!(
        started_count, 0,
        "on_match_started must not fire for a setup failure",
    );
    assert_eq!(
        finished_count, 0,
        "on_match_finished must not fire for a setup failure",
    );
    assert_eq!(
        failed_calls.len(),
        1,
        "expected exactly one on_match_failed call, got {failed_calls:?}",
    );
    if let RecordedCall::Failed {
        durable_record,
        reason_debug,
        ..
    } = failed_calls[0]
    {
        assert!(
            *durable_record,
            "on_match_failed durable_record must be true for setup failures",
        );
        assert!(
            reason_debug.contains("SpawnFailed"),
            "expected reason debug to contain SpawnFailed, got {reason_debug}",
        );
    }
}
