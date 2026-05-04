//! Compose multiple sinks into one, with role-based error handling.
//!
//! Children are stored with their [`SinkRole`]. On a terminal callback:
//! - The first `Required` child to error short-circuits and returns `Err`.
//!   The executor demotes the terminal to `MatchFailed { durable_record: false }`.
//!   `Optional` children that follow are skipped: we don't write a forensic
//!   replay for a match the store couldn't record.
//! - `Optional` errors log at `warn` and increment a counter; `Ok(())` is
//!   still returned to the caller.
//!
//! The outcome is never mutated by the sink path. Losing an optional file
//! is a telemetry concern; the match really did succeed.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use async_trait::async_trait;
use tracing::warn;

use pyrat_host::match_host::MatchEvent;
use pyrat_host::player::PlayerIdentity;

use crate::descriptor::Descriptor;
use crate::id::MatchId;
use crate::outcome::{MatchFailure, MatchOutcome};
use crate::sink::{MatchSink, SinkError, SinkRole};

/// Composes a list of `(role, sink)` children behind one `MatchSink`.
pub struct CompositeSink<D: Descriptor> {
    children: Vec<(SinkRole, Arc<dyn MatchSink<D>>)>,
    optional_errors: AtomicU64,
}

impl<D: Descriptor> CompositeSink<D> {
    /// Build a composite. Children are partitioned by role: every `Required`
    /// sink is invoked before any `Optional` sink. Intra-role order is
    /// preserved from the input. This guarantees that an `Optional` sink
    /// (e.g. forensic replay) never produces side effects for a match the
    /// `Required` sink (e.g. store) ends up rejecting.
    pub fn new(children: Vec<(SinkRole, Arc<dyn MatchSink<D>>)>) -> Self {
        let (required, optional): (Vec<_>, Vec<_>) = children
            .into_iter()
            .partition(|(role, _)| *role == SinkRole::Required);
        let ordered = required.into_iter().chain(optional).collect();
        Self {
            children: ordered,
            optional_errors: AtomicU64::new(0),
        }
    }

    /// Total count of optional-sink errors logged across this composite's
    /// lifetime. Useful for asserting in tests and for telemetry wiring.
    pub fn optional_error_count(&self) -> u64 {
        self.optional_errors.load(Ordering::Relaxed)
    }

    /// Classify a single child's error by role: `Required` propagates,
    /// `Optional` logs at `warn` and bumps the counter.
    fn classify(&self, role: SinkRole, label: &'static str, err: SinkError) -> Option<SinkError> {
        match role {
            SinkRole::Required => Some(err),
            SinkRole::Optional => {
                self.optional_errors.fetch_add(1, Ordering::Relaxed);
                warn!(callback = label, error = %err, "optional sink error");
                None
            },
        }
    }
}

#[async_trait]
impl<D: Descriptor> MatchSink<D> for CompositeSink<D> {
    async fn on_match_started(
        &self,
        descriptor: &D,
        players: &[PlayerIdentity; 2],
    ) -> Result<(), SinkError> {
        for (role, child) in &self.children {
            let Err(e) = child.on_match_started(descriptor, players).await else {
                continue;
            };
            if let Some(propagate) = self.classify(*role, "on_match_started", e) {
                return Err(propagate);
            }
        }
        Ok(())
    }

    async fn on_match_event(&self, id: MatchId, event: &MatchEvent) -> Result<(), SinkError> {
        for (role, child) in &self.children {
            let Err(e) = child.on_match_event(id, event).await else {
                continue;
            };
            if let Some(propagate) = self.classify(*role, "on_match_event", e) {
                return Err(propagate);
            }
        }
        Ok(())
    }

    async fn on_match_finished(&self, outcome: &MatchOutcome<D>) -> Result<(), SinkError> {
        for (role, child) in &self.children {
            let Err(e) = child.on_match_finished(outcome).await else {
                continue;
            };
            if let Some(propagate) = self.classify(*role, "on_match_finished", e) {
                return Err(propagate);
            }
        }
        Ok(())
    }

    async fn on_match_failed(&self, failure: &MatchFailure<D>) -> Result<(), SinkError> {
        for (role, child) in &self.children {
            let Err(e) = child.on_match_failed(failure).await else {
                continue;
            };
            if let Some(propagate) = self.classify(*role, "on_match_failed", e) {
                return Err(propagate);
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicU32, Ordering};
    use std::time::SystemTime;

    use anyhow::anyhow;

    use super::*;
    use crate::descriptor::AdHocDescriptor;
    use crate::id::MatchId;
    use crate::outcome::{FailureReason, MatchFailure};
    use crate::sink::SinkError;

    fn ad_hoc(id: u64) -> AdHocDescriptor {
        AdHocDescriptor {
            match_id: MatchId(id),
            seed: 0,
            planned_at: SystemTime::UNIX_EPOCH,
        }
    }

    fn failure(desc: AdHocDescriptor) -> MatchFailure<AdHocDescriptor> {
        MatchFailure {
            descriptor: desc,
            started_at: None,
            failed_at: SystemTime::UNIX_EPOCH,
            reason: FailureReason::Internal("test".into()),
            players: None,
            durable_record: true,
        }
    }

    /// Sink that returns Ok and counts calls.
    struct CountingSink {
        calls: Arc<AtomicU32>,
    }

    #[async_trait]
    impl MatchSink<AdHocDescriptor> for CountingSink {
        async fn on_match_started(
            &self,
            _descriptor: &AdHocDescriptor,
            _players: &[PlayerIdentity; 2],
        ) -> Result<(), SinkError> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }
        async fn on_match_event(&self, _id: MatchId, _event: &MatchEvent) -> Result<(), SinkError> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }
        async fn on_match_finished(
            &self,
            _outcome: &MatchOutcome<AdHocDescriptor>,
        ) -> Result<(), SinkError> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }
        async fn on_match_failed(
            &self,
            _failure: &MatchFailure<AdHocDescriptor>,
        ) -> Result<(), SinkError> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }
    }

    /// Sink that always errors on the failed callback (used to drive the
    /// classification path).
    struct FailingSink;

    #[async_trait]
    impl MatchSink<AdHocDescriptor> for FailingSink {
        async fn on_match_started(
            &self,
            _descriptor: &AdHocDescriptor,
            _players: &[PlayerIdentity; 2],
        ) -> Result<(), SinkError> {
            Err(SinkError {
                source: anyhow!("started boom"),
            })
        }
        async fn on_match_event(&self, _id: MatchId, _event: &MatchEvent) -> Result<(), SinkError> {
            Err(SinkError {
                source: anyhow!("event boom"),
            })
        }
        async fn on_match_finished(
            &self,
            _outcome: &MatchOutcome<AdHocDescriptor>,
        ) -> Result<(), SinkError> {
            Err(SinkError {
                source: anyhow!("finished boom"),
            })
        }
        async fn on_match_failed(
            &self,
            _failure: &MatchFailure<AdHocDescriptor>,
        ) -> Result<(), SinkError> {
            Err(SinkError {
                source: anyhow!("failed boom"),
            })
        }
    }

    #[tokio::test]
    async fn required_error_short_circuits_and_returns_err() {
        let calls = Arc::new(AtomicU32::new(0));
        let trailing: Arc<dyn MatchSink<AdHocDescriptor>> = Arc::new(CountingSink {
            calls: calls.clone(),
        });
        let composite = CompositeSink::new(vec![
            (SinkRole::Required, Arc::new(FailingSink)),
            (SinkRole::Optional, trailing),
        ]);

        let desc = ad_hoc(1);
        let result = composite.on_match_failed(&failure(desc)).await;
        assert!(result.is_err(), "required sink err must propagate");
        assert_eq!(
            calls.load(Ordering::SeqCst),
            0,
            "trailing sinks must NOT be called once a Required sink errored"
        );
        assert_eq!(composite.optional_error_count(), 0);
    }

    #[tokio::test]
    async fn optional_error_does_not_propagate_and_required_still_runs() {
        let calls = Arc::new(AtomicU32::new(0));
        let required: Arc<dyn MatchSink<AdHocDescriptor>> = Arc::new(CountingSink {
            calls: calls.clone(),
        });
        let composite = CompositeSink::new(vec![
            (SinkRole::Optional, Arc::new(FailingSink)),
            (SinkRole::Required, required),
        ]);

        let desc = ad_hoc(2);
        let result = composite.on_match_failed(&failure(desc)).await;
        assert!(
            result.is_ok(),
            "optional sink err must NOT propagate to caller"
        );
        assert_eq!(
            calls.load(Ordering::SeqCst),
            1,
            "required sink must run regardless of optional sink error"
        );
        assert_eq!(composite.optional_error_count(), 1);
    }

    /// `Optional` listed before `Required` in the constructor must still see
    /// the `Required` callback invoked first. Partitioning by role at
    /// construction prevents an optional forensic sink (e.g. replay) from
    /// writing side effects for a match the required sink later rejects.
    #[tokio::test]
    async fn required_runs_before_optional_regardless_of_input_order() {
        use std::sync::Mutex;
        let order: Arc<Mutex<Vec<&'static str>>> = Arc::default();

        struct OrderRecorder {
            label: &'static str,
            order: Arc<Mutex<Vec<&'static str>>>,
        }
        #[async_trait]
        impl MatchSink<AdHocDescriptor> for OrderRecorder {
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
            async fn on_match_finished(
                &self,
                _: &MatchOutcome<AdHocDescriptor>,
            ) -> Result<(), SinkError> {
                Ok(())
            }
            async fn on_match_failed(
                &self,
                _: &MatchFailure<AdHocDescriptor>,
            ) -> Result<(), SinkError> {
                self.order.lock().unwrap().push(self.label);
                Ok(())
            }
        }

        let opt: Arc<dyn MatchSink<AdHocDescriptor>> = Arc::new(OrderRecorder {
            label: "optional",
            order: order.clone(),
        });
        let req: Arc<dyn MatchSink<AdHocDescriptor>> = Arc::new(OrderRecorder {
            label: "required",
            order: order.clone(),
        });
        let composite =
            CompositeSink::new(vec![(SinkRole::Optional, opt), (SinkRole::Required, req)]);

        let _ = composite.on_match_failed(&failure(ad_hoc(3))).await;
        assert_eq!(*order.lock().unwrap(), vec!["required", "optional"]);
    }
}
