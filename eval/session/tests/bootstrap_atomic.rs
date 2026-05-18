//! `EvalSession::create_tournament` runs its four bootstrap steps inside a
//! single SQLite transaction. A mid-sequence failure must roll the
//! tournament row + any participants back so the store doesn't end up in a
//! partial state that would confuse resume.

mod common;

use std::sync::Arc;

use parking_lot::Mutex;
use pyrat_eval::{EvalSession, ResolvedPlayer, SessionError, TournamentSpec};
use pyrat_eval_store::{AddTournamentPlayerError, EvalStore, TournamentId};
use pyrat_orchestrator::PlayerSpec;

use crate::common::{mock_factory, small_game_config};

/// Two `ResolvedPlayer`s with the same id and identical identity fields.
/// `register_player` is idempotent for matching identities, so both
/// register cleanly. The tournament row inserts. The first
/// `add_tournament_player` succeeds at slot 0. The second collides on
/// `PRIMARY KEY (tournament_id, player_id)` and the store's typed
/// pre-check returns `PlayerAlreadyInTournament` — failure lands *after*
/// the tournament row is inserted, which is what makes rollback meaningful.
#[tokio::test]
async fn tournament_row_not_orphaned_on_mid_bootstrap_failure() {
    let store = Arc::new(Mutex::new(EvalStore::open_in_memory().unwrap()));
    let duplicate = ResolvedPlayer {
        id: "a".into(),
        spec: PlayerSpec::Embedded {
            agent_id: "a".into(),
            name: "a".into(),
            author: "tests".into(),
            factory: mock_factory(),
        },
    };
    // Same id, same identity — both register as no-op-success.
    let players = vec![duplicate.clone(), duplicate];

    let spec = TournamentSpec {
        format: "round_robin".into(),
        target_games_per_matchup: Some(1),
        params_json: "{}".into(),
        game_config: small_game_config(),
        tournament_seed: 0xC0FFEE,
    };

    let err = EvalSession::create_tournament(store.clone(), spec, players)
        .await
        .expect_err("duplicate player id should fail bootstrap");

    // The typed error must bubble through.
    match err {
        SessionError::AddTournamentPlayer(
            AddTournamentPlayerError::PlayerAlreadyInTournament { .. },
        ) => {},
        other => panic!("expected PlayerAlreadyInTournament, got {other:?}"),
    }

    // Post-state assertions — the call errored so there is no `tid` to
    // query directly. Use the list to prove no row survived.
    let s = store.lock();
    let tournaments = s.list_tournaments().expect("list_tournaments");
    assert!(
        tournaments.is_empty(),
        "tournament row leaked after rollback: {tournaments:?}"
    );
    // Defense-in-depth: a fresh DB would have allocated id 1. If the
    // rollback failed to remove the slot-0 participant, the row would
    // still be visible by that id.
    let participants = s
        .get_tournament_players(TournamentId(1))
        .expect("get_tournament_players");
    assert!(
        participants.is_empty(),
        "participant row leaked after rollback: {participants:?}"
    );
}
