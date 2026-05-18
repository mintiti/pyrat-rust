//! Atomicity of `EvalSession::subscribe`. The contract is "snapshot +
//! tail with no gap": the returned snapshot reflects state at some time
//! T, and the returned receiver gets every event published strictly
//! after T. Nothing falls into a gap between them.

mod common;

use std::sync::Arc;
use std::time::Duration;

use parking_lot::Mutex;
use pyrat_eval::{EvalSession, SessionConfig, SessionEvent, SessionMode};
use pyrat_eval_store::{EloOptions, EvalStore};

use crate::common::{
    embedded_player, fast_orch_config, round_robin, round_robin_spec, small_game_config,
};

/// After subscribing, the receiver is guaranteed to see
/// `TournamentFinished` once the run loop publishes it. If subscribe
/// dropped any tail event the receiver would miss the terminal and
/// this would never observe it.
#[tokio::test]
async fn subscribe_does_not_drop_tail_terminal() {
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

    // Subscribe before the tournament finishes. The (snapshot, rx) pair
    // is atomic — every event after this point lands in rx.
    let (_snapshot, mut rx) = session.subscribe();

    // Drive the run loop forward; the tournament has exactly one matchup.
    // We poll `rx` rather than calling `join`, because `join` would also
    // consume the run loop handle — we want to see events arrive on the
    // receiver independently.
    let mut events = Vec::new();
    loop {
        match tokio::time::timeout(Duration::from_secs(5), rx.recv()).await {
            Ok(Ok(ev)) => {
                let is_terminal = matches!(ev, SessionEvent::TournamentFinished);
                events.push(ev);
                if is_terminal {
                    break;
                }
            },
            Ok(Err(_)) => panic!("broadcast closed before TournamentFinished arrived"),
            Err(_) => panic!("timed out waiting for events (got: {events:?})"),
        }
    }

    assert!(
        matches!(events.last(), Some(SessionEvent::TournamentFinished)),
        "TournamentFinished must be the terminal event observed; got {events:?}"
    );

    session.shutdown().await.expect("shutdown");
}

/// Subscribing twice from the same session yields independent receivers
/// that both see the same tail. Pins the "broadcast" semantic — every
/// late subscriber gets every subsequent event, not just the next one.
#[tokio::test]
async fn two_subscribers_see_same_tail() {
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

    let (_snap_a, rx_a) = session.subscribe();
    let (_snap_b, rx_b) = session.subscribe();

    // Each receiver independently observes TournamentFinished. If
    // subscribe accidentally consumed events from a shared queue rather
    // than broadcasting, one receiver would block forever.
    let drain = |mut rx: tokio::sync::broadcast::Receiver<SessionEvent>| async move {
        loop {
            match tokio::time::timeout(Duration::from_secs(5), rx.recv()).await {
                Ok(Ok(SessionEvent::TournamentFinished)) => return Ok(()),
                Ok(Ok(_)) => continue,
                Ok(Err(e)) => return Err(format!("rx closed: {e:?}")),
                Err(_) => return Err("timeout".into()),
            }
        }
    };

    let (a, b) = tokio::join!(drain(rx_a.resubscribe()), drain(rx_b.resubscribe()));
    a.expect("rx_a should see TournamentFinished");
    b.expect("rx_b should see TournamentFinished");
    drop(rx_a);
    drop(rx_b);

    session.shutdown().await.expect("shutdown");
}
