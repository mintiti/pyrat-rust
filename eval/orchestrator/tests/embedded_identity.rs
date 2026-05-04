//! `PlayerSpec::Embedded` must propagate `name` and `author` through to:
//! - `MatchEvent::BotIdentified` on the broadcast (the host emits this).
//! - `MatchOutcome.players[i].name` / `.author` (read from
//!   `Player::identity()` after handshake).

mod common;

use std::sync::Arc;
use std::time::Duration;

use pyrat_host::match_host::MatchEvent;
use pyrat_host::wire::Player as PlayerSlot;
use pyrat_orchestrator::{
    AdHocDescriptor, DriverEvent, NoOpSink, Orchestrator, OrchestratorConfig, OrchestratorEvent,
};
use tokio::time::timeout;

use common::{embedded_matchup, mock_factory};

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn embedded_name_and_author_propagate_through_events_and_outcome() {
    let cfg = OrchestratorConfig {
        max_parallel: 1,
        ..Default::default()
    };
    let sink: Arc<NoOpSink<AdHocDescriptor>> = Arc::new(NoOpSink::new());
    let (orch, mut driver_rx) = Orchestrator::new(cfg, sink);
    let mut events = orch.events();

    orch.submit(embedded_matchup(0, mock_factory(), mock_factory()))
        .await
        .expect("submit");

    let driver_task = tokio::spawn(async move {
        loop {
            match driver_rx.recv().await.expect("driver_rx") {
                DriverEvent::MatchFinished { outcome } => break outcome,
                DriverEvent::MatchFailed { failure } => {
                    panic!("unexpected failure: {:?}", failure.reason)
                },
                _ => {},
            }
        }
    });

    // Collect broadcast events; assert BotIdentified for each slot carries
    // the supplied name/author.
    let mut bot_identified_p1 = None;
    let mut bot_identified_p2 = None;
    let collect = async {
        loop {
            match events.recv().await {
                Ok(OrchestratorEvent::MatchEvent { event, .. }) => {
                    if let MatchEvent::BotIdentified {
                        player,
                        name,
                        author,
                        agent_id,
                    } = event
                    {
                        let slot = (name.clone(), author.clone(), agent_id.clone());
                        match player {
                            PlayerSlot::Player1 => bot_identified_p1 = Some(slot),
                            PlayerSlot::Player2 => bot_identified_p2 = Some(slot),
                            _ => {},
                        }
                    }
                },
                Ok(OrchestratorEvent::MatchFinished { .. })
                | Ok(OrchestratorEvent::MatchFailed { .. }) => {
                    break;
                },
                Ok(_) => {},
                Err(_) => break,
            }
        }
    };
    timeout(Duration::from_secs(10), collect)
        .await
        .expect("events");
    let outcome = driver_task.await.expect("driver task");

    let (n1, a1, id1) = bot_identified_p1.expect("BotIdentified for Player1");
    assert_eq!(n1, "Player1Bot");
    assert_eq!(a1, "tests");
    assert_eq!(id1, "test/p1");
    let (n2, a2, id2) = bot_identified_p2.expect("BotIdentified for Player2");
    assert_eq!(n2, "Player2Bot");
    assert_eq!(a2, "tests");
    assert_eq!(id2, "test/p2");

    // Outcome.players carry the same identities (read from the player
    // handles post-handshake).
    assert_eq!(outcome.players[0].name, "Player1Bot");
    assert_eq!(outcome.players[0].author, "tests");
    assert_eq!(outcome.players[0].agent_id, "test/p1");
    assert_eq!(outcome.players[1].name, "Player2Bot");
    assert_eq!(outcome.players[1].author, "tests");
    assert_eq!(outcome.players[1].agent_id, "test/p2");
}
