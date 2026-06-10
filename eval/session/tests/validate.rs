//! `EvalSession::start` cross-checks the planner against the stored
//! tournament spec on resume. Any divergence must surface as
//! `SessionError::TournamentMismatch` before the run loop is launched.
//!
//! Checks the validator performs today:
//! - `format` (round_robin vs gauntlet)
//! - `tournament_id`
//! - `expected_game_config_id` string equality
//! - **runtime `GameConfig` content hash** vs stored `game_config_id`
//!   (catches drift where the planner reuses the stored id but the
//!   resolved runtime config has different geometry)
//! - `tournament_seed`
//! - `target_games_per_matchup` (when both sides surface a value)
//! - `expected_params()` vs decoded `params_json` (currently
//!   `max_failures_per_pair`)
//! - players in slot order

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
            let reason = reason.to_string();
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

/// Format mismatch: a `GauntletPlanner` whose `expected_players()` happens
/// to slot-match a stored `round_robin` tournament's participants would
/// pass the player + target + seed + config checks. The format guard is
/// what catches this — otherwise the gauntlet would run challenger-vs-each
/// pairings and silently skip opponent-vs-opponent matchups, fragmenting
/// the tournament's history.
#[tokio::test]
async fn rejects_gauntlet_planner_for_round_robin_tournament() {
    let store = Arc::new(Mutex::new(EvalStore::open_in_memory().unwrap()));
    let players = vec![
        embedded_player("a"),
        embedded_player("b"),
        embedded_player("c"),
    ];
    // Stored spec is round_robin (format = "round_robin"), players [a, b, c],
    // target_games_per_matchup = 1.
    let created =
        EvalSession::create_tournament(store.clone(), round_robin_spec(), players.clone())
            .await
            .expect("create_tournament");

    // Gauntlet planner with the same id, same game_config_id, same seed,
    // same total players in slot order ([challenger, opponents..]), same
    // target. The only divergence is format.
    let planner = pyrat_eval::GauntletPlanner::new(pyrat_eval::GauntletPlannerConfig {
        challenger: players[0].clone(),
        opponents: vec![players[1].clone(), players[2].clone()],
        game_config: small_game_config(),
        game_config_id: created.game_config_id,
        timing: crate::common::fast_timing(),
        tournament_id: created.tournament_id,
        target_each: 1,
        max_failures_per_pair: 3,
        tournament_seed: 0xC0FFEE,
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
        "format",
    )
    .await;
}

/// Stored runtime config has max_turns=5; planner is built with the
/// *stored* `game_config_id` (so the id-string check passes) but a
/// runtime `GameConfig` of different geometry (max_turns=99). The new
/// content-hash check should reject this — without it, attempts would
/// be recorded against a stored row that doesn't describe what was
/// played.
#[tokio::test]
async fn rejects_planner_with_drifted_runtime_game_config() {
    use pyrat::{Coordinates, GameBuilder};

    let store = Arc::new(Mutex::new(EvalStore::open_in_memory().unwrap()));
    let players = vec![embedded_player("a"), embedded_player("b")];
    let created =
        EvalSession::create_tournament(store.clone(), round_robin_spec(), players.clone())
            .await
            .expect("create_tournament");

    // Different runtime config (max_turns=99 vs the stored small's 5).
    // Same shape otherwise so `game_config_to_record` succeeds.
    let drifted_runtime = GameBuilder::new(3, 3)
        .with_max_turns(99)
        .with_open_maze()
        .with_custom_positions(Coordinates::new(0, 0), Coordinates::new(2, 2))
        .with_random_cheese(1, false)
        .build();

    // Build planner with the *stored* game_config_id (string equality
    // passes) but the drifted runtime config.
    let planner = pyrat_eval::RoundRobinPlanner::new(pyrat_eval::RoundRobinPlannerConfig {
        players,
        game_config: drifted_runtime,
        game_config_id: created.game_config_id,
        timing: crate::common::fast_timing(),
        tournament_id: created.tournament_id,
        target_per_pair: 1,
        max_failures_per_pair: 3,
        tournament_seed: 0xC0FFEE,
    });

    // The message must carry expected-vs-got geometry, not just two
    // opaque hashes — `max_turns=99` (resolved) and `max_turns=5`
    // (stored) is what lets a user trace the drift back to their flags.
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
        "hashes to",
    )
    .await;
}

/// Same drift setup as above, asserting the geometry rendering: the
/// mismatch message shows both the resolved and the stored max_turns so
/// the user sees *what* drifted, not just that hashes differ.
#[tokio::test]
async fn drifted_game_config_message_shows_geometry_expected_vs_got() {
    use pyrat::{Coordinates, GameBuilder};

    let store = Arc::new(Mutex::new(EvalStore::open_in_memory().unwrap()));
    let players = vec![embedded_player("a"), embedded_player("b")];
    let created =
        EvalSession::create_tournament(store.clone(), round_robin_spec(), players.clone())
            .await
            .expect("create_tournament");

    let drifted_runtime = GameBuilder::new(3, 3)
        .with_max_turns(99)
        .with_open_maze()
        .with_custom_positions(Coordinates::new(0, 0), Coordinates::new(2, 2))
        .with_random_cheese(1, false)
        .build();

    let planner = pyrat_eval::RoundRobinPlanner::new(pyrat_eval::RoundRobinPlannerConfig {
        players,
        game_config: drifted_runtime,
        game_config_id: created.game_config_id,
        timing: crate::common::fast_timing(),
        tournament_id: created.tournament_id,
        target_per_pair: 1,
        max_failures_per_pair: 3,
        tournament_seed: 0xC0FFEE,
    });

    let result = EvalSession::start(
        store.clone(),
        SessionMode {
            tournament_id: created.tournament_id,
        },
        planner,
        fast_orch_config(),
        EloOptions::new("a"),
        SessionConfig::default(),
    )
    .await;
    match result {
        Err(SessionError::TournamentMismatch(reason)) => {
            let reason = reason.to_string();
            assert!(
                reason.contains("max_turns=99"),
                "resolved geometry missing: {reason}"
            );
            assert!(
                reason.contains("max_turns=5"),
                "stored geometry missing: {reason}"
            );
        },
        Err(other) => panic!("expected TournamentMismatch, got {other:?}"),
        Ok(_session) => panic!("expected TournamentMismatch, got Ok"),
    }
}

/// Stored `params_json` decodes to `max_failures_per_pair: 3` (set by
/// `round_robin_spec`). A planner built with `max_failures_per_pair: 99`
/// should be rejected — resume with a different retry budget would
/// silently change tournament semantics.
#[tokio::test]
async fn rejects_planner_with_different_params() {
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
        max_failures_per_pair: 99, // diverges from spec's 3
        tournament_seed: 0xC0FFEE,
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
        "params:",
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
