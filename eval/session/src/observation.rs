//! Lifecycle observation handed to the planner.
//!
//! `DriverEvent` carries orchestrator-shaped fields (`PlayerIdentity`, full
//! `MatchOutcome` and `MatchFailure`). The planner only needs descriptor +
//! durability, so we project to a slimmer type at the session boundary.

use pyrat_orchestrator::{DriverEvent, FailureReason};

use crate::descriptor::EvalMatchDescriptor;

#[derive(Debug, Clone)]
pub enum Observation {
    Queued {
        descriptor: EvalMatchDescriptor,
    },
    Started {
        descriptor: EvalMatchDescriptor,
    },
    Finished {
        descriptor: EvalMatchDescriptor,
    },
    Failed {
        descriptor: EvalMatchDescriptor,
        durable_record: bool,
        reason: FailureReason,
    },
}

impl Observation {
    pub fn descriptor(&self) -> &EvalMatchDescriptor {
        match self {
            Self::Queued { descriptor }
            | Self::Started { descriptor }
            | Self::Finished { descriptor }
            | Self::Failed { descriptor, .. } => descriptor,
        }
    }
}

impl Observation {
    /// Project a `DriverEvent` to the planner-facing observation. Returns
    /// `None` for variants this crate doesn't yet recognise (forward-compat
    /// hatch — `DriverEvent` is `#[non_exhaustive]`). Callers should log and
    /// skip; silently treating unknown variants as state-mutating events
    /// would risk planner/state drift.
    pub fn from_driver_event(event: &DriverEvent<EvalMatchDescriptor>) -> Option<Self> {
        match event {
            DriverEvent::MatchQueued { descriptor, .. } => Some(Observation::Queued {
                descriptor: descriptor.clone(),
            }),
            DriverEvent::MatchStarted { descriptor, .. } => Some(Observation::Started {
                descriptor: descriptor.clone(),
            }),
            DriverEvent::MatchFinished { outcome } => Some(Observation::Finished {
                descriptor: outcome.descriptor.clone(),
            }),
            DriverEvent::MatchFailed { failure } => Some(Observation::Failed {
                descriptor: failure.descriptor.clone(),
                durable_record: failure.durable_record,
                reason: failure.reason.clone(),
            }),
            _ => None,
        }
    }
}
