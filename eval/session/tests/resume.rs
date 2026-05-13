//! Resume reconstruction: after planting partial attempt rows in the
//! store, the planner should re-issue exactly the missing matchups.

mod common;

use std::sync::Arc;

use parking_lot::Mutex;
use pyrat_eval::{
    mapping::synthetic_attempt, matchup_seed, MatchupKey, MatchupOutcome, Planner, TournamentState,
};
use pyrat_eval_store::{EvalStore, NewAttemptOutcome, NewPlayer, NewTournament};
use pyrat_orchestrator::MatchIdAllocator;

use crate::common::{embedded_player, open_store_with_config, round_robin, small_game_config};

/// Helper to plant a success row directly into the store, then verify the
/// planner reconstructs `TournamentState.history` from it on resume.
#[tokio::test]
async fn resume_skips_completed_matchups() {
    let store = Arc::new(Mutex::new(EvalStore::open_in_memory().unwrap()));
    let game_config_id = open_store_with_config(&store);

    let p1 = embedded_player("a");
    let p2 = embedded_player("b");
    let p3 = embedded_player("c");

    // Register players + tournament + participants.
    let tid = {
        let s = store.lock();
        for p in [&p1, &p2, &p3] {
            s.register_player(&NewPlayer {
                id: p.id.clone(),
                display_name: p.id.clone(),
                agent_id: Some(p.id.clone()),
                version: None,
                command: None,
                metadata_json: None,
            })
            .unwrap();
        }
        let tid = s
            .create_tournament(&NewTournament {
                format: "round_robin".into(),
                target_games_per_matchup: Some(1),
                params_json: "{}".into(),
                game_config_id: game_config_id.clone(),
                tournament_seed: 0xC0FFEE,
            })
            .unwrap();
        for (slot, p) in [&p1, &p2, &p3].iter().enumerate() {
            s.add_tournament_player(tid, &p.id, slot as i64).unwrap();
        }
        tid
    };

    // Plant one success row for (a, b): that pair is "done".
    let seed_ab = matchup_seed(0xC0FFEE, "a", "b", &game_config_id, 0);
    let attempt = synthetic_attempt(
        tid,
        &game_config_id,
        "a",
        "b",
        seed_ab,
        0,
        0,
        "1970-01-01 00:01:00",
        NewAttemptOutcome::Success {
            player1_score: 1.0,
            player2_score: 0.0,
            turns: 5,
            started_at: "1970-01-01 00:00:00".into(),
        },
    );
    store.lock().record_attempt(&attempt).unwrap();

    // Reconstruct state and ask the planner what to issue.
    let attempts = store.lock().get_attempts(tid, None).unwrap();
    let mut state = TournamentState::empty(tid);
    for a in &attempts {
        state.fold_attempt(a);
    }

    // Sanity: history shows the (a, b) matchup as Success.
    let key_ab = MatchupKey {
        player1_id: "a".into(),
        player2_id: "b".into(),
        game_config_id: game_config_id.clone(),
        repetition_index: 0,
    };
    assert!(matches!(
        state.history.get(&key_ab).unwrap()[0].outcome,
        MatchupOutcome::Success { .. }
    ));

    // Planner with all three players, target=1: should issue (a,c) and (b,c) only.
    let mut planner = round_robin(
        vec![p1, p2, p3],
        small_game_config(),
        game_config_id.clone(),
        tid,
        1,
    );
    let alloc = MatchIdAllocator::new();
    let batch = planner.next_batch(&state, 100, &mut || alloc.allocate());

    let mut pairs: Vec<_> = batch
        .iter()
        .map(|m| {
            (
                m.descriptor.player1_id.clone(),
                m.descriptor.player2_id.clone(),
            )
        })
        .collect();
    pairs.sort();
    assert_eq!(
        pairs,
        vec![("a".into(), "c".into()), ("b".into(), "c".into()),],
        "planner should re-issue exactly the two missing matchups",
    );
}
