//! Per-match identity for tournament-context evaluation.
//!
//! `EvalMatchDescriptor` is the eval-layer descriptor: it carries the
//! tournament id, the planner's matchup identity, and the attempt index.
//! Sinks correlate durable rows by these fields. The orchestrator only
//! reads `match_id` and `seed` (via the `Descriptor` trait).

use std::time::SystemTime;

use pyrat_eval_store::TournamentId;
use pyrat_orchestrator::{Descriptor, MatchId};
use serde::{Deserialize, Serialize};

/// Identity for one tournament-context match.
///
/// Every field is durable: a row in `match_attempts` is keyed by
/// `(tournament_id, game_config_id, player1_id, player2_id, repetition_index,
/// attempt_index)`, with `seed` recorded for forensics. Resume reconstructs
/// state by reading these rows back.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct EvalMatchDescriptor {
    pub match_id: MatchId,
    pub tournament_id: TournamentId,
    pub game_config_id: String,
    pub player1_id: String,
    pub player2_id: String,
    pub seed: u64,
    pub repetition_index: u32,
    pub attempt_index: u32,
    pub planned_at: SystemTime,
}

impl Descriptor for EvalMatchDescriptor {
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

    fn sample() -> EvalMatchDescriptor {
        EvalMatchDescriptor {
            match_id: MatchId(42),
            tournament_id: TournamentId(7),
            game_config_id: "abc123".into(),
            player1_id: "pyrat/greedy".into(),
            player2_id: "pyrat/search".into(),
            seed: 0x0123_4567_89AB_CDEF,
            repetition_index: 1,
            attempt_index: 2,
            planned_at: SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(1_700_000_000),
        }
    }

    #[test]
    fn roundtrips_through_serde_json() {
        let desc = sample();
        let s = serde_json::to_string(&desc).unwrap();
        let back: EvalMatchDescriptor = serde_json::from_str(&s).unwrap();
        assert_eq!(desc, back);
    }

    #[test]
    fn descriptor_trait_returns_stored_match_id_and_seed() {
        let desc = sample();
        assert_eq!(desc.match_id(), MatchId(42));
        assert_eq!(desc.seed(), 0x0123_4567_89AB_CDEF);
    }
}
