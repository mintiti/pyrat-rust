//! The sink seam.
//!
//! Sinks see every match event, the terminal outcome, and the failure path.
//! The orchestrator is the producer; consumers (eval store, replay JSON,
//! tests) plug in here. Sinks are classified `Required` or `Optional` at
//! composition time. Required-sink terminal failure is fatal to the
//! match's durable record; optional-sink failure is a telemetry concern.

use async_trait::async_trait;

use pyrat_host::match_host::MatchEvent;
use pyrat_host::player::PlayerIdentity;

use crate::descriptor::Descriptor;
use crate::id::MatchId;
use crate::outcome::{MatchFailure, MatchOutcome};

/// Sink role at composition time. Determines how `CompositeSink` handles
/// errors from this child.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SinkRole {
    /// A failure here means we can't claim the match was durably recorded.
    /// Composite returns `Err` so the executor can demote the terminal.
    Required,
    /// A failure here is logged at `warn` and a counter increments.
    /// The match outcome is unaffected.
    Optional,
}

/// Error returned by sink callbacks. Criticality is per-sink (controlled
/// by `SinkRole`), not per-error: `SinkError` carries only the source.
#[derive(Debug, thiserror::Error)]
#[error("sink error: {source}")]
pub struct SinkError {
    #[from]
    pub source: anyhow::Error,
}

/// Sink callback surface. The orchestrator drives `on_match_event` per host
/// event and a single `on_match_finished` *or* `on_match_failed` at the end.
///
/// Per-event calls are accept-only (a sink may buffer). Only terminals are
/// flush points. Replay sinks can be stricter internally without changing
/// the contract.
#[async_trait]
pub trait MatchSink<D: Descriptor>: Send + Sync {
    async fn on_match_started(
        &self,
        descriptor: &D,
        players: &[PlayerIdentity; 2],
    ) -> Result<(), SinkError>;

    async fn on_match_event(&self, id: MatchId, event: &MatchEvent) -> Result<(), SinkError>;

    async fn on_match_finished(&self, outcome: &MatchOutcome<D>) -> Result<(), SinkError>;

    async fn on_match_failed(&self, failure: &MatchFailure<D>) -> Result<(), SinkError>;

    /// Called when a match's lifecycle was cut short before terminal
    /// callbacks could fire (typically: a `Required` sink errored, so
    /// terminal callbacks were skipped on the rest). Best-effort cleanup
    /// only: errors are logged at `warn` and never mutate outcomes.
    /// Default no-op so stateless sinks ignore it.
    ///
    /// Stateful sinks (e.g. replay buffers keyed by `MatchId`) implement it
    /// to release per-match state. Conflating cleanup with terminal
    /// `on_match_failed` would force a synthesized `MatchFailure` that
    /// doesn't reflect what really happened to *this* sink (it never
    /// failed; another sink did). Separate hook = honest semantics.
    async fn on_match_abandoned(&self, _id: MatchId) -> Result<(), SinkError> {
        Ok(())
    }
}

/// Drops every callback. Useful for tests that don't need persistence and
/// for the default sink wiring before any consumer plugs in.
#[derive(Debug, Default)]
pub struct NoOpSink<D> {
    _phantom: std::marker::PhantomData<fn() -> D>,
}

impl<D> NoOpSink<D> {
    pub const fn new() -> Self {
        Self {
            _phantom: std::marker::PhantomData,
        }
    }
}

#[async_trait]
impl<D: Descriptor> MatchSink<D> for NoOpSink<D> {
    async fn on_match_started(
        &self,
        _descriptor: &D,
        _players: &[PlayerIdentity; 2],
    ) -> Result<(), SinkError> {
        Ok(())
    }

    async fn on_match_event(&self, _id: MatchId, _event: &MatchEvent) -> Result<(), SinkError> {
        Ok(())
    }

    async fn on_match_finished(&self, _outcome: &MatchOutcome<D>) -> Result<(), SinkError> {
        Ok(())
    }

    async fn on_match_failed(&self, _failure: &MatchFailure<D>) -> Result<(), SinkError> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::descriptor::AdHocDescriptor;

    /// Confirms the trait is object-safe. `Box<dyn MatchSink<AdHocDescriptor>>`
    /// must compile so the executor can hold heterogeneous sinks behind one type.
    #[test]
    fn match_sink_is_object_safe() {
        let _sink: Box<dyn MatchSink<AdHocDescriptor>> =
            Box::new(NoOpSink::<AdHocDescriptor>::new());
    }
}
