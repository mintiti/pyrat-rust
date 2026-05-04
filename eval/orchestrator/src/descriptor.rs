//! Per-match identity that flows through the orchestrator and out to sinks.
//!
//! The orchestrator is generic over `D: Descriptor` so consumers can carry
//! domain-specific identity (tournament id, player ids, attempt index) on
//! every event without the orchestrator inspecting any of it. The trait
//! demands only what executor bookkeeping needs (`match_id`) plus what
//! forensic sinks need (`seed` for replay headers).

use std::time::SystemTime;

use serde::{Deserialize, Serialize};

use crate::id::MatchId;

/// Identity carried alongside a match through queue, execution, and sinks.
///
/// Implementors:
/// - [`AdHocDescriptor`]: minimal, used by run-one paths and tests.
/// - `EvalMatchDescriptor` (in `pyrat-eval`): carries tournament/store ids.
pub trait Descriptor: Send + Sync + Clone + 'static {
    fn match_id(&self) -> MatchId;
    fn seed(&self) -> u64;
}

/// Minimal descriptor for runs that don't carry tournament context.
///
/// Used by the CLI's `run-one` subcommand, tests, and any consumer that
/// just wants to execute matches without persisting them in a pool.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AdHocDescriptor {
    pub match_id: MatchId,
    pub seed: u64,
    pub planned_at: SystemTime,
}

impl Descriptor for AdHocDescriptor {
    fn match_id(&self) -> MatchId {
        self.match_id
    }

    fn seed(&self) -> u64 {
        self.seed
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ad_hoc_descriptor_roundtrips() {
        let desc = AdHocDescriptor {
            match_id: MatchId(7),
            seed: 0xDEAD_BEEF,
            planned_at: SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(1_700_000_000),
        };
        let s = serde_json::to_string(&desc).unwrap();
        let back: AdHocDescriptor = serde_json::from_str(&s).unwrap();
        assert_eq!(desc, back);
    }

    #[test]
    fn ad_hoc_descriptor_returns_stored_match_id_and_seed() {
        let desc = AdHocDescriptor {
            match_id: MatchId(3),
            seed: 99,
            planned_at: SystemTime::UNIX_EPOCH,
        };
        assert_eq!(desc.match_id(), MatchId(3));
        assert_eq!(desc.seed(), 99);
    }
}
