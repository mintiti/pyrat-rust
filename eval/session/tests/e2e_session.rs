//! End-to-end: real `EvalSession` with embedded MockBots, real
//! `pyrat-orchestrator`, real `pyrat-eval-store`. Two MockBots play a
//! round-robin to completion. Verifies durable rows match the planned set.

mod common;

use std::sync::Arc;

use parking_lot::Mutex;
use pyrat_eval::{EvalSession, SessionMode};
use pyrat_eval_store::{AttemptOutcome, EloOptions, EvalStore};

use crate::common::{
    embedded_player, fast_orch_config, open_store_with_config, round_robin, round_robin_spec,
    small_game_config,
};

#[tokio::test]
async fn round_robin_two_mockbots_finishes_with_durable_rows() {
    let store = Arc::new(Mutex::new(EvalStore::open_in_memory().unwrap()));
    let game_config_id = open_store_with_config(&store);

    let players = vec![embedded_player("a"), embedded_player("b")];

    let tid = EvalSession::create_tournament(store.clone(), round_robin_spec(), players.clone())
        .await
        .expect("create_tournament");

    let planner = round_robin(players, small_game_config(), game_config_id, tid, 1);
    let session = EvalSession::start(
        store.clone(),
        SessionMode { tournament_id: tid },
        planner,
        fast_orch_config(),
        EloOptions::new("a"),
    )
    .await
    .expect("session start");

    session.join().await;

    let attempts = store.lock().get_attempts(tid, None).unwrap();
    // 2 players × 1 game per pair = 1 matchup.
    assert_eq!(
        attempts.len(),
        1,
        "expected 1 attempt row, got {attempts:?}"
    );
    assert!(matches!(
        attempts[0].outcome,
        AttemptOutcome::Success { .. }
    ));
}

#[tokio::test]
async fn three_player_round_robin_records_three_attempts() {
    let store = Arc::new(Mutex::new(EvalStore::open_in_memory().unwrap()));
    let game_config_id = open_store_with_config(&store);
    let players = vec![
        embedded_player("a"),
        embedded_player("b"),
        embedded_player("c"),
    ];

    let tid = EvalSession::create_tournament(store.clone(), round_robin_spec(), players.clone())
        .await
        .unwrap();

    let planner = round_robin(players, small_game_config(), game_config_id, tid, 1);
    let session = EvalSession::start(
        store.clone(),
        SessionMode { tournament_id: tid },
        planner,
        fast_orch_config(),
        EloOptions::new("a"),
    )
    .await
    .unwrap();
    session.join().await;

    let attempts = store.lock().get_attempts(tid, None).unwrap();
    assert_eq!(
        attempts.len(),
        3,
        "expected 3 attempts (3 unordered pairs × 1 game)"
    );
    for a in &attempts {
        assert!(matches!(a.outcome, AttemptOutcome::Success { .. }));
    }
}
