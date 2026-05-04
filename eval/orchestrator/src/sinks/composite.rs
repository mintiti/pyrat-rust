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
//!
//! ## Demotion cleanup
//!
//! When a `Required` child errors anywhere (`on_match_event`,
//! `on_match_finished`, `on_match_failed`, or `on_match_started` after the
//! first child has already signalled OK), the composite calls
//! [`MatchSink::on_match_abandoned`] on every child that previously received
//! a successful `on_match_started`, *except* the broken sink itself. This
//! lets stateful sinks (e.g. replay buffers keyed by `MatchId`) release
//! per-match state when no terminal callback will fire.
//!
//! Per-match tracking lives in `match_states: Mutex<HashMap<MatchId, ...>>`.
//! The mutex is `parking_lot` (sync) and held only across `HashMap` ops,
//! never across an `await` of a child callback.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use async_trait::async_trait;
use parking_lot::Mutex;
use tracing::warn;

use pyrat_host::match_host::MatchEvent;
use pyrat_host::player::PlayerIdentity;

use crate::descriptor::Descriptor;
use crate::id::MatchId;
use crate::outcome::{MatchFailure, MatchOutcome};
use crate::sink::{MatchSink, SinkError, SinkRole};

/// Per-match composite state: which children received a successful
/// `on_match_started`, in the order they were called. Used to drive
/// `on_match_abandoned` if cleanup runs.
struct MatchState {
    seen_started: Vec<usize>,
}

/// Composes a list of `(role, sink)` children behind one `MatchSink`.
pub struct CompositeSink<D: Descriptor> {
    children: Vec<(SinkRole, Arc<dyn MatchSink<D>>)>,
    optional_errors: AtomicU64,
    /// Per-match state keyed by `MatchId`. Entries are inserted on a
    /// successful `on_match_started`, removed on a clean terminal or on
    /// demotion cleanup. Sync mutex; never held across `.await` of a child.
    match_states: Mutex<HashMap<MatchId, MatchState>>,
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
            match_states: Mutex::new(HashMap::new()),
        }
    }

    /// Total count of optional-sink errors logged across this composite's
    /// lifetime. Includes errors from `on_match_abandoned` calls; every
    /// abandoned-cleanup error is logged at `warn` regardless of the
    /// originating sink's role.
    pub fn optional_error_count(&self) -> u64 {
        self.optional_errors.load(Ordering::Relaxed)
    }

    fn record_optional_error(&self, callback: &'static str, err: &SinkError) {
        self.optional_errors.fetch_add(1, Ordering::Relaxed);
        warn!(callback = callback, error = %err, "optional sink error");
    }

    fn take_match_state(&self, id: MatchId) -> Option<MatchState> {
        self.match_states.lock().remove(&id)
    }

    /// Call `on_match_abandoned` on each indexed child, skipping
    /// `broken_idx`. Errors are logged + counted and never propagate.
    async fn abandon_indices<I: IntoIterator<Item = usize>>(
        &self,
        id: MatchId,
        indices: I,
        broken_idx: Option<usize>,
    ) {
        for idx in indices {
            if Some(idx) == broken_idx {
                continue;
            }
            let child = &self.children[idx].1;
            if let Err(e) = child.on_match_abandoned(id).await {
                self.record_optional_error("on_match_abandoned", &e);
            }
        }
    }

    /// Take the per-match state and abandon every recorded child except
    /// the broken one.
    async fn dispatch_abandoned(&self, id: MatchId, broken_idx: Option<usize>) {
        let Some(state) = self.take_match_state(id) else {
            return;
        };
        self.abandon_indices(id, state.seen_started, broken_idx)
            .await;
    }
}

#[async_trait]
impl<D: Descriptor> MatchSink<D> for CompositeSink<D> {
    async fn on_match_started(
        &self,
        descriptor: &D,
        players: &[PlayerIdentity; 2],
    ) -> Result<(), SinkError> {
        let id = descriptor.match_id();
        let mut seen: Vec<usize> = Vec::new();
        for (idx, (role, child)) in self.children.iter().enumerate() {
            match child.on_match_started(descriptor, players).await {
                Ok(()) => seen.push(idx),
                Err(e) => match role {
                    SinkRole::Required => {
                        // Cleanup priors that already started successfully.
                        // Optionals among them carry per-match state (e.g.
                        // replay buffers); abandoned releases it. The broken
                        // child wasn't recorded in `seen`, so no broken_idx
                        // exclude needed.
                        self.abandon_indices(id, seen, None).await;
                        return Err(e);
                    },
                    SinkRole::Optional => {
                        self.record_optional_error("on_match_started", &e);
                    },
                },
            }
        }
        self.match_states
            .lock()
            .insert(id, MatchState { seen_started: seen });
        Ok(())
    }

    async fn on_match_event(&self, id: MatchId, event: &MatchEvent) -> Result<(), SinkError> {
        for (idx, (role, child)) in self.children.iter().enumerate() {
            match child.on_match_event(id, event).await {
                Ok(()) => {},
                Err(e) => match role {
                    SinkRole::Required => {
                        self.dispatch_abandoned(id, Some(idx)).await;
                        return Err(e);
                    },
                    SinkRole::Optional => {
                        self.record_optional_error("on_match_event", &e);
                    },
                },
            }
        }
        Ok(())
    }

    async fn on_match_finished(&self, outcome: &MatchOutcome<D>) -> Result<(), SinkError> {
        let id = outcome.descriptor.match_id();
        for (idx, (role, child)) in self.children.iter().enumerate() {
            match child.on_match_finished(outcome).await {
                Ok(()) => {},
                Err(e) => match role {
                    SinkRole::Required => {
                        self.dispatch_abandoned(id, Some(idx)).await;
                        return Err(e);
                    },
                    SinkRole::Optional => {
                        self.record_optional_error("on_match_finished", &e);
                    },
                },
            }
        }
        // Clean terminal: drop per-match state, no abandoned calls.
        self.take_match_state(id);
        Ok(())
    }

    async fn on_match_failed(&self, failure: &MatchFailure<D>) -> Result<(), SinkError> {
        let id = failure.descriptor.match_id();
        for (idx, (role, child)) in self.children.iter().enumerate() {
            match child.on_match_failed(failure).await {
                Ok(()) => {},
                Err(e) => match role {
                    SinkRole::Required => {
                        self.dispatch_abandoned(id, Some(idx)).await;
                        return Err(e);
                    },
                    SinkRole::Optional => {
                        self.record_optional_error("on_match_failed", &e);
                    },
                },
            }
        }
        // Clean terminal: drop per-match state.
        self.take_match_state(id);
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

    /// Stateful sink: counts `started` and `abandoned` calls. The abandoned
    /// counter must increment exactly once per match when a Required sink
    /// errors after this child saw a successful start.
    struct StatefulRecorder {
        started: Arc<AtomicU32>,
        abandoned: Arc<AtomicU32>,
    }

    #[async_trait]
    impl MatchSink<AdHocDescriptor> for StatefulRecorder {
        async fn on_match_started(
            &self,
            _: &AdHocDescriptor,
            _: &[PlayerIdentity; 2],
        ) -> Result<(), SinkError> {
            self.started.fetch_add(1, Ordering::SeqCst);
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
            Ok(())
        }
        async fn on_match_abandoned(&self, _: MatchId) -> Result<(), SinkError> {
            self.abandoned.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }
    }

    /// Required-error during `on_match_event` calls `on_match_abandoned` on
    /// every child that previously received a successful `on_match_started`,
    /// except the broken one. Pins the demotion-cleanup contract for the
    /// replay-buffer leak case.
    #[tokio::test]
    async fn required_error_during_event_calls_abandoned_on_started_priors() {
        // Required sink that succeeds on `started` but errors on `event`.
        struct EventErroringRequired {
            started: Arc<AtomicU32>,
            abandoned: Arc<AtomicU32>,
        }
        #[async_trait]
        impl MatchSink<AdHocDescriptor> for EventErroringRequired {
            async fn on_match_started(
                &self,
                _: &AdHocDescriptor,
                _: &[PlayerIdentity; 2],
            ) -> Result<(), SinkError> {
                self.started.fetch_add(1, Ordering::SeqCst);
                Ok(())
            }
            async fn on_match_event(&self, _: MatchId, _: &MatchEvent) -> Result<(), SinkError> {
                Err(SinkError {
                    source: anyhow!("event boom"),
                })
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
                Ok(())
            }
            async fn on_match_abandoned(&self, _: MatchId) -> Result<(), SinkError> {
                self.abandoned.fetch_add(1, Ordering::SeqCst);
                Ok(())
            }
        }

        let req_started = Arc::new(AtomicU32::new(0));
        let req_abandoned = Arc::new(AtomicU32::new(0));
        let opt_started = Arc::new(AtomicU32::new(0));
        let opt_abandoned = Arc::new(AtomicU32::new(0));

        let req: Arc<dyn MatchSink<AdHocDescriptor>> = Arc::new(EventErroringRequired {
            started: req_started.clone(),
            abandoned: req_abandoned.clone(),
        });
        let opt: Arc<dyn MatchSink<AdHocDescriptor>> = Arc::new(StatefulRecorder {
            started: opt_started.clone(),
            abandoned: opt_abandoned.clone(),
        });
        let composite =
            CompositeSink::new(vec![(SinkRole::Required, req), (SinkRole::Optional, opt)]);

        let desc = ad_hoc(7);
        let players = [
            PlayerIdentity {
                name: "p1".into(),
                author: "t".into(),
                agent_id: "a/p1".into(),
                slot: pyrat_host::wire::Player::Player1,
            },
            PlayerIdentity {
                name: "p2".into(),
                author: "t".into(),
                agent_id: "a/p2".into(),
                slot: pyrat_host::wire::Player::Player2,
            },
        ];
        composite
            .on_match_started(&desc, &players)
            .await
            .expect("started ok");
        assert_eq!(req_started.load(Ordering::SeqCst), 1);
        assert_eq!(opt_started.load(Ordering::SeqCst), 1);

        let result = composite
            .on_match_event(desc.match_id(), &MatchEvent::PreprocessingStarted)
            .await;
        assert!(result.is_err(), "required sink err propagates");
        // The broken Required sink must NOT receive abandoned (calling
        // methods on the broken sink remains forbidden).
        assert_eq!(req_abandoned.load(Ordering::SeqCst), 0);
        // The Optional sink saw a successful start, so it gets abandoned.
        assert_eq!(opt_abandoned.load(Ordering::SeqCst), 1);
    }

    /// Clean terminal does NOT call abandoned on any child. Pins that we
    /// only abandon when cleanup actually runs.
    #[tokio::test]
    async fn clean_terminal_does_not_call_abandoned() {
        let started = Arc::new(AtomicU32::new(0));
        let abandoned = Arc::new(AtomicU32::new(0));
        let child: Arc<dyn MatchSink<AdHocDescriptor>> = Arc::new(StatefulRecorder {
            started: started.clone(),
            abandoned: abandoned.clone(),
        });
        let composite = CompositeSink::new(vec![(SinkRole::Optional, child)]);

        let desc = ad_hoc(8);
        let players = [
            PlayerIdentity {
                name: "p1".into(),
                author: "t".into(),
                agent_id: "a/p1".into(),
                slot: pyrat_host::wire::Player::Player1,
            },
            PlayerIdentity {
                name: "p2".into(),
                author: "t".into(),
                agent_id: "a/p2".into(),
                slot: pyrat_host::wire::Player::Player2,
            },
        ];
        composite.on_match_started(&desc, &players).await.unwrap();
        composite.on_match_failed(&failure(desc)).await.unwrap();
        assert_eq!(started.load(Ordering::SeqCst), 1);
        assert_eq!(abandoned.load(Ordering::SeqCst), 0);
    }
}
