//! `EvalSession::shutdown` must return within a bounded time even when the
//! run loop is suspended on `driver_rx.recv()`. Regression test for the
//! prior shape where `shutdown` deadlocked because `orch.abort()` only
//! cancelled the orchestrator's root token and never dropped `driver_tx`.

mod common;

use std::sync::Arc;
use std::time::Duration;

use parking_lot::Mutex;
use pyrat_eval::{EvalSession, SessionConfig, SessionMode, TournamentParams, TournamentSpec};
use pyrat_eval_store::{EloOptions, EvalStore};

use crate::common::{embedded_player, fast_orch_config, round_robin, small_game_config};

#[tokio::test]
async fn shutdown_returns_promptly_with_pending_matchups() {
    let store = Arc::new(Mutex::new(EvalStore::open_in_memory().unwrap()));
    // Many players → many matchups still pending after we ask to shut down.
    let players = vec![
        embedded_player("a"),
        embedded_player("b"),
        embedded_player("c"),
        embedded_player("d"),
        embedded_player("e"),
    ];

    // Spec target must match the planner's target_per_pair below or
    // `start` rejects the planner via TournamentMismatch.
    const TARGET: u32 = 50;
    let spec = TournamentSpec {
        format: "round_robin".into(),
        target_games_per_matchup: Some(TARGET),
        // Matches the helper's `round_robin` planner
        // (`max_failures_per_pair: 3`) so resume validation passes.
        params_json: TournamentParams {
            max_failures_per_pair: 3,
        }
        .to_json(),
        game_config: small_game_config(),
        tournament_seed: 0xC0FFEE,
    };
    let created = EvalSession::create_tournament(store.clone(), spec, players.clone())
        .await
        .expect("create_tournament");

    let planner = round_robin(
        players,
        small_game_config(),
        created.game_config_id,
        created.tournament_id,
        // Many games per pair so the planner has plenty of pending work
        // when we call shutdown. The point is to prove `shutdown` doesn't
        // wait for natural completion.
        TARGET,
    );
    let session = EvalSession::start(
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
    .expect("session start");

    // Before the fix this would deadlock: the run loop blocks on
    // `driver_rx.recv().await`, `orch.abort()` only cancels the root token,
    // `driver_tx` stays alive via `self.orch: Arc<Orchestrator>`, and
    // `is_done && idle` never fires because there's pending planner work.
    tokio::time::timeout(Duration::from_secs(5), session.shutdown())
        .await
        .expect("shutdown should not hang")
        .expect("shutdown result");
}
