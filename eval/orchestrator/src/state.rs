//! Operational, domain-free executor state.
//!
//! Snapshot type the orchestrator publishes. Domain-free on purpose: the
//! map value is a bare `SystemTime` so any consumer (live UI, dashboard,
//! test) can observe progress without needing the eval's domain types.

use std::collections::HashMap;
use std::time::SystemTime;

use crate::id::MatchId;

/// Snapshot of the orchestrator's operational state.
///
/// `finished` and `failed` are running totals over the orchestrator's
/// lifetime; `queued` and `running` reflect the live view. `running` is
/// keyed by id with the match's start time as value — snapshot consumers
/// can render an in-flight list without joining against the broadcast.
#[derive(Debug, Clone, Default)]
pub struct ExecutorState {
    pub queued: u64,
    pub running: HashMap<MatchId, SystemTime>,
    pub finished: u64,
    pub failed: u64,
}
