//! Integration tests for the in-process [`EmbeddedPlayer`].
//!
//! These exercise the full `Player` trait surface from outside the crate.
//! They don't reach into private helpers, so paths that need
//! hash-verification (Advance + SyncOk) are covered by inline unit tests in
//! `player/embedded.rs`; this file covers end-to-end flows that only need
//! public API.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use pyrat::{Coordinates, Direction};
use pyrat_host::game_loop::MatchEvent;
use pyrat_host::player::{
    EmbeddedBot, EmbeddedCtx, EmbeddedPlayer, EventSink, InfoParams, Options, Player, PlayerError,
    PlayerIdentity,
};
use pyrat_protocol::{
    BotMsg, HashedTurnState, HostMsg, OwnedMatchConfig, OwnedTurnState, SearchLimits,
};
use pyrat_wire::{GameResult, Player as PlayerSlot, TimingMode};
use tokio::sync::{mpsc, Notify};
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

/// Default `OwnedTurnState` matching `sample_match_config`'s layout: players
/// at their starting corners, one cheese at (2, 2), no mud, zero scores,
/// last moves `Stay`. Callers override fields via struct update syntax.
fn base_turn_state(turn: u16) -> OwnedTurnState {
    OwnedTurnState {
        turn,
        player1_position: Coordinates::new(0, 0),
        player2_position: Coordinates::new(4, 4),
        player1_score: 0.0,
        player2_score: 0.0,
        player1_mud_turns: 0,
        player2_mud_turns: 0,
        cheese: vec![Coordinates::new(2, 2)],
        player1_last_move: Direction::Stay,
        player2_last_move: Direction::Stay,
    }
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
/// passes in. Lets tests verify `GoState` overwrote the local mirror.
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

/// Cooperative-stop bot. Signals `started` once `think` is entered, then
/// busy-waits until `should_stop` flips. Returns `Right` (distinct from
/// `Stay` so the test can tell early-exit from never-started).
struct SpinBot {
    started: Arc<Notify>,
}
impl Options for SpinBot {}
impl EmbeddedBot for SpinBot {
    fn think(&mut self, _: &HashedTurnState, ctx: &EmbeddedCtx) -> Direction {
        self.started.notify_one();
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
    let mut player = EmbeddedPlayer::new(InfoEmittingBot, identity(), EventSink::new(event_tx));
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
        player1_position: Coordinates::new(1, 0),
        player1_last_move: Direction::Right,
        ..base_turn_state(1)
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
    let started = Arc::new(Notify::new());
    let mut player = EmbeddedPlayer::new(
        SpinBot {
            started: started.clone(),
        },
        identity(),
        EventSink::noop(),
    );
    let hash = walk_through_setup(&mut player).await;

    // Kick off a Go; the bot spins until should_stop() flips.
    player
        .send(HostMsg::Go {
            state_hash: hash,
            limits: SearchLimits::default(),
        })
        .await
        .unwrap();

    // Wait for the bot to enter think, then send Stop.
    started.notified().await;
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
    let injected = base_turn_state(42);
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

// ── New coverage ──────────────────────────────────────

/// Bot that calls `ctx.send_provisional(Right)` from inside `think`, then
/// returns `Stay` as its committed move. Lets the test verify that
/// provisional messages flow through `recv()` before the final Action.
struct ProvisionalBot;
impl Options for ProvisionalBot {}
impl EmbeddedBot for ProvisionalBot {
    fn think(&mut self, _: &HashedTurnState, ctx: &EmbeddedCtx) -> Direction {
        ctx.send_provisional(Direction::Right);
        Direction::Stay
    }
}

type GameOverRecord = Arc<Mutex<Option<(GameResult, (f32, f32))>>>;

/// Bot that records its `on_game_over` invocation.
struct GameOverBot {
    called: GameOverRecord,
}
impl Options for GameOverBot {}
impl EmbeddedBot for GameOverBot {
    fn think(&mut self, _: &HashedTurnState, _: &EmbeddedCtx) -> Direction {
        Direction::Stay
    }
    fn on_game_over(&mut self, result: GameResult, scores: (f32, f32)) {
        *self.called.lock().unwrap() = Some((result, scores));
    }
}

/// Bot whose `think` blocks for 100ms with `std::thread::sleep`, ignoring
/// `should_stop`. Used to force the dispatcher into a mid-think state while
/// the test injects a forbidden host message.
struct DelayedBot;
impl Options for DelayedBot {}
impl EmbeddedBot for DelayedBot {
    fn think(&mut self, _: &HashedTurnState, _: &EmbeddedCtx) -> Direction {
        std::thread::sleep(Duration::from_millis(100));
        Direction::Stay
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn recv_is_cancel_safe() {
    let mut player = EmbeddedPlayer::new(StayBot, identity(), EventSink::noop());
    let hash = walk_through_setup(&mut player).await;

    // Drop a pending recv() by letting a biased zero-duration timer win.
    // The trait contract says the next recv() must still deliver any
    // message that arrives after cancellation.
    tokio::select! {
        biased;
        () = tokio::time::sleep(Duration::from_millis(0)) => {},
        _ = player.recv() => panic!("recv should lose to zero-duration timer"),
    }

    // Now produce a BotMsg and verify recv still yields it.
    player
        .send(HostMsg::Go {
            state_hash: hash,
            limits: SearchLimits::default(),
        })
        .await
        .unwrap();
    match recv_ok(&mut player).await {
        BotMsg::Action { .. } => {},
        other => panic!("recv lost a message after cancel: {other:?}"),
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn provisional_routes_through_recv() {
    let mut player = EmbeddedPlayer::new(ProvisionalBot, identity(), EventSink::noop());
    let hash = walk_through_setup(&mut player).await;

    player
        .send(HostMsg::Go {
            state_hash: hash,
            limits: SearchLimits::default(),
        })
        .await
        .unwrap();

    // Provisional arrives first, then the committed Action.
    match recv_ok(&mut player).await {
        BotMsg::Provisional {
            direction,
            player: slot,
            state_hash: h,
            ..
        } => {
            assert_eq!(direction, Direction::Right);
            assert_eq!(slot, PlayerSlot::Player1);
            assert_eq!(h, hash);
        },
        other => panic!("expected Provisional, got {other:?}"),
    }
    match recv_ok(&mut player).await {
        BotMsg::Action { direction, .. } => assert_eq!(direction, Direction::Stay),
        other => panic!("expected Action, got {other:?}"),
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn on_game_over_is_invoked() {
    let called = Arc::new(Mutex::new(None));
    let mut player = EmbeddedPlayer::new(
        GameOverBot {
            called: called.clone(),
        },
        identity(),
        EventSink::noop(),
    );
    let _hash = walk_through_setup(&mut player).await;

    player
        .send(HostMsg::GameOver {
            result: GameResult::Player1,
            player1_score: 5.0,
            player2_score: 2.5,
        })
        .await
        .unwrap();

    // Dispatcher processes GameOver and exits cleanly: next recv yields None.
    let next = timeout(Duration::from_secs(1), player.recv())
        .await
        .expect("recv timed out");
    assert!(matches!(next, Ok(None)));
    player.close().await.expect("close should succeed");

    let recorded = *called.lock().unwrap();
    let (result, scores) = recorded.expect("on_game_over not called");
    assert!(matches!(result, GameResult::Player1));
    assert!((scores.0 - 5.0).abs() < f32::EPSILON);
    assert!((scores.1 - 2.5).abs() < f32::EPSILON);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn protocol_error_fullstate_while_synced() {
    let mut player = EmbeddedPlayer::new(StayBot, identity(), EventSink::noop());
    let _hash = walk_through_setup(&mut player).await;

    // In Playing<Synced>/Idle; a FullState arriving here is a protocol
    // violation (the server must only send FullState after a Resync).
    player
        .send(HostMsg::FullState {
            match_config: sample_match_config(),
            turn_state: Box::new(base_turn_state(0)),
        })
        .await
        .unwrap();

    let err = timeout(Duration::from_secs(1), player.recv())
        .await
        .expect("recv timed out")
        .expect_err("expected protocol error");
    match err {
        PlayerError::ProtocolError(msg) => {
            assert!(msg.contains("FullState received while Synced"), "msg={msg}");
            assert!(msg.contains("player=Player1"), "msg={msg}");
        },
        other => panic!("expected ProtocolError, got {other:?}"),
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn protocol_error_go_preprocess_while_syncing() {
    let mut player = EmbeddedPlayer::new(StayBot, identity(), EventSink::noop());
    let hash0 = walk_through_setup(&mut player).await;

    // Walk one turn to land in InnerState::Syncing: Go -> Action -> Advance
    // -> SyncOk. After SyncOk the dispatcher is in Syncing, awaiting Go or
    // FullState. GoPreprocess is forbidden here.
    player
        .send(HostMsg::Go {
            state_hash: hash0,
            limits: SearchLimits::default(),
        })
        .await
        .unwrap();
    match recv_ok(&mut player).await {
        BotMsg::Action { .. } => {},
        other => panic!("expected Action, got {other:?}"),
    }

    // Apply a (Stay, Stay) advance. The post-move local state has turn=1
    // and its canonical hash is what the bot will compare against.
    let hash1 = HashedTurnState::new(base_turn_state(1)).state_hash();
    player
        .send(HostMsg::Advance {
            p1_dir: Direction::Stay,
            p2_dir: Direction::Stay,
            turn: 1,
            new_hash: hash1,
        })
        .await
        .unwrap();
    match recv_ok(&mut player).await {
        BotMsg::SyncOk { .. } => {},
        other => panic!("expected SyncOk, got {other:?}"),
    }

    // Dispatcher is Syncing; GoPreprocess from Syncing is a protocol error.
    player
        .send(HostMsg::GoPreprocess { state_hash: hash1 })
        .await
        .unwrap();
    let err = timeout(Duration::from_secs(1), player.recv())
        .await
        .expect("recv timed out")
        .expect_err("expected protocol error");
    match err {
        PlayerError::ProtocolError(msg) => {
            assert!(msg.contains("GoPreprocess in state Syncing"), "msg={msg}");
            assert!(msg.contains("player=Player1"), "msg={msg}");
            assert!(msg.contains("turn=1"), "msg={msg}");
        },
        other => panic!("expected ProtocolError, got {other:?}"),
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn protocol_error_advance_while_thinking() {
    let mut player = EmbeddedPlayer::new(DelayedBot, identity(), EventSink::noop());
    let hash = walk_through_setup(&mut player).await;

    player
        .send(HostMsg::Go {
            state_hash: hash,
            limits: SearchLimits::default(),
        })
        .await
        .unwrap();

    // Bot is now sleeping inside spawn_blocking. Send a forbidden Advance
    // while it works; the dispatcher's watch_for_stop should reject it.
    tokio::time::sleep(Duration::from_millis(20)).await;
    player
        .send(HostMsg::Advance {
            p1_dir: Direction::Stay,
            p2_dir: Direction::Stay,
            turn: 1,
            new_hash: 0,
        })
        .await
        .unwrap();

    let err = timeout(Duration::from_secs(2), player.recv())
        .await
        .expect("recv timed out")
        .expect_err("expected protocol error");
    match err {
        PlayerError::ProtocolError(msg) => {
            assert!(msg.contains("Advance"), "msg={msg}");
            assert!(msg.contains("bot is working"), "msg={msg}");
            assert!(msg.contains("player=Player1"), "msg={msg}");
        },
        other => panic!("expected ProtocolError, got {other:?}"),
    }

    // Let the detached blocking bot task finish before the runtime shuts
    // down. DelayedBot sleeps 100ms; give it headroom.
    tokio::time::sleep(Duration::from_millis(150)).await;
}

// ── close + think_ms coverage ────────────────────────

/// Bot that sleeps inside `think` for longer than the close grace period
/// and never polls `should_stop`. Used to verify `close` is bounded by the
/// grace timeout, not by the bot's wall time.
struct UncooperativeSleeperBot;
impl Options for UncooperativeSleeperBot {}
impl EmbeddedBot for UncooperativeSleeperBot {
    fn think(&mut self, _: &HashedTurnState, _: &EmbeddedCtx) -> Direction {
        // Must exceed `embedded::CLOSE_GRACE` (1s) so the close timeout fires.
        std::thread::sleep(Duration::from_millis(1200));
        Direction::Stay
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn close_during_cooperative_think_returns_promptly() {
    let started = Arc::new(Notify::new());
    let mut player = EmbeddedPlayer::new(
        SpinBot {
            started: started.clone(),
        },
        identity(),
        EventSink::noop(),
    );
    let hash = walk_through_setup(&mut player).await;

    player
        .send(HostMsg::Go {
            state_hash: hash,
            limits: SearchLimits::default(),
        })
        .await
        .unwrap();
    started.notified().await;

    // SpinBot exits as soon as `should_stop()` flips. close should set the
    // flag and reap well under the grace period.
    let close_start = std::time::Instant::now();
    player.close().await.expect("close should succeed");
    let elapsed = close_start.elapsed();
    assert!(
        elapsed < Duration::from_millis(300),
        "close took {elapsed:?}, expected fast exit for cooperative bot"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn close_during_uncooperative_think_is_bounded() {
    let mut player = EmbeddedPlayer::new(UncooperativeSleeperBot, identity(), EventSink::noop());
    let hash = walk_through_setup(&mut player).await;

    player
        .send(HostMsg::Go {
            state_hash: hash,
            limits: SearchLimits::default(),
        })
        .await
        .unwrap();
    // Let the bot enter spawn_blocking before close.
    tokio::time::sleep(Duration::from_millis(50)).await;

    // close should fire its grace timeout (~1s) and abort the dispatcher.
    // Bound the assertion at 1.5s to leave room for scheduling jitter.
    let close_start = std::time::Instant::now();
    player.close().await.expect("close should succeed within grace");
    let elapsed = close_start.elapsed();
    assert!(
        elapsed < Duration::from_millis(1500),
        "close took {elapsed:?}, expected bounded by CLOSE_GRACE (~1s)"
    );

    // Let the detached blocking bot task drain before runtime shutdown.
    // UncooperativeSleeperBot sleeps 1200ms; give it headroom from t=0.
    tokio::time::sleep(Duration::from_millis(1300)).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn think_ms_clamped_to_one_for_fast_bots() {
    let mut player = EmbeddedPlayer::new(StayBot, identity(), EventSink::noop());
    let hash = walk_through_setup(&mut player).await;

    player
        .send(HostMsg::Go {
            state_hash: hash,
            limits: SearchLimits::default(),
        })
        .await
        .unwrap();

    match recv_ok(&mut player).await {
        BotMsg::Action { think_ms, .. } => {
            assert!(
                think_ms >= 1,
                "think_ms must be clamped to >=1 (host rejects 0), got {think_ms}"
            );
        },
        other => panic!("expected Action, got {other:?}"),
    }
}
