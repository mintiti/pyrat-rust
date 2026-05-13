//! `EvalSession::start` cross-checks the planner against the stored
//! tournament spec on resume. Any divergence (players, game_config_id,
//! tournament_seed, target_games_per_matchup) must surface as
//! `SessionError::TournamentMismatch` before the run loop is launched.

mod common;

use std::sync::Arc;

use parking_lot::Mutex;
use pyrat_eval::{EvalSession, SessionConfig, SessionError, SessionMode};
use pyrat_eval_store::{EloOptions, EvalStore};

use crate::common::{
    embedded_player, fast_orch_config, round_robin, round_robin_spec, small_game_config,
};

/// Assert that the result is `Err(TournamentMismatch(reason))` and the
/// reason string contains `expected_substr`. `EvalSession` doesn't impl
/// `Debug`, so `expect_err` is unavailable; this helper unwraps via
/// pattern matching instead.
async fn expect_mismatch_containing<F, Fut>(builder: F, expected_substr: &str)
where
    F: FnOnce() -> Fut,
    Fut: std::future::Future<Output = Result<EvalSession, SessionError>>,
{
    match builder().await {
        Err(SessionError::TournamentMismatch(reason)) => {
            assert!(
                reason.contains(expected_substr),
                "expected '{expected_substr}' in mismatch reason, got: {reason}"
            );
        },
        Err(other) => panic!("expected TournamentMismatch, got {other:?}"),
        Ok(_session) => panic!("expected TournamentMismatch, got Ok"),
    }
}

/// Stored spec: players `[a, b]` at seed 0xC0FFEE. Planner with players
/// `[a, c]` should be rejected.
#[tokio::test]
async fn rejects_planner_with_different_players() {
    let store = Arc::new(Mutex::new(EvalStore::open_in_memory().unwrap()));
    let stored_players = vec![embedded_player("a"), embedded_player("b")];

    let created = EvalSession::create_tournament(store.clone(), round_robin_spec(), stored_players)
        .await
        .expect("create_tournament");
    // Player "c" never registered for this tournament — would silently
    // append rows for an off-roster player without this guard.
    store
        .lock()
        .register_player(&pyrat_eval_store::NewPlayer {
            id: "c".into(),
            display_name: "c".into(),
            agent_id: Some("c".into()),
            version: None,
            command: None,
            metadata_json: None,
        })
        .unwrap();

    let drifted_players = vec![embedded_player("a"), embedded_player("c")];
    let planner = round_robin(
        drifted_players,
        small_game_config(),
        created.game_config_id,
        created.tournament_id,
        1,
    );
    expect_mismatch_containing(
        || {
            EvalSession::start(
                store.clone(),
                SessionMode {
                    tournament_id: created.tournament_id,
                },
                planner,
                fast_orch_config(),
                EloOptions::new("a"),
                SessionConfig::default(),
            )
        },
        "players",
    )
    .await;
}

/// Stored seed is 0xC0FFEE; planner with seed 0xDEAD is rejected.
#[tokio::test]
async fn rejects_planner_with_different_tournament_seed() {
    let store = Arc::new(Mutex::new(EvalStore::open_in_memory().unwrap()));
    let players = vec![embedded_player("a"), embedded_player("b")];
    let created =
        EvalSession::create_tournament(store.clone(), round_robin_spec(), players.clone())
            .await
            .expect("create_tournament");

    let planner = pyrat_eval::RoundRobinPlanner::new(pyrat_eval::RoundRobinPlannerConfig {
        players,
        game_config: small_game_config(),
        game_config_id: created.game_config_id,
        timing: crate::common::fast_timing(),
        tournament_id: created.tournament_id,
        target_per_pair: 1,
        max_failures_per_pair: 3,
        tournament_seed: 0xDEAD, // diverges from spec's 0xC0FFEE
    });

    expect_mismatch_containing(
        || {
            EvalSession::start(
                store.clone(),
                SessionMode {
                    tournament_id: created.tournament_id,
                },
                planner,
                fast_orch_config(),
                EloOptions::new("a"),
                SessionConfig::default(),
            )
        },
        "tournament_seed",
    )
    .await;
}

/// Stored game_config_id is content-hashed from `small_game_config`;
/// planner with a different game_config_id is rejected.
#[tokio::test]
async fn rejects_planner_with_different_game_config_id() {
    let store = Arc::new(Mutex::new(EvalStore::open_in_memory().unwrap()));
    let players = vec![embedded_player("a"), embedded_player("b")];
    let created =
        EvalSession::create_tournament(store.clone(), round_robin_spec(), players.clone())
            .await
            .expect("create_tournament");

    let planner = round_robin(
        players,
        small_game_config(),
        "wrong-config-id".to_string(),
        created.tournament_id,
        1,
    );

    expect_mismatch_containing(
        || {
            EvalSession::start(
                store.clone(),
                SessionMode {
                    tournament_id: created.tournament_id,
                },
                planner,
                fast_orch_config(),
                EloOptions::new("a"),
                SessionConfig::default(),
            )
        },
        "game_config_id",
    )
    .await;
}

/// Stored target_games_per_matchup is 1; planner with target 5 is rejected.
#[tokio::test]
async fn rejects_planner_with_different_target_per_pair() {
    let store = Arc::new(Mutex::new(EvalStore::open_in_memory().unwrap()));
    let players = vec![embedded_player("a"), embedded_player("b")];
    let created =
        EvalSession::create_tournament(store.clone(), round_robin_spec(), players.clone())
            .await
            .expect("create_tournament");

    let planner = round_robin(
        players,
        small_game_config(),
        created.game_config_id,
        created.tournament_id,
        5, // diverges from spec's Some(1)
    );

    expect_mismatch_containing(
        || {
            EvalSession::start(
                store.clone(),
                SessionMode {
                    tournament_id: created.tournament_id,
                },
                planner,
                fast_orch_config(),
                EloOptions::new("a"),
                SessionConfig::default(),
            )
        },
        "target_games_per_matchup",
    )
    .await;
}

/// Happy path: a planner that matches the stored spec resumes cleanly.
#[tokio::test]
async fn accepts_matching_planner() {
    let store = Arc::new(Mutex::new(EvalStore::open_in_memory().unwrap()));
    let players = vec![embedded_player("a"), embedded_player("b")];
    let created =
        EvalSession::create_tournament(store.clone(), round_robin_spec(), players.clone())
            .await
            .expect("create_tournament");

    let planner = round_robin(
        players,
        small_game_config(),
        created.game_config_id,
        created.tournament_id,
        1,
    );

    let session = match EvalSession::start(
        store.clone(),
        SessionMode {
            tournament_id: created.tournament_id,
        },
        planner,
        fast_orch_config(),
        EloOptions::new("a"),
        SessionConfig::default(),
    )
    .await
    {
        Ok(s) => s,
        Err(e) => panic!("matching planner should be accepted: {e:?}"),
    };

    session.shutdown().await.expect("shutdown");
}
