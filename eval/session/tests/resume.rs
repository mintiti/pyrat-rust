//! Resume reconstruction: after planting partial attempt rows in the
//! store, the planner should re-issue exactly the missing matchups.

mod common;

use std::sync::Arc;

use parking_lot::Mutex;
use pyrat_eval::{
    mapping::synthetic_attempt, matchup_seed, EvalSession, MatchupKey, MatchupOutcome, Planner,
    SessionConfig, SessionMode, TournamentParams, TournamentSpec, TournamentState,
};
use pyrat_eval_store::{EloOptions, EvalStore, NewAttemptOutcome, NewPlayer, NewTournament};
use pyrat_orchestrator::MatchIdAllocator;

use crate::common::{
    embedded_player, fast_orch_config, open_store_with_config, round_robin, small_game_config,
};

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
    let key_ab = MatchupKey::from_pair("a", "b", &game_config_id, 0);
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

    // Tighten: every re-issued matchup should have attempt_index=0 (no
    // prior rows for these pairs) and the canonical seed derived from
    // tournament_seed + pair + game_config_id + repetition.
    for matchup in &batch {
        assert_eq!(
            matchup.descriptor.attempt_index, 0,
            "fresh slot (no prior rows) should use attempt_index=0"
        );
        let expected_seed = matchup_seed(
            0xC0FFEE,
            &matchup.descriptor.player1_id,
            &matchup.descriptor.player2_id,
            &game_config_id,
            matchup.descriptor.repetition_index,
        );
        assert_eq!(
            matchup.descriptor.seed, expected_seed,
            "seed should be the canonical matchup_seed of the pair"
        );
    }
}

/// Kill-9 case: a matchup whose attempt previously failed durably must
/// be re-issued at attempt_index=N+1 with the same canonical seed.
///
/// "Durably failed" means there IS a Failure row in the store. The
/// planner sees one entry in history with outcome Failure and bumps to
/// the next attempt index.
#[tokio::test]
async fn resume_retries_durably_failed_matchup_at_next_attempt_index() {
    let store = Arc::new(Mutex::new(EvalStore::open_in_memory().unwrap()));
    let game_config_id = open_store_with_config(&store);

    let p1 = embedded_player("a");
    let p2 = embedded_player("b");

    let tid = {
        let s = store.lock();
        for p in [&p1, &p2] {
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
        s.add_tournament_player(tid, &p1.id, 0).unwrap();
        s.add_tournament_player(tid, &p2.id, 1).unwrap();
        tid
    };

    // Plant a durable Failure row at attempt_index=0.
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
        NewAttemptOutcome::Failure {
            failure_reason: "spawn_failed".into(),
            started_at: None,
        },
    );
    store.lock().record_attempt(&attempt).unwrap();

    let attempts = store.lock().get_attempts(tid, None).unwrap();
    let mut state = TournamentState::empty(tid);
    for a in &attempts {
        state.fold_attempt(a);
    }

    let mut planner = round_robin(
        vec![p1, p2],
        small_game_config(),
        game_config_id.clone(),
        tid,
        1,
    );
    let alloc = MatchIdAllocator::new();
    let batch = planner.next_batch(&state, 100, &mut || alloc.allocate());

    assert_eq!(batch.len(), 1, "expected single retry");
    let m = &batch[0];
    assert_eq!(
        m.descriptor.attempt_index, 1,
        "durable failure → retry at attempt_index=1"
    );
    assert_eq!(
        m.descriptor.seed, seed_ab,
        "seed must be the same canonical seed across retries"
    );
}

/// Kill-9 case proper: no row exists at all (the original match was
/// lost without a write). The planner must re-issue at attempt_index=0
/// with the canonical seed — the same seeded game the original would
/// have played.
///
/// This is the failure mode the durability invariant exists to handle:
/// `match_attempts` is empty for this matchup, so a fresh `TournamentState`
/// reconstructed from rows has no entry for it, and the planner treats
/// it as never-issued.
#[tokio::test]
async fn resume_re_issues_kill9_matchup_at_attempt_zero_with_canonical_seed() {
    let store = Arc::new(Mutex::new(EvalStore::open_in_memory().unwrap()));
    let game_config_id = open_store_with_config(&store);

    let p1 = embedded_player("a");
    let p2 = embedded_player("b");
    let p3 = embedded_player("c");

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

    // Plant a success for (a, b) only. (a, c) and (b, c) have NO rows —
    // simulating kill-9 mid-match for those two pairs.
    let seed_ab = matchup_seed(0xC0FFEE, "a", "b", &game_config_id, 0);
    store
        .lock()
        .record_attempt(&synthetic_attempt(
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
        ))
        .unwrap();

    let attempts = store.lock().get_attempts(tid, None).unwrap();
    let mut state = TournamentState::empty(tid);
    for a in &attempts {
        state.fold_attempt(a);
    }

    let mut planner = round_robin(
        vec![p1, p2, p3],
        small_game_config(),
        game_config_id.clone(),
        tid,
        1,
    );
    let alloc = MatchIdAllocator::new();
    let batch = planner.next_batch(&state, 100, &mut || alloc.allocate());

    // Both kill-9 pairs should be re-issued at attempt_index=0 with the
    // canonical seed they would have used originally.
    assert_eq!(batch.len(), 2, "expected (a,c) and (b,c) to be re-issued");
    for matchup in &batch {
        assert_eq!(
            matchup.descriptor.attempt_index, 0,
            "kill-9 matchup with no prior row should use attempt_index=0"
        );
        let canonical_seed = matchup_seed(
            0xC0FFEE,
            &matchup.descriptor.player1_id,
            &matchup.descriptor.player2_id,
            &game_config_id,
            matchup.descriptor.repetition_index,
        );
        assert_eq!(
            matchup.descriptor.seed, canonical_seed,
            "kill-9 retry must replay the same canonical seed"
        );
    }
}

/// Regression test: subscribing to a resumed session immediately after
/// `start` should see non-empty standings in the snapshot, because Elo
/// gets recomputed *before* the watch is populated.
///
/// Before this fix, the run loop did the initial recompute as its first
/// step, so a subscriber landing between `start` returning and the loop's
/// first iteration saw empty standings.
#[tokio::test]
async fn subscribe_immediately_after_resume_sees_standings() {
    let store = Arc::new(Mutex::new(EvalStore::open_in_memory().unwrap()));
    let players = vec![embedded_player("a"), embedded_player("b")];

    // Bootstrap a tournament and plant ONE success row directly so the
    // resumed state has something to recompute Elo from.
    let spec = TournamentSpec {
        format: "round_robin".into(),
        target_games_per_matchup: Some(1),
        // Matches the helper's `round_robin` planner config below
        // (max_failures_per_pair: 3) so resume validation passes.
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

    let seed = matchup_seed(0xC0FFEE, "a", "b", &created.game_config_id, 0);
    store
        .lock()
        .record_attempt(&synthetic_attempt(
            created.tournament_id,
            &created.game_config_id,
            "a",
            "b",
            seed,
            0,
            0,
            "1970-01-01 00:01:00",
            NewAttemptOutcome::Success {
                player1_score: 1.0,
                player2_score: 0.0,
                turns: 5,
                started_at: "1970-01-01 00:00:00".into(),
            },
        ))
        .unwrap();

    let planner = round_robin(
        players,
        small_game_config(),
        created.game_config_id.clone(),
        created.tournament_id,
        1,
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

    // Immediately subscribe — *before* yielding to the run loop. Snapshot
    // must already reflect the post-resume Elo recompute, not empty
    // standings.
    let (snapshot, _rx) = session.subscribe();
    assert!(
        !snapshot.standings.is_empty(),
        "resume snapshot must include recomputed standings"
    );
    // Anchor player "a" should be at the rating system's anchor value.
    let a_rating = snapshot
        .standings
        .iter()
        .find(|r| r.player_id == "a")
        .expect("anchor player should appear in standings");
    // EloOptions::new(<anchor>) defaults to 1000.0 for the anchor.
    assert!(
        (a_rating.elo - 1000.0).abs() < 0.01,
        "anchor Elo should be 1000.0, got {}",
        a_rating.elo
    );

    session.shutdown().await.expect("shutdown");
}
