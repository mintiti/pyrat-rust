//! Tournament-level state: what the planner reads, what resume reconstructs.
//!
//! Two views into the same data:
//! - `history: HashMap<MatchupKey, MatchupHistory>` — per-matchup attempts
//!   (success rows carry scores so Elo can be recomputed without re-querying
//!   the store).
//! - `in_flight: HashSet<MatchId>` — runtime-only set of submitted matches
//!   whose terminal has not yet been observed.
//!
//! `apply(&DriverEvent)` is the canonical state mutator. The four-rule
//! contract pinned at `plan.md:468` keeps runtime state byte-equivalent to
//! the durable record: anything not in `match_attempts` is not in `history`
//! either (kill-9 mid-match leaves no row and no entry; the planner re-issues
//! at the same `attempt_index` on the next tick).

use std::collections::{HashMap, HashSet};

use pyrat_eval_store::{
    aggregate_pairs, compute_elo, AttemptOutcome, AttemptRecord, AttemptStatus, EloOptions,
    EloRating, EloResult, HeadToHead, TournamentId,
};
use pyrat_orchestrator::{DriverEvent, MatchId};

use crate::descriptor::EvalMatchDescriptor;

pub type PlayerId = String;
pub type GameConfigId = String;

/// Identity of one matchup-pair-with-config-and-repetition.
///
/// Seed is intentionally NOT in the key: it is functionally derived from
/// the other fields via the planner's stateless seed function. Including
/// seed here would make every retry a distinct matchup, breaking the
/// "retry plays the same seeded game" semantics.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct MatchupKey {
    pub player1_id: PlayerId,
    pub player2_id: PlayerId,
    pub game_config_id: GameConfigId,
    pub repetition_index: u32,
}

/// One attempt for a matchup. Carries enough to recompute Elo without
/// re-querying the store.
///
/// The plan originally specified `(u32, AttemptStatus)` here. Extended to
/// carry success scores so `head_to_head` can aggregate without fabricating
/// `AttemptRecord`s (the in-memory shape stays a faithful projection of the
/// durable row).
#[derive(Debug, Clone, PartialEq)]
pub struct MatchupAttempt {
    pub attempt_index: u32,
    pub outcome: MatchupOutcome,
}

#[derive(Debug, Clone, PartialEq)]
pub enum MatchupOutcome {
    Success {
        player1_score: f64,
        player2_score: f64,
    },
    Failure,
}

impl MatchupOutcome {
    pub fn status(&self) -> AttemptStatus {
        match self {
            Self::Success { .. } => AttemptStatus::Success,
            Self::Failure => AttemptStatus::Failure,
        }
    }
}

pub type MatchupHistory = Vec<MatchupAttempt>;

#[derive(Debug, Clone)]
pub struct TournamentState {
    pub tournament_id: TournamentId,
    pub history: HashMap<MatchupKey, MatchupHistory>,
    pub in_flight: HashSet<MatchId>,
    pub standings: Vec<EloRating>,
}

impl TournamentState {
    pub fn empty(tournament_id: TournamentId) -> Self {
        Self {
            tournament_id,
            history: HashMap::new(),
            in_flight: HashSet::new(),
            standings: Vec::new(),
        }
    }

    /// Fold one durable row into history. Used by resume.
    pub fn fold_attempt(&mut self, a: &AttemptRecord) {
        let key = MatchupKey {
            player1_id: a.key.player1_id.clone(),
            player2_id: a.key.player2_id.clone(),
            game_config_id: a.key.game_config_id.clone(),
            repetition_index: a.key.repetition_index,
        };
        let outcome = match &a.outcome {
            AttemptOutcome::Success {
                player1_score,
                player2_score,
                ..
            } => MatchupOutcome::Success {
                player1_score: *player1_score,
                player2_score: *player2_score,
            },
            AttemptOutcome::Failure { .. } => MatchupOutcome::Failure,
        };
        self.history.entry(key).or_default().push(MatchupAttempt {
            attempt_index: a.key.attempt_index,
            outcome,
        });
    }

    /// Apply one lifecycle event from the orchestrator. See `plan.md:468`
    /// for the four-rule contract this implements.
    pub fn apply(&mut self, event: &DriverEvent<EvalMatchDescriptor>) {
        // DriverEvent is `#[non_exhaustive]`. The wildcard absorbs future
        // lifecycle variants; current ones are exhaustive.
        match event {
            DriverEvent::MatchQueued { id, .. } => {
                self.in_flight.insert(*id);
            },
            DriverEvent::MatchStarted { .. } => {},
            DriverEvent::MatchFinished { outcome } => {
                self.in_flight.remove(&outcome.descriptor.match_id);
                let desc = &outcome.descriptor;
                self.history
                    .entry(matchup_key(desc))
                    .or_default()
                    .push(MatchupAttempt {
                        attempt_index: desc.attempt_index,
                        outcome: MatchupOutcome::Success {
                            player1_score: f64::from(outcome.result.player1_score),
                            player2_score: f64::from(outcome.result.player2_score),
                        },
                    });
            },
            DriverEvent::MatchFailed { failure } => {
                self.in_flight.remove(&failure.descriptor.match_id);
                if failure.durable_record {
                    let desc = &failure.descriptor;
                    self.history
                        .entry(matchup_key(desc))
                        .or_default()
                        .push(MatchupAttempt {
                            attempt_index: desc.attempt_index,
                            outcome: MatchupOutcome::Failure,
                        });
                } else {
                    tracing::error!(
                        match_id = ?failure.descriptor.match_id,
                        attempt_index = failure.descriptor.attempt_index,
                        reason = ?failure.reason,
                        "lost match without durable row; planner will silently retry at same attempt_index",
                    );
                }
            },
            _ => {},
        }
    }

    /// Aggregate the success rows in history into head-to-head records
    /// (Elo input). Failure rows are skipped — Elo is computed from
    /// successful matches only, mirroring `head_to_head_from_attempt_records`.
    pub fn head_to_head(&self) -> Vec<HeadToHead> {
        let pairs = self.history.iter().flat_map(|(key, attempts)| {
            attempts.iter().filter_map(move |a| match &a.outcome {
                MatchupOutcome::Success {
                    player1_score,
                    player2_score,
                } => Some((
                    &key.player1_id,
                    &key.player2_id,
                    *player1_score,
                    *player2_score,
                )),
                MatchupOutcome::Failure => None,
            })
        });
        aggregate_pairs(pairs)
    }

    /// Recompute Elo from current history. On error (no records, disconnected
    /// graph) standings are cleared. The session calls this after each
    /// `MatchFinished` apply.
    pub fn recompute_elo(&mut self, options: &EloOptions) -> Option<EloResult> {
        let h2h = self.head_to_head();
        match compute_elo(&h2h, options) {
            Ok(result) => {
                self.standings = result.ratings.clone();
                Some(result)
            },
            Err(err) => {
                tracing::debug!(
                    tournament_id = ?self.tournament_id,
                    error = %err,
                    "elo recompute skipped",
                );
                self.standings.clear();
                None
            },
        }
    }
}

fn matchup_key(desc: &EvalMatchDescriptor) -> MatchupKey {
    MatchupKey {
        player1_id: desc.player1_id.clone(),
        player2_id: desc.player2_id.clone(),
        game_config_id: desc.game_config_id.clone(),
        repetition_index: desc.repetition_index,
    }
}

#[cfg(test)]
mod tests {
    use std::time::SystemTime;

    use pyrat_eval_store::{AttemptKey, AttemptOutcome};
    use pyrat_host::match_host::MatchResult;
    use pyrat_host::player::PlayerIdentity;
    use pyrat_host::wire::{GameResult, Player};
    use pyrat_orchestrator::{FailureReason, MatchFailure, MatchOutcome};

    use super::*;

    fn desc(attempt_index: u32, p1: &str, p2: &str) -> EvalMatchDescriptor {
        EvalMatchDescriptor {
            match_id: MatchId(u64::from(attempt_index)),
            tournament_id: TournamentId(1),
            game_config_id: "gc".into(),
            player1_id: p1.into(),
            player2_id: p2.into(),
            seed: 0,
            repetition_index: 0,
            attempt_index,
            planned_at: SystemTime::UNIX_EPOCH,
        }
    }

    fn identity(slot: Player) -> PlayerIdentity {
        PlayerIdentity {
            name: "x".into(),
            author: "x".into(),
            agent_id: "x".into(),
            slot,
        }
    }

    fn finished(
        d: EvalMatchDescriptor,
        p1_score: f32,
        p2_score: f32,
    ) -> DriverEvent<EvalMatchDescriptor> {
        DriverEvent::MatchFinished {
            outcome: MatchOutcome {
                descriptor: d,
                started_at: SystemTime::UNIX_EPOCH,
                finished_at: SystemTime::UNIX_EPOCH,
                result: MatchResult {
                    result: if p1_score > p2_score {
                        GameResult::Player1
                    } else if p2_score > p1_score {
                        GameResult::Player2
                    } else {
                        GameResult::Draw
                    },
                    player1_score: p1_score,
                    player2_score: p2_score,
                    turns_played: 50,
                },
                players: [identity(Player::Player1), identity(Player::Player2)],
            },
        }
    }

    fn failed(d: EvalMatchDescriptor, durable: bool) -> DriverEvent<EvalMatchDescriptor> {
        DriverEvent::MatchFailed {
            failure: MatchFailure {
                descriptor: d,
                started_at: None,
                failed_at: SystemTime::UNIX_EPOCH,
                reason: FailureReason::SpawnFailed,
                players: None,
                durable_record: durable,
            },
        }
    }

    #[test]
    fn match_queued_inserts_in_flight() {
        let mut s = TournamentState::empty(TournamentId(1));
        s.apply(&DriverEvent::MatchQueued {
            id: MatchId(7),
            descriptor: desc(0, "a", "b"),
        });
        assert!(s.in_flight.contains(&MatchId(7)));
    }

    #[test]
    fn match_finished_removes_in_flight_and_appends_success() {
        let mut s = TournamentState::empty(TournamentId(1));
        let d = desc(0, "a", "b");
        s.apply(&DriverEvent::MatchQueued {
            id: d.match_id,
            descriptor: d.clone(),
        });
        s.apply(&finished(d.clone(), 5.0, 3.0));
        assert!(!s.in_flight.contains(&d.match_id));
        let key = matchup_key(&d);
        let entries = s.history.get(&key).unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].attempt_index, 0);
        assert!(matches!(
            entries[0].outcome,
            MatchupOutcome::Success {
                player1_score: 5.0,
                player2_score: 3.0
            }
        ));
    }

    #[test]
    fn match_failed_durable_appends_failure_entry() {
        let mut s = TournamentState::empty(TournamentId(1));
        let d = desc(2, "a", "b");
        s.apply(&DriverEvent::MatchQueued {
            id: d.match_id,
            descriptor: d.clone(),
        });
        s.apply(&failed(d.clone(), true));
        assert!(!s.in_flight.contains(&d.match_id));
        let entries = s.history.get(&matchup_key(&d)).unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].attempt_index, 2);
        assert_eq!(entries[0].outcome.status(), AttemptStatus::Failure);
    }

    /// `durable_record == false` is the kill-9 / sink-flush-error case.
    /// State must remove from `in_flight` but leave `history` untouched —
    /// otherwise resume diverges from the durable record (no row exists in
    /// the store; an entry here would make the planner skip the slot).
    #[test]
    fn match_failed_non_durable_does_not_touch_history() {
        let mut s = TournamentState::empty(TournamentId(1));
        let d = desc(2, "a", "b");
        s.apply(&DriverEvent::MatchQueued {
            id: d.match_id,
            descriptor: d.clone(),
        });
        s.apply(&failed(d.clone(), false));
        assert!(!s.in_flight.contains(&d.match_id));
        assert!(!s.history.contains_key(&matchup_key(&d)));
    }

    #[test]
    fn fold_attempt_reconstructs_history_from_records() {
        let mut s = TournamentState::empty(TournamentId(1));
        let key = AttemptKey {
            tournament_id: TournamentId(1),
            game_config_id: "gc".into(),
            player1_id: "a".into(),
            player2_id: "b".into(),
            seed: 0,
            repetition_index: 0,
            attempt_index: 0,
        };
        let rec = AttemptRecord {
            id: 1,
            key: key.clone(),
            finished_at: "2026-05-07 00:00:00".into(),
            outcome: AttemptOutcome::Success {
                player1_score: 4.0,
                player2_score: 2.0,
                turns: 50,
                started_at: "2026-05-07 00:00:00".into(),
            },
        };
        s.fold_attempt(&rec);
        let entries = s
            .history
            .get(&MatchupKey {
                player1_id: "a".into(),
                player2_id: "b".into(),
                game_config_id: "gc".into(),
                repetition_index: 0,
            })
            .unwrap();
        assert_eq!(entries.len(), 1);
    }

    #[test]
    fn head_to_head_skips_failures() {
        let mut s = TournamentState::empty(TournamentId(1));
        s.apply(&finished(desc(0, "a", "b"), 5.0, 3.0));
        s.apply(&finished(desc(1, "a", "b"), 1.0, 4.0));
        s.apply(&failed(desc(2, "a", "b"), true));
        let h = s.head_to_head();
        assert_eq!(h.len(), 1);
        assert_eq!(h[0].player_a, "a");
        assert_eq!(h[0].player_b, "b");
        assert_eq!(h[0].wins_a, 1);
        assert_eq!(h[0].wins_b, 1);
        assert_eq!(h[0].draws, 0);
    }

    #[test]
    fn head_to_head_normalizes_pair_order() {
        // Storing under (b, a) should still aggregate into the canonical
        // (a, b) pair (sorted) so the same pair from different MatchupKey
        // orientations doesn't fragment.
        let mut s = TournamentState::empty(TournamentId(1));
        s.apply(&finished(desc(0, "a", "b"), 5.0, 3.0));
        s.apply(&finished(desc(1, "b", "a"), 5.0, 3.0));
        let h = s.head_to_head();
        assert_eq!(h.len(), 1);
        // Both games go to the canonical (a, b) pair.
        // (a, b, 5, 3) → wins_a += 1.
        // (b, a, 5, 3) → swapped to (a, b, 3, 5) → wins_b += 1.
        assert_eq!(h[0].wins_a, 1);
        assert_eq!(h[0].wins_b, 1);
    }
}
