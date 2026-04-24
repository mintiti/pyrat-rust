//! Integration tests for the in-process [`EmbeddedPlayer`].
//!
//! These exercise the full `Player` trait surface from outside the crate.
//! They don't reach into private helpers, so paths that need
//! hash-verification (Advance + SyncOk) are covered by inline unit tests in
//! `player/embedded.rs`; this file covers end-to-end flows that only need
//! public API.

use std::time::Duration;

use pyrat::{Coordinates, Direction};
use pyrat_host::game_loop::MatchEvent;
use pyrat_host::player::{
    EmbeddedBot, EmbeddedCtx, EmbeddedPlayer, EventSink, InfoParams, Options, Player,
    PlayerError, PlayerIdentity,
};
use pyrat_protocol::{
    BotMsg, HashedTurnState, HostMsg, OwnedMatchConfig, OwnedTurnState, SearchLimits,
};
use pyrat_wire::{GameResult, Player as PlayerSlot, TimingMode};
use tokio::sync::mpsc;
use tokio::time::timeout;

// ── Test fixtures ─────────────────────────────────────

fn identity() -> PlayerIdentity {
    PlayerIdentity {
        name: "TestBot".into(),
        author: "tests".into(),
        agent_id: "pyrat/test".into(),
    }
}

fn sample_match_config() -> Box<OwnedMatchConfig> {
    Box::new(OwnedMatchConfig {
        width: 5,
        height: 5,
        max_turns: 100,
        walls: vec![],
        mud: vec![],
        cheese: vec![Coordinates::new(2, 2)],
        player1_start: Coordinates::new(0, 0),
        player2_start: Coordinates::new(4, 4),
        controlled_players: vec![PlayerSlot::Player1],
        timing: TimingMode::Wait,
        move_timeout_ms: 100,
        preprocessing_timeout_ms: 1000,
    })
}

/// Drive an [`EmbeddedPlayer`] through setup (Identify → Welcome →
/// Configure → Ready), returning the hash the bot announced.
async fn walk_through_setup(player: &mut EmbeddedPlayer) -> u64 {
    match recv_ok(player).await {
        BotMsg::Identify { .. } => {},
        other => panic!("expected Identify, got {other:?}"),
    }
    player
        .send(HostMsg::Welcome {
            player_slot: PlayerSlot::Player1,
        })
        .await
        .unwrap();
    player
        .send(HostMsg::Configure {
            options: vec![],
            match_config: sample_match_config(),
        })
        .await
        .unwrap();
    match recv_ok(player).await {
        BotMsg::Ready { state_hash } => state_hash,
        other => panic!("expected Ready, got {other:?}"),
    }
}

/// Short-circuit helper: await `recv()` with a timeout so a hung test fails
/// fast instead of hanging CI.
async fn recv_ok(player: &mut EmbeddedPlayer) -> BotMsg {
    timeout(Duration::from_secs(2), player.recv())
        .await
        .expect("recv timed out")
        .expect("recv returned Err")
        .expect("recv returned Ok(None)")
}

// ── Test bots ─────────────────────────────────────────

struct StayBot;
impl Options for StayBot {}
impl EmbeddedBot for StayBot {
    fn think(&mut self, _: &HashedTurnState, _: &EmbeddedCtx) -> Direction {
        Direction::Stay
    }
}

/// Returns a different direction depending on the turn number the dispatcher
/// passes in — lets tests verify `GoState` overwrote the local mirror.
struct TurnSensitiveBot;
impl Options for TurnSensitiveBot {}
impl EmbeddedBot for TurnSensitiveBot {
    fn think(&mut self, state: &HashedTurnState, _: &EmbeddedCtx) -> Direction {
        if state.turn == 42 {
            Direction::Right
        } else {
            Direction::Down
        }
    }
}

/// Calls `ctx.send_info` during `think`, then returns `Stay`. Used to verify
/// sideband routing.
struct InfoEmittingBot;
impl Options for InfoEmittingBot {}
impl EmbeddedBot for InfoEmittingBot {
    fn think(&mut self, _: &HashedTurnState, ctx: &EmbeddedCtx) -> Direction {
        ctx.send_info(&InfoParams {
            depth: 3,
            nodes: 100,
            message: "analysis",
            ..InfoParams::for_player(PlayerSlot::Player1)
        });
        Direction::Stay
    }
}

/// Cooperative-stop bot. Busy-waits until `should_stop` flips, then returns
/// `Right` (distinct from `Stay` so the test can tell early-exit from
/// never-started).
struct SpinBot;
impl Options for SpinBot {}
impl EmbeddedBot for SpinBot {
    fn think(&mut self, _: &HashedTurnState, ctx: &EmbeddedCtx) -> Direction {
        while !ctx.should_stop() {
            std::thread::sleep(Duration::from_millis(1));
        }
        Direction::Right
    }
}

// ── Tests ─────────────────────────────────────────────

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn happy_path_preprocess_think_game_over() {
    let mut player = EmbeddedPlayer::new(StayBot, identity(), EventSink::noop());
    let hash = walk_through_setup(&mut player).await;

    // Preprocess → PreprocessingDone.
    player
        .send(HostMsg::GoPreprocess { state_hash: hash })
        .await
        .unwrap();
    match recv_ok(&mut player).await {
        BotMsg::PreprocessingDone => {},
        other => panic!("expected PreprocessingDone, got {other:?}"),
    }

    // Go (first turn, direct from Configured) → Action.
    player
        .send(HostMsg::Go {
            state_hash: hash,
            limits: SearchLimits::default(),
        })
        .await
        .unwrap();
    match recv_ok(&mut player).await {
        BotMsg::Action {
            direction,
            player: slot,
            state_hash,
            ..
        } => {
            assert_eq!(direction, Direction::Stay);
            assert_eq!(slot, PlayerSlot::Player1);
            assert_eq!(state_hash, hash);
        },
        other => panic!("expected Action, got {other:?}"),
    }

    // GameOver → dispatcher exits cleanly.
    player
        .send(HostMsg::GameOver {
            result: GameResult::Draw,
            player1_score: 0.0,
            player2_score: 0.0,
        })
        .await
        .unwrap();

    // After GameOver, bot_rx drains and dispatcher drops its sender.
    let next = timeout(Duration::from_secs(2), player.recv())
        .await
        .expect("recv timed out");
    assert!(matches!(next, Ok(None)), "{next:?}");
    player.close().await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn info_routes_to_event_sink_not_bot_recv() {
    let (event_tx, mut event_rx) = mpsc::unbounded_channel::<MatchEvent>();
    let mut player =
        EmbeddedPlayer::new(InfoEmittingBot, identity(), EventSink::new(event_tx));
    let hash = walk_through_setup(&mut player).await;

    player
        .send(HostMsg::Go {
            state_hash: hash,
            limits: SearchLimits::default(),
        })
        .await
        .unwrap();

    // The bot emits Info (sideband) then Action (game-driving). Only Action
    // surfaces through recv(); Info arrives on the EventSink.
    let msg = recv_ok(&mut player).await;
    assert!(
        matches!(msg, BotMsg::Action { .. }),
        "recv() yielded sideband: {msg:?}"
    );

    let event = timeout(Duration::from_secs(1), event_rx.recv())
        .await
        .expect("EventSink timed out")
        .expect("EventSink closed");
    match event {
        MatchEvent::BotInfo { info, sender, .. } => {
            assert_eq!(sender, PlayerSlot::Player1);
            assert_eq!(info.message, "analysis");
            assert_eq!(info.depth, 3);
            assert_eq!(info.nodes, 100);
        },
        other => panic!("expected BotInfo, got {other:?}"),
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn desync_emits_resync_then_fullstate_recovers() {
    let mut player = EmbeddedPlayer::new(StayBot, identity(), EventSink::noop());
    let hash = walk_through_setup(&mut player).await;

    // First turn to move the client past the initial-Go gate. After Action
    // we're back in Idle and can send Advance.
    player
        .send(HostMsg::Go {
            state_hash: hash,
            limits: SearchLimits::default(),
        })
        .await
        .unwrap();
    let _action = recv_ok(&mut player).await;

    // Send an Advance with a deliberately wrong new_hash.
    player
        .send(HostMsg::Advance {
            p1_dir: Direction::Stay,
            p2_dir: Direction::Stay,
            turn: 1,
            new_hash: 0xDEAD_BEEF_DEAD_BEEF,
        })
        .await
        .unwrap();

    let bot_hash = match recv_ok(&mut player).await {
        BotMsg::Resync { my_hash } => my_hash,
        other => panic!("expected Resync, got {other:?}"),
    };
    assert_ne!(bot_hash, 0xDEAD_BEEF_DEAD_BEEF);

    // Send a FullState that restores a known position. The bot should emit
    // SyncOk with the hash of that state.
    let recovery_state = OwnedTurnState {
        turn: 1,
        player1_position: Coordinates::new(1, 0),
        player2_position: Coordinates::new(4, 4),
        player1_score: 0.0,
        player2_score: 0.0,
        player1_mud_turns: 0,
        player2_mud_turns: 0,
        cheese: vec![Coordinates::new(2, 2)],
        player1_last_move: Direction::Right,
        player2_last_move: Direction::Stay,
    };
    player
        .send(HostMsg::FullState {
            match_config: sample_match_config(),
            turn_state: Box::new(recovery_state),
        })
        .await
        .unwrap();
    match recv_ok(&mut player).await {
        BotMsg::SyncOk { .. } => {},
        other => panic!("expected SyncOk after FullState, got {other:?}"),
    }

    // Bot is Synced again; we can proceed. Close cleanly via GameOver.
    player
        .send(HostMsg::GameOver {
            result: GameResult::Draw,
            player1_score: 0.0,
            player2_score: 0.0,
        })
        .await
        .unwrap();
    let next = timeout(Duration::from_secs(1), player.recv())
        .await
        .expect("recv timed out");
    assert!(matches!(next, Ok(None)));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn stop_cancels_think_cooperatively() {
    let mut player = EmbeddedPlayer::new(SpinBot, identity(), EventSink::noop());
    let hash = walk_through_setup(&mut player).await;

    // Kick off a Go — the bot spins until should_stop() flips.
    player
        .send(HostMsg::Go {
            state_hash: hash,
            limits: SearchLimits::default(),
        })
        .await
        .unwrap();

    // Give the bot a moment to enter its spin loop, then send Stop.
    tokio::time::sleep(Duration::from_millis(20)).await;
    player.send(HostMsg::Stop).await.unwrap();

    // The bot should observe should_stop and return Direction::Right.
    match recv_ok(&mut player).await {
        BotMsg::Action { direction, .. } => {
            assert_eq!(direction, Direction::Right);
        },
        other => panic!("expected Action after Stop, got {other:?}"),
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn go_state_overrides_local_mirror() {
    let mut player = EmbeddedPlayer::new(TurnSensitiveBot, identity(), EventSink::noop());
    let _hash = walk_through_setup(&mut player).await;

    // Local mirror has turn=0; TurnSensitiveBot would return Down. Inject
    // turn=42 via GoState and expect Right. Compute the canonical hash of
    // the injected state so the dispatcher's verification accepts it.
    let injected = OwnedTurnState {
        turn: 42,
        player1_position: Coordinates::new(0, 0),
        player2_position: Coordinates::new(4, 4),
        player1_score: 0.0,
        player2_score: 0.0,
        player1_mud_turns: 0,
        player2_mud_turns: 0,
        cheese: vec![Coordinates::new(2, 2)],
        player1_last_move: Direction::Stay,
        player2_last_move: Direction::Stay,
    };
    let state_hash = HashedTurnState::new(injected.clone()).state_hash();
    player
        .send(HostMsg::GoState {
            turn_state: Box::new(injected),
            state_hash,
            limits: SearchLimits::default(),
        })
        .await
        .unwrap();

    match recv_ok(&mut player).await {
        BotMsg::Action { direction, .. } => {
            assert_eq!(direction, Direction::Right);
        },
        other => panic!("expected Action, got {other:?}"),
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn close_during_idle_exits_cleanly() {
    let mut player = EmbeddedPlayer::new(StayBot, identity(), EventSink::noop());
    let _hash = walk_through_setup(&mut player).await;

    // Sitting in Playing<Synced> / Idle. Closing drops host_tx; the
    // dispatcher sees host_rx.recv() yield None from Idle and exits Ok(()).
    player.close().await.expect("close should succeed");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn protocol_error_message_propagates() {
    let mut player = EmbeddedPlayer::new(StayBot, identity(), EventSink::noop());
    let _hash = walk_through_setup(&mut player).await;

    // Synced + Idle: send a ProtocolError to simulate the server signalling a
    // terminal protocol fault. Dispatcher returns Err, surfaced on recv().
    player
        .send(HostMsg::ProtocolError {
            reason: "test fault".into(),
        })
        .await
        .unwrap();

    let err = timeout(Duration::from_secs(1), player.recv())
        .await
        .expect("recv timed out")
        .expect_err("expected protocol error");
    match err {
        PlayerError::ProtocolError(msg) => assert!(msg.contains("test fault")),
        other => panic!("expected ProtocolError, got {other:?}"),
    }
}
