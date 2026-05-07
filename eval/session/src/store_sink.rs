//! `MatchSink` that writes `match_attempts` rows to `pyrat-eval-store`.
//!
//! Always classified `Required` by callers — a flush failure here means the
//! tournament's durable record is incomplete.
//!
//! `durable_record == false` failures are skipped: they signal "the row
//! couldn't be written" (sink flush failure) or "the match was lost without
//! a record" (kill-9). Either way, no row exists, so the planner re-issues
//! at the same `attempt_index` on resume. This is what keeps runtime state
//! and durable state byte-equivalent.

use std::sync::Arc;

use async_trait::async_trait;
use parking_lot::Mutex;
use pyrat_eval_store::{EvalStore, RecordAttemptError};
use pyrat_host::match_host::MatchEvent;
use pyrat_host::player::PlayerIdentity;
use pyrat_orchestrator::{MatchFailure, MatchId, MatchOutcome, MatchSink, SinkError};

use crate::descriptor::EvalMatchDescriptor;
use crate::mapping::{failure_to_new_attempt, outcome_to_new_attempt};

/// Required sink that persists match terminals as `match_attempts` rows.
///
/// SQLite ownership: a single `Arc<Mutex<EvalStore>>` is acquired inside
/// `tokio::task::spawn_blocking`, never held across an `.await` (rusqlite
/// is sync, so the mutex covers the inline blocking call). For a future
/// hot path, the alternative is a per-call connection pool — documented
/// here, not implemented in v1.
pub struct StoreSink {
    store: Arc<Mutex<EvalStore>>,
}

impl StoreSink {
    pub fn new(store: Arc<Mutex<EvalStore>>) -> Self {
        Self { store }
    }
}

#[async_trait]
impl MatchSink<EvalMatchDescriptor> for StoreSink {
    async fn on_match_started(
        &self,
        _descriptor: &EvalMatchDescriptor,
        _players: &[PlayerIdentity; 2],
    ) -> Result<(), SinkError> {
        // No row at start. v1 punt: an `attempts.status='in_flight'` row
        // would give forensics on lost matches, but it complicates resume
        // (must reconcile). The plan documents this as a follow-up.
        Ok(())
    }

    async fn on_match_event(&self, _id: MatchId, _event: &MatchEvent) -> Result<(), SinkError> {
        // Per-event accept is cheap. The store is terminal-only; replay
        // sinks (a separate Optional sink) handle per-event capture.
        Ok(())
    }

    async fn on_match_finished(
        &self,
        outcome: &MatchOutcome<EvalMatchDescriptor>,
    ) -> Result<(), SinkError> {
        let attempt = outcome_to_new_attempt(outcome);
        let store = self.store.clone();
        tokio::task::spawn_blocking(move || {
            store
                .lock()
                .record_attempt(&attempt)
                .map(|_id| ())
                .map_err(StoreSinkError::Record)
        })
        .await
        .map_err(StoreSinkError::Join)
        .and_then(|r| r)
        .map_err(|e| SinkError {
            source: anyhow::Error::new(e),
        })
    }

    async fn on_match_failed(
        &self,
        failure: &MatchFailure<EvalMatchDescriptor>,
    ) -> Result<(), SinkError> {
        // The orchestrator emits durable_record=false in two cases:
        //   1. The match was lost without a row (kill-9).
        //   2. A Required sink errored on terminal flush — the executor uses
        //      this to signal "the broken sink isn't called again".
        // In both, no row should exist, otherwise the planner would think
        // the slot is done and skip retry.
        if !failure.durable_record {
            tracing::error!(
                match_id = ?failure.descriptor.match_id,
                attempt_index = failure.descriptor.attempt_index,
                reason = ?failure.reason,
                "skipping store write for non-durable failure (planner will silently retry)",
            );
            return Ok(());
        }
        let attempt = failure_to_new_attempt(failure);
        let store = self.store.clone();
        tokio::task::spawn_blocking(move || {
            store
                .lock()
                .record_attempt(&attempt)
                .map(|_id| ())
                .map_err(StoreSinkError::Record)
        })
        .await
        .map_err(StoreSinkError::Join)
        .and_then(|r| r)
        .map_err(|e| SinkError {
            source: anyhow::Error::new(e),
        })
    }
}

/// Internal store-sink error. Wrapped in `SinkError` at the sink boundary so
/// the orchestrator's contract sees `anyhow::Error`.
#[derive(Debug, thiserror::Error)]
pub enum StoreSinkError {
    #[error("blocking task panicked: {0}")]
    Join(tokio::task::JoinError),

    #[error("record_attempt failed: {0}")]
    Record(#[from] RecordAttemptError),
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, SystemTime, UNIX_EPOCH};

    use pyrat_eval_store::{
        AttemptOutcome, EvalStore, GameConfigRecord, NewPlayer, NewTournament, TournamentId,
    };
    use pyrat_host::match_host::MatchResult;
    use pyrat_host::player::PlayerIdentity;
    use pyrat_host::wire::{GameResult, Player};
    use pyrat_orchestrator::{FailureReason, MatchId};

    use super::*;

    fn identity(slot: Player) -> PlayerIdentity {
        PlayerIdentity {
            name: "x".into(),
            author: "x".into(),
            agent_id: "x".into(),
            slot,
        }
    }

    /// Bootstrap an in-memory store with one tournament, two players, one
    /// game-config row. Returns the seeded ids.
    fn seeded_store() -> (Arc<Mutex<EvalStore>>, TournamentId, String) {
        let store = EvalStore::open_in_memory().unwrap();
        let game_config_id = store
            .ensure_game_config(&GameConfigRecord {
                width: 7,
                height: 5,
                max_turns: 300,
                wall_density: 0.7,
                mud_density: 0.1,
                mud_range: 3,
                connected: true,
                symmetric: true,
                cheese_count: 3,
                cheese_symmetric: true,
            })
            .unwrap();
        store
            .register_player(&NewPlayer {
                id: "a".into(),
                display_name: "A".into(),
                agent_id: Some("a".into()),
                version: None,
                command: None,
                metadata_json: None,
            })
            .unwrap();
        store
            .register_player(&NewPlayer {
                id: "b".into(),
                display_name: "B".into(),
                agent_id: Some("b".into()),
                version: None,
                command: None,
                metadata_json: None,
            })
            .unwrap();
        let tid = store
            .create_tournament(&NewTournament {
                format: "round_robin".into(),
                target_games_per_matchup: Some(1),
                params_json: "{}".into(),
            })
            .unwrap();
        store.add_tournament_player(tid, "a", 0).unwrap();
        store.add_tournament_player(tid, "b", 1).unwrap();
        (Arc::new(Mutex::new(store)), tid, game_config_id)
    }

    fn descriptor(
        tid: TournamentId,
        game_config_id: &str,
        match_id: u64,
        attempt_index: u32,
    ) -> EvalMatchDescriptor {
        EvalMatchDescriptor {
            match_id: MatchId(match_id),
            tournament_id: tid,
            game_config_id: game_config_id.into(),
            player1_id: "a".into(),
            player2_id: "b".into(),
            seed: 42,
            repetition_index: 0,
            attempt_index,
            planned_at: SystemTime::UNIX_EPOCH,
        }
    }

    #[tokio::test]
    async fn on_match_finished_writes_success_row() {
        let (store, tid, gc) = seeded_store();
        let sink = StoreSink::new(store.clone());
        let outcome = MatchOutcome {
            descriptor: descriptor(tid, &gc, 1, 0),
            started_at: UNIX_EPOCH + Duration::from_secs(100),
            finished_at: UNIX_EPOCH + Duration::from_secs(200),
            result: MatchResult {
                result: GameResult::Player1,
                player1_score: 5.0,
                player2_score: 3.0,
                turns_played: 50,
            },
            players: [identity(Player::Player1), identity(Player::Player2)],
        };
        sink.on_match_finished(&outcome).await.unwrap();
        // Row visible via the same store handle (sufficient because we
        // hold the only writer; PR 5's plan refers to "via a separate
        // SQLite connection" but the in-memory store is single-connection
        // by design — re-opening creates a new empty DB).
        let attempts = store.lock().get_attempts(tid, None).unwrap();
        assert_eq!(attempts.len(), 1);
        match &attempts[0].outcome {
            AttemptOutcome::Success {
                player1_score,
                player2_score,
                turns,
                ..
            } => {
                assert!((player1_score - 5.0).abs() < 1e-9);
                assert!((player2_score - 3.0).abs() < 1e-9);
                assert_eq!(*turns, 50);
            },
            AttemptOutcome::Failure { .. } => panic!("expected success"),
        }
    }

    #[tokio::test]
    async fn on_match_failed_durable_writes_failure_row() {
        let (store, tid, gc) = seeded_store();
        let sink = StoreSink::new(store.clone());
        let failure = MatchFailure {
            descriptor: descriptor(tid, &gc, 1, 0),
            started_at: None,
            failed_at: UNIX_EPOCH + Duration::from_secs(50),
            reason: FailureReason::SpawnFailed,
            players: None,
            durable_record: true,
        };
        sink.on_match_failed(&failure).await.unwrap();
        let attempts = store.lock().get_attempts(tid, None).unwrap();
        assert_eq!(attempts.len(), 1);
        match &attempts[0].outcome {
            AttemptOutcome::Failure {
                failure_reason,
                started_at,
            } => {
                assert_eq!(failure_reason, "spawn_failed");
                assert!(started_at.is_none());
            },
            AttemptOutcome::Success { .. } => panic!("expected failure"),
        }
    }

    /// `durable_record == false` is the load-bearing rule: skip the write
    /// entirely so resume / runtime state stays equivalent.
    #[tokio::test]
    async fn on_match_failed_non_durable_skips_write() {
        let (store, tid, gc) = seeded_store();
        let sink = StoreSink::new(store.clone());
        let failure = MatchFailure {
            descriptor: descriptor(tid, &gc, 1, 0),
            started_at: None,
            failed_at: UNIX_EPOCH + Duration::from_secs(50),
            reason: FailureReason::Cancelled,
            players: None,
            durable_record: false,
        };
        sink.on_match_failed(&failure).await.unwrap();
        let attempts = store.lock().get_attempts(tid, None).unwrap();
        assert!(attempts.is_empty(), "no row should be written");
    }

    /// Descriptor fields end up in the right `match_attempts` columns.
    /// Pins the projection contract.
    #[tokio::test]
    async fn descriptor_fields_round_trip_to_attempt_record() {
        let (store, tid, gc) = seeded_store();
        let sink = StoreSink::new(store.clone());
        let outcome = MatchOutcome {
            descriptor: EvalMatchDescriptor {
                match_id: MatchId(7),
                tournament_id: tid,
                game_config_id: gc.clone(),
                player1_id: "a".into(),
                player2_id: "b".into(),
                seed: 0x1234,
                repetition_index: 2,
                attempt_index: 3,
                planned_at: SystemTime::UNIX_EPOCH,
            },
            started_at: UNIX_EPOCH + Duration::from_secs(100),
            finished_at: UNIX_EPOCH + Duration::from_secs(200),
            result: MatchResult {
                result: GameResult::Draw,
                player1_score: 1.0,
                player2_score: 1.0,
                turns_played: 25,
            },
            players: [identity(Player::Player1), identity(Player::Player2)],
        };
        sink.on_match_finished(&outcome).await.unwrap();
        let attempts = store.lock().get_attempts(tid, None).unwrap();
        assert_eq!(attempts.len(), 1);
        let key = &attempts[0].key;
        assert_eq!(key.tournament_id, tid);
        assert_eq!(key.game_config_id, gc);
        assert_eq!(key.player1_id, "a");
        assert_eq!(key.player2_id, "b");
        assert_eq!(key.seed, 0x1234);
        assert_eq!(key.repetition_index, 2);
        assert_eq!(key.attempt_index, 3);
    }
}
