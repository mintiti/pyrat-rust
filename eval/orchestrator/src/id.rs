//! Monotonic match identifier.
//!
//! `MatchId` is allocated at submit time and stays stable across broadcast,
//! sink calls, and replay records. The allocator is explicit (not a global
//! atomic) so each orchestrator instance — and each test — controls its own
//! id space.

use std::sync::atomic::{AtomicU64, Ordering};

use serde::{Deserialize, Serialize};

/// Stable identifier for a match within an orchestrator instance.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct MatchId(pub u64);

impl std::fmt::Display for MatchId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "match#{}", self.0)
    }
}

/// Allocator for monotonic [`MatchId`]s. One per orchestrator instance.
#[derive(Debug, Default)]
pub struct MatchIdAllocator {
    next: AtomicU64,
}

impl MatchIdAllocator {
    pub const fn new() -> Self {
        Self {
            next: AtomicU64::new(0),
        }
    }

    /// Allocate the next id. Monotonic, never reused within this allocator.
    pub fn allocate(&self) -> MatchId {
        MatchId(self.next.fetch_add(1, Ordering::Relaxed))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn allocator_yields_monotonic_ids() {
        let alloc = MatchIdAllocator::new();
        let ids: Vec<_> = (0..5).map(|_| alloc.allocate()).collect();
        assert_eq!(
            ids,
            vec![MatchId(0), MatchId(1), MatchId(2), MatchId(3), MatchId(4),]
        );
    }

    #[test]
    fn allocators_are_independent() {
        let a = MatchIdAllocator::new();
        let b = MatchIdAllocator::new();
        assert_eq!(a.allocate(), MatchId(0));
        assert_eq!(a.allocate(), MatchId(1));
        assert_eq!(b.allocate(), MatchId(0));
    }

    #[test]
    fn match_id_serialize_roundtrip() {
        let id = MatchId(42);
        let s = serde_json::to_string(&id).unwrap();
        let back: MatchId = serde_json::from_str(&s).unwrap();
        assert_eq!(id, back);
    }
}
