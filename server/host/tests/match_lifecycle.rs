//! Integration tests for the [`Match`] typestate.
//!
//! These exercise the full Match lifecycle: setup (Configure → Ready →
//! GoPreprocess → PreprocessingDone), playing (Advance → SyncOk → Go →
//! Action), Resync recovery, ReadyHashMismatch failure paths, the
//! FaultPolicy seam (Default vs Strict), and the analysis sub-states
//! (`Thinking` / `Collected`) for GUI step-mode.
//!
//! The happy path uses two `EmbeddedPlayer<StayBot>` instances boxed as
//! `dyn Player`. The lie-based tests (Resync, hash mismatch) use a small
//! channel-backed `ScriptedPlayer` so the test can choose what each bot
//! reports.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_trait::async_trait;
use pyrat::{Coordinates, Direction, GameBuilder, GameState};
use pyrat_host::match_config::build_match_config;
use pyrat_host::match_host::{
    Match, MatchError, MatchEvent, PlayingConfig, SetupTiming, StepResult, StrictFaultPolicy,
};
use pyrat_host::player::{
    EmbeddedBot, EmbeddedCtx, EmbeddedPlayer, EventSink, Options, Player, PlayerError,
    PlayerIdentity,
};
use pyrat_protocol::{BotMsg, HashedTurnState, HostMsg};
use pyrat_wire::{GameResult, Player as PlayerSlot, TimingMode};
use tokio::sync::mpsc;
use tokio::time::timeout;

// ── Shared fixtures ───────────────────────────────────

fn make_game() -> GameState {
    GameBuilder::new(5, 5)
        .with_max_turns(5)
        .with_open_maze()
        .with_custom_positions(Coordinates::new(0, 0), Coordinates::new(4, 4))
        .with_custom_cheese(vec![Coordinates::new(2, 2)])
        .build()
        .create(Some(42))
        .expect("create game")
}

/// Single-turn variant for policy tests — match ends after turn 0 so the
/// driver script doesn't need to handle Advance/Sync past the timeout.
fn make_game_one_turn() -> GameState {
    GameBuilder::new(5, 5)
        .with_max_turns(1)
        .with_open_maze()
        .with_custom_positions(Coordinates::new(0, 0), Coordinates::new(4, 4))
        .with_custom_cheese(vec![Coordinates::new(2, 2)])
        .build()
        .create(Some(42))
        .expect("create game")
}

fn identity(slot: PlayerSlot, name: &str) -> PlayerIdentity {
    PlayerIdentity {
        name: name.into(),
        author: "tests".into(),
        agent_id: format!("pyrat/test/{name}"),
        slot,
    }
}

fn fast_setup() -> SetupTiming {
    SetupTiming {
        configure_timeout: Duration::from_secs(2),
        preprocessing_timeout: Duration::from_secs(2),
    }
}

fn fast_playing() -> PlayingConfig {
    PlayingConfig {
        move_timeout: Duration::from_millis(500),
        network_grace: Duration::from_millis(50),
        ..Default::default()
    }
}

// ── EmbeddedBot for happy path ────────────────────────

struct StayBot;
impl Options for StayBot {}
impl EmbeddedBot for StayBot {
    fn think(&mut self, _: &HashedTurnState, _: &EmbeddedCtx) -> Direction {
        Direction::Stay
    }
}

// ── ScriptedPlayer: channel-backed Player for lie-based tests ─

/// Test-only `Player` that exposes raw `send`/`recv` channels so a test
/// can drive arbitrary protocol exchanges.
struct ScriptedPlayer {
    identity: PlayerIdentity,
    host_tx: mpsc::UnboundedSender<HostMsg>,
    bot_rx: mpsc::UnboundedReceiver<BotMsg>,
    /// Returned (and cleared) by `take_provisional`. Tests for the
    /// timeout-resolution seam set this to verify the FaultPolicy hook;
    /// scripts that don't care leave it `None`.
    stored_provisional: Option<Direction>,
}

struct ScriptedHandle {
    host_rx: mpsc::UnboundedReceiver<HostMsg>,
    bot_tx: mpsc::UnboundedSender<BotMsg>,
}

impl ScriptedPlayer {
    fn pair(identity: PlayerIdentity) -> (Self, ScriptedHandle) {
        Self::pair_with_provisional(identity, None)
    }

    fn pair_with_provisional(
        identity: PlayerIdentity,
        stored_provisional: Option<Direction>,
    ) -> (Self, ScriptedHandle) {
        let (host_tx, host_rx) = mpsc::unbounded_channel();
        let (bot_tx, bot_rx) = mpsc::unbounded_channel();
        (
            Self {
                identity,
                host_tx,
                bot_rx,
                stored_provisional,
            },
            ScriptedHandle { host_rx, bot_tx },
        )
    }
}

#[async_trait]
impl Player for ScriptedPlayer {
    fn identity(&self) -> &PlayerIdentity {
        &self.identity
    }

    async fn send(&mut self, msg: HostMsg) -> Result<(), PlayerError> {
        self.host_tx
            .send(msg)
            .map_err(|_| PlayerError::TransportError("test driver dropped host_rx".into()))
    }

    async fn recv(&mut self) -> Result<Option<BotMsg>, PlayerError> {
        Ok(self.bot_rx.recv().await)
    }

    fn take_provisional(&mut self, _turn: u16, _hash: u64) -> Option<Direction> {
        self.stored_provisional.take()
    }

    async fn close(self: Box<Self>) -> Result<(), PlayerError> {
        Ok(())
    }
}

impl ScriptedHandle {
    async fn expect_send(&mut self) -> HostMsg {
        timeout(Duration::from_secs(2), self.host_rx.recv())
            .await
            .expect("expect_send timed out")
            .expect("host_tx dropped")
    }

    fn respond(&self, msg: BotMsg) {
        self.bot_tx.send(msg).expect("bot_rx dropped");
    }
}

// ── Tests ─────────────────────────────────────────────

/// Two cooperative bots run a 5-turn match to completion. Validates the
/// full pipeline: setup → start → step loop → game over → MatchOver event.
#[tokio::test]
async fn match_run_completes_with_two_embedded_bots() {
    let game = make_game();
    let cfg = build_match_config(&game, TimingMode::Wait, 500, 1000);

    let (event_tx, mut event_rx) = mpsc::unbounded_channel::<MatchEvent>();
    let sink = EventSink::new(event_tx.clone());
    let p1 = Box::new(
        EmbeddedPlayer::accept(StayBot, identity(PlayerSlot::Player1, "p1"), sink.clone())
            .await
            .expect("accept p1"),
    ) as Box<dyn Player>;
    let p2 = Box::new(
        EmbeddedPlayer::accept(StayBot, identity(PlayerSlot::Player2, "p2"), sink)
            .await
            .expect("accept p2"),
    ) as Box<dyn Player>;

    let m = Match::new(
        game,
        [p1, p2],
        cfg,
        [vec![], vec![]],
        fast_setup(),
        fast_playing(),
        Some(event_tx),
    );

    let result = timeout(Duration::from_secs(5), m.run())
        .await
        .expect("match run hung")
        .expect("match run failed");

    // Both bots Stay forever, no cheese collected, max_turns=5 → draw.
    assert_eq!(result.result, GameResult::Draw);
    assert_eq!(result.turns_played, 5);

    // Drain events: SetupComplete, MatchStarted, 5x TurnPlayed, MatchOver.
    let mut events = vec![];
    while let Ok(ev) = event_rx.try_recv() {
        events.push(ev);
    }
    let n_turns = events
        .iter()
        .filter(|e| matches!(e, MatchEvent::TurnPlayed { .. }))
        .count();
    assert_eq!(n_turns, 5, "expected 5 TurnPlayed events, got {n_turns}");
    assert!(events
        .iter()
        .any(|e| matches!(e, MatchEvent::SetupComplete)));
    assert!(events
        .iter()
        .any(|e| matches!(e, MatchEvent::MatchStarted { .. })));
    assert!(events
        .iter()
        .any(|e| matches!(e, MatchEvent::MatchOver { .. })));
}

/// Bot that lies about its initial hash → `MatchError::ReadyHashMismatch`.
#[tokio::test]
async fn ready_hash_mismatch_aborts_setup() {
    let game = make_game();
    let cfg = build_match_config(&game, TimingMode::Wait, 500, 1000);

    let (p1_player, p1_handle) = ScriptedPlayer::pair(identity(PlayerSlot::Player1, "p1"));
    let (p2_player, p2_handle) = ScriptedPlayer::pair(identity(PlayerSlot::Player2, "p2"));

    let p1: Box<dyn Player> = Box::new(p1_player);
    let p2: Box<dyn Player> = Box::new(p2_player);

    let m = Match::new(
        game,
        [p1, p2],
        cfg,
        [vec![], vec![]],
        fast_setup(),
        fast_playing(),
        None,
    );

    // Drive the bot side. P1 returns the wrong hash.
    let driver = tokio::spawn(async move {
        let mut p1 = p1_handle;
        let mut p2 = p2_handle;
        // Configure → Ready
        match p1.expect_send().await {
            HostMsg::Configure { .. } => {},
            other => panic!("p1 expected Configure, got {other:?}"),
        }
        match p2.expect_send().await {
            HostMsg::Configure { .. } => {},
            other => panic!("p2 expected Configure, got {other:?}"),
        }
        // P1 lies; P2 still respond cleanly so order doesn't matter.
        p1.respond(BotMsg::Ready {
            state_hash: 0xDEAD_BEEF,
        });
        p2.respond(BotMsg::Ready { state_hash: 0 });
    });

    let err = timeout(Duration::from_secs(3), m.run())
        .await
        .expect("run hung")
        .expect_err("expected ReadyHashMismatch");

    driver.abort();
    let _ = driver.await;

    match err {
        MatchError::ReadyHashMismatch { slot, .. } => {
            assert_eq!(slot, PlayerSlot::Player1, "P1 was the liar");
        },
        other => panic!("expected ReadyHashMismatch, got {other:?}"),
    }
}

/// Player1 sends Resync on the first Advance → Match recovers via
/// FullState → SyncOk and the match continues normally.
#[tokio::test]
async fn resync_recovery_after_advance_continues_match() {
    let game = make_game();
    let expected_hash = game.state_hash();
    let cfg = build_match_config(&game, TimingMode::Wait, 500, 1000);

    let (p1_player, p1_handle) = ScriptedPlayer::pair(identity(PlayerSlot::Player1, "p1"));
    let (p2_player, p2_handle) = ScriptedPlayer::pair(identity(PlayerSlot::Player2, "p2"));

    let p1: Box<dyn Player> = Box::new(p1_player);
    let p2: Box<dyn Player> = Box::new(p2_player);

    let m = Match::new(
        game,
        [p1, p2],
        cfg,
        [vec![], vec![]],
        fast_setup(),
        fast_playing(),
        None,
    );

    let resync_seen = Arc::new(Mutex::new(false));
    let full_state_seen = Arc::new(Mutex::new(false));
    let resync_seen_clone = resync_seen.clone();
    let full_state_seen_clone = full_state_seen.clone();

    // Drive both bots through the protocol. P1 sends Resync on the first
    // Advance, expects FullState back, then SyncOk.
    let driver = tokio::spawn(async move {
        let mut p1 = p1_handle;
        let mut p2 = p2_handle;

        // Setup: Configure → Ready
        let _ = p1.expect_send().await;
        let _ = p2.expect_send().await;
        p1.respond(BotMsg::Ready {
            state_hash: expected_hash,
        });
        p2.respond(BotMsg::Ready {
            state_hash: expected_hash,
        });

        // GoPreprocess → PreprocessingDone
        let _ = p1.expect_send().await;
        let _ = p2.expect_send().await;
        p1.respond(BotMsg::PreprocessingDone);
        p2.respond(BotMsg::PreprocessingDone);

        // Turn 0: Go → Action(Stay)
        let go1 = p1.expect_send().await;
        let go2 = p2.expect_send().await;
        let go_hash = match go1 {
            HostMsg::Go { state_hash, .. } => state_hash,
            other => panic!("p1 expected Go, got {other:?}"),
        };
        assert!(matches!(go2, HostMsg::Go { .. }));
        p1.respond(BotMsg::Action {
            direction: Direction::Stay,
            player: PlayerSlot::Player1,
            turn: 0,
            state_hash: go_hash,
            think_ms: 1,
        });
        p2.respond(BotMsg::Action {
            direction: Direction::Stay,
            player: PlayerSlot::Player2,
            turn: 0,
            state_hash: go_hash,
            think_ms: 1,
        });

        // Turn 1: Advance → P1 Resyncs, P2 SyncOk.
        let advance1 = p1.expect_send().await;
        let advance2 = p2.expect_send().await;
        let new_hash = match advance1 {
            HostMsg::Advance { new_hash, .. } => new_hash,
            other => panic!("p1 expected Advance, got {other:?}"),
        };
        assert!(matches!(advance2, HostMsg::Advance { .. }));

        // P1 sends Resync; P2 sends SyncOk.
        p1.respond(BotMsg::Resync { my_hash: 0 });
        p2.respond(BotMsg::SyncOk { hash: new_hash });

        // Match should respond to P1 with FullState.
        let full = p1.expect_send().await;
        match full {
            HostMsg::FullState { .. } => {
                *full_state_seen_clone.lock().unwrap() = true;
            },
            other => panic!("p1 expected FullState, got {other:?}"),
        }
        *resync_seen_clone.lock().unwrap() = true;
        // P1 then sends SyncOk.
        p1.respond(BotMsg::SyncOk { hash: new_hash });

        // Drive remaining turns to completion (turns 1..5). The hash advances
        // each turn; track it as `current_hash`.
        let mut current_hash = new_hash;
        for t in 1..5_u16 {
            let _ = p1.expect_send().await; // Go
            let _ = p2.expect_send().await; // Go
            p1.respond(BotMsg::Action {
                direction: Direction::Stay,
                player: PlayerSlot::Player1,
                turn: t,
                state_hash: current_hash,
                think_ms: 1,
            });
            p2.respond(BotMsg::Action {
                direction: Direction::Stay,
                player: PlayerSlot::Player2,
                turn: t,
                state_hash: current_hash,
                think_ms: 1,
            });
            // For all turns except the last, respond to Advance with SyncOk.
            // For the last turn (t == 4), the match sends GameOver, no Advance.
            if t < 4 {
                let adv1 = p1.expect_send().await;
                let adv2 = p2.expect_send().await;
                let nh = match adv1 {
                    HostMsg::Advance { new_hash, .. } => new_hash,
                    other => panic!("expected Advance, got {other:?}"),
                };
                assert!(matches!(adv2, HostMsg::Advance { .. }));
                p1.respond(BotMsg::SyncOk { hash: nh });
                p2.respond(BotMsg::SyncOk { hash: nh });
                current_hash = nh;
            }
        }
        // GameOver
        let go1 = p1.expect_send().await;
        let go2 = p2.expect_send().await;
        assert!(matches!(go1, HostMsg::GameOver { .. }));
        assert!(matches!(go2, HostMsg::GameOver { .. }));
    });

    let result = timeout(Duration::from_secs(5), m.run())
        .await
        .expect("match run hung")
        .expect("match run failed");

    driver.await.unwrap();

    assert!(*resync_seen.lock().unwrap(), "Resync was processed");
    assert!(
        *full_state_seen.lock().unwrap(),
        "FullState was sent in response"
    );
    assert_eq!(result.turns_played, 5);
}

/// Two Resyncs in a row from the same player on the same turn →
/// `MatchError::PersistentDesync`.
#[tokio::test]
async fn second_resync_on_same_turn_is_persistent_desync() {
    let game = make_game();
    let expected_hash = game.state_hash();
    let cfg = build_match_config(&game, TimingMode::Wait, 500, 1000);

    let (p1_player, p1_handle) = ScriptedPlayer::pair(identity(PlayerSlot::Player1, "p1"));
    let (p2_player, p2_handle) = ScriptedPlayer::pair(identity(PlayerSlot::Player2, "p2"));

    let p1: Box<dyn Player> = Box::new(p1_player);
    let p2: Box<dyn Player> = Box::new(p2_player);

    let m = Match::new(
        game,
        [p1, p2],
        cfg,
        [vec![], vec![]],
        fast_setup(),
        fast_playing(),
        None,
    );

    let driver = tokio::spawn(async move {
        let mut p1 = p1_handle;
        let mut p2 = p2_handle;

        let _ = p1.expect_send().await;
        let _ = p2.expect_send().await;
        p1.respond(BotMsg::Ready {
            state_hash: expected_hash,
        });
        p2.respond(BotMsg::Ready {
            state_hash: expected_hash,
        });

        let _ = p1.expect_send().await; // GoPreprocess
        let _ = p2.expect_send().await;
        p1.respond(BotMsg::PreprocessingDone);
        p2.respond(BotMsg::PreprocessingDone);

        // Turn 0: Go → Action.
        let go1 = p1.expect_send().await;
        let _go2 = p2.expect_send().await;
        let go_hash = match go1 {
            HostMsg::Go { state_hash, .. } => state_hash,
            _ => unreachable!(),
        };
        p1.respond(BotMsg::Action {
            direction: Direction::Stay,
            player: PlayerSlot::Player1,
            turn: 0,
            state_hash: go_hash,
            think_ms: 1,
        });
        p2.respond(BotMsg::Action {
            direction: Direction::Stay,
            player: PlayerSlot::Player2,
            turn: 0,
            state_hash: go_hash,
            think_ms: 1,
        });

        // Turn 1: P1 Resyncs twice in a row.
        let _ = p1.expect_send().await; // Advance
        let _ = p2.expect_send().await;
        p1.respond(BotMsg::Resync { my_hash: 0 });
        // P2 stays cooperative so we exit the SyncOk loop on P1 first.
        // Actually: match awaits SyncOk for slot 0 first (resync_with_retry
        // is per-slot in order). So we don't need to respond to P2 yet.
        let _full = p1.expect_send().await; // FullState
                                            // Second Resync on same turn → PersistentDesync.
        p1.respond(BotMsg::Resync { my_hash: 0 });
    });

    let err = timeout(Duration::from_secs(3), m.run())
        .await
        .expect("run hung")
        .expect_err("expected PersistentDesync");

    driver.abort();
    let _ = driver.await;

    match err {
        MatchError::PersistentDesync(slot) => {
            assert_eq!(slot, PlayerSlot::Player1);
        },
        other => panic!("expected PersistentDesync, got {other:?}"),
    }
}

// ── FaultPolicy seam (slice 7) ────────────────────────

/// Drive the Configure → Ready → GoPreprocess → PreprocessingDone setup
/// for two scripted players whose initial hash should match `expected_hash`.
async fn drive_setup(p1: &mut ScriptedHandle, p2: &mut ScriptedHandle, expected_hash: u64) {
    let _ = p1.expect_send().await; // Configure
    let _ = p2.expect_send().await;
    p1.respond(BotMsg::Ready {
        state_hash: expected_hash,
    });
    p2.respond(BotMsg::Ready {
        state_hash: expected_hash,
    });
    let _ = p1.expect_send().await; // GoPreprocess
    let _ = p2.expect_send().await;
    p1.respond(BotMsg::PreprocessingDone);
    p2.respond(BotMsg::PreprocessingDone);
}

fn tight_playing(
    fault_policy: Option<std::sync::Arc<dyn pyrat_host::match_host::FaultPolicy>>,
) -> PlayingConfig {
    let mut cfg = PlayingConfig {
        move_timeout: Duration::from_millis(80),
        network_grace: Duration::from_millis(30),
        ..Default::default()
    };
    if let Some(p) = fault_policy {
        cfg.fault_policy = p;
    }
    cfg
}

/// P2 stalls on Go but has a stored Provisional. Default policy resolves the
/// turn with the provisional direction; match completes; `BotTimeout` event
/// fires for the timed-out slot.
#[tokio::test]
async fn default_policy_uses_provisional_on_timeout() {
    let game = make_game_one_turn();
    let expected_hash = game.state_hash();
    let cfg = build_match_config(&game, TimingMode::Wait, 80, 1000);

    let (event_tx, mut event_rx) = mpsc::unbounded_channel::<MatchEvent>();
    let (p1_player, p1_handle) = ScriptedPlayer::pair(identity(PlayerSlot::Player1, "p1"));
    let (p2_player, p2_handle) = ScriptedPlayer::pair_with_provisional(
        identity(PlayerSlot::Player2, "p2"),
        Some(Direction::Right),
    );

    let m = Match::new(
        game,
        [Box::new(p1_player), Box::new(p2_player)],
        cfg,
        [vec![], vec![]],
        fast_setup(),
        tight_playing(None),
        Some(event_tx),
    );

    let driver = tokio::spawn(async move {
        let mut p1 = p1_handle;
        let mut p2 = p2_handle;
        drive_setup(&mut p1, &mut p2, expected_hash).await;
        // Turn 0: both Go received, only P1 commits.
        let go1 = p1.expect_send().await;
        let _go2 = p2.expect_send().await;
        let go_hash = match go1 {
            HostMsg::Go { state_hash, .. } => state_hash,
            other => panic!("expected Go, got {other:?}"),
        };
        p1.respond(BotMsg::Action {
            direction: Direction::Stay,
            player: PlayerSlot::Player1,
            turn: 0,
            state_hash: go_hash,
            think_ms: 1,
        });
        // P2 doesn't respond → Match deadline → Stop → grace → TimedOut.
        // Hold handles alive so P2's recv sees no message (not a clean close).
        std::future::pending::<()>().await;
    });

    let result = timeout(Duration::from_secs(3), m.run())
        .await
        .expect("match run hung")
        .expect("match run failed");

    driver.abort();
    let _ = driver.await;

    let mut events = vec![];
    while let Ok(ev) = event_rx.try_recv() {
        events.push(ev);
    }
    let timeouts: Vec<_> = events
        .iter()
        .filter_map(|e| match e {
            MatchEvent::BotTimeout { player, turn } => Some((*player, *turn)),
            _ => None,
        })
        .collect();
    assert_eq!(timeouts, vec![(PlayerSlot::Player2, 0)]);

    let actions = events
        .iter()
        .find_map(|e| match e {
            MatchEvent::TurnPlayed {
                p1_action,
                p2_action,
                ..
            } => Some((*p1_action, *p2_action)),
            _ => None,
        })
        .expect("expected TurnPlayed");
    assert_eq!(actions, (Direction::Stay, Direction::Right));
    assert_eq!(result.turns_played, 1);
}

/// P2 stalls on Go with no stored Provisional. Default policy falls back to
/// `Direction::Stay`; match completes; `BotTimeout` event fires.
#[tokio::test]
async fn default_policy_falls_back_to_stay_when_no_provisional() {
    let game = make_game_one_turn();
    let expected_hash = game.state_hash();
    let cfg = build_match_config(&game, TimingMode::Wait, 80, 1000);

    let (event_tx, mut event_rx) = mpsc::unbounded_channel::<MatchEvent>();
    let (p1_player, p1_handle) = ScriptedPlayer::pair(identity(PlayerSlot::Player1, "p1"));
    let (p2_player, p2_handle) = ScriptedPlayer::pair(identity(PlayerSlot::Player2, "p2"));

    let m = Match::new(
        game,
        [Box::new(p1_player), Box::new(p2_player)],
        cfg,
        [vec![], vec![]],
        fast_setup(),
        tight_playing(None),
        Some(event_tx),
    );

    let driver = tokio::spawn(async move {
        let mut p1 = p1_handle;
        let mut p2 = p2_handle;
        drive_setup(&mut p1, &mut p2, expected_hash).await;
        let go1 = p1.expect_send().await;
        let _go2 = p2.expect_send().await;
        let go_hash = match go1 {
            HostMsg::Go { state_hash, .. } => state_hash,
            other => panic!("expected Go, got {other:?}"),
        };
        p1.respond(BotMsg::Action {
            direction: Direction::Stay,
            player: PlayerSlot::Player1,
            turn: 0,
            state_hash: go_hash,
            think_ms: 1,
        });
        std::future::pending::<()>().await;
    });

    let result = timeout(Duration::from_secs(3), m.run())
        .await
        .expect("match run hung")
        .expect("match run failed");

    driver.abort();
    let _ = driver.await;

    let mut events = vec![];
    while let Ok(ev) = event_rx.try_recv() {
        events.push(ev);
    }
    assert!(events.iter().any(|e| matches!(
        e,
        MatchEvent::BotTimeout {
            player: PlayerSlot::Player2,
            turn: 0
        }
    )));

    let actions = events
        .iter()
        .find_map(|e| match e {
            MatchEvent::TurnPlayed {
                p1_action,
                p2_action,
                ..
            } => Some((*p1_action, *p2_action)),
            _ => None,
        })
        .expect("expected TurnPlayed");
    assert_eq!(actions, (Direction::Stay, Direction::Stay));
    assert_eq!(result.turns_played, 1);
}

/// Strict policy: P2 stalls → `MatchError::ActionTimeout(Player2)`. Match
/// aborts mid-turn; `BotTimeout` event still fires (observable protocol fact).
#[tokio::test]
async fn strict_policy_escalates_timeout_to_action_timeout() {
    let game = make_game_one_turn();
    let expected_hash = game.state_hash();
    let cfg = build_match_config(&game, TimingMode::Wait, 80, 1000);

    let (event_tx, mut event_rx) = mpsc::unbounded_channel::<MatchEvent>();
    let (p1_player, p1_handle) = ScriptedPlayer::pair(identity(PlayerSlot::Player1, "p1"));
    let (p2_player, p2_handle) = ScriptedPlayer::pair_with_provisional(
        identity(PlayerSlot::Player2, "p2"),
        Some(Direction::Right),
    );

    let m = Match::new(
        game,
        [Box::new(p1_player), Box::new(p2_player)],
        cfg,
        [vec![], vec![]],
        fast_setup(),
        tight_playing(Some(std::sync::Arc::new(StrictFaultPolicy))),
        Some(event_tx),
    );

    let driver = tokio::spawn(async move {
        let mut p1 = p1_handle;
        let mut p2 = p2_handle;
        drive_setup(&mut p1, &mut p2, expected_hash).await;
        let go1 = p1.expect_send().await;
        let _go2 = p2.expect_send().await;
        let go_hash = match go1 {
            HostMsg::Go { state_hash, .. } => state_hash,
            other => panic!("expected Go, got {other:?}"),
        };
        p1.respond(BotMsg::Action {
            direction: Direction::Stay,
            player: PlayerSlot::Player1,
            turn: 0,
            state_hash: go_hash,
            think_ms: 1,
        });
        std::future::pending::<()>().await;
    });

    let err = timeout(Duration::from_secs(3), m.run())
        .await
        .expect("match run hung")
        .expect_err("expected ActionTimeout");

    driver.abort();
    let _ = driver.await;

    match err {
        MatchError::ActionTimeout(slot) => assert_eq!(slot, PlayerSlot::Player2),
        other => panic!("expected ActionTimeout, got {other:?}"),
    }

    let mut events = vec![];
    while let Ok(ev) = event_rx.try_recv() {
        events.push(ev);
    }
    assert!(events.iter().any(|e| matches!(
        e,
        MatchEvent::BotTimeout {
            player: PlayerSlot::Player2,
            turn: 0
        }
    )));
    // No TurnPlayed — match aborted before applying.
    assert!(!events
        .iter()
        .any(|e| matches!(e, MatchEvent::TurnPlayed { .. })));
}

// ── Analysis sub-states (slice 7 part A) ──────────────

/// Drive a 5-turn match through the analysis-mode typestate
/// (`start_turn → stop_and_collect → advance`) instead of `step`/`run`.
/// Both paths should produce the same end state.
#[tokio::test]
async fn analysis_lifecycle_completes_match() {
    let game = make_game();
    let cfg = build_match_config(&game, TimingMode::Wait, 200, 500);

    let (event_tx, mut event_rx) = mpsc::unbounded_channel::<MatchEvent>();
    let sink = EventSink::new(event_tx.clone());
    let p1 = Box::new(
        EmbeddedPlayer::accept(StayBot, identity(PlayerSlot::Player1, "p1"), sink.clone())
            .await
            .expect("accept p1"),
    ) as Box<dyn Player>;
    let p2 = Box::new(
        EmbeddedPlayer::accept(StayBot, identity(PlayerSlot::Player2, "p2"), sink)
            .await
            .expect("accept p2"),
    ) as Box<dyn Player>;

    let m = Match::new(
        game,
        [p1, p2],
        cfg,
        [vec![], vec![]],
        fast_setup(),
        fast_playing(),
        Some(event_tx.clone()),
    );

    let ready = timeout(Duration::from_secs(3), m.setup())
        .await
        .expect("setup hung")
        .expect("setup failed");
    let mut playing = ready.start();

    let result = loop {
        let thinking = playing.start_turn().await.expect("start_turn");
        let collected = thinking.stop_and_collect().await.expect("stop_and_collect");
        match collected.advance().await.expect("advance") {
            StepResult::Continue(next) => playing = next,
            StepResult::GameOver(finished) => break finished.finalize().await,
        }
    };

    assert_eq!(result.result, GameResult::Draw);
    assert_eq!(result.turns_played, 5);

    drop(event_tx);
    let mut events = vec![];
    while let Some(ev) = event_rx.recv().await {
        events.push(ev);
    }
    let n_turns = events
        .iter()
        .filter(|e| matches!(e, MatchEvent::TurnPlayed { .. }))
        .count();
    assert_eq!(n_turns, 5, "expected 5 TurnPlayed events");
    assert!(events
        .iter()
        .any(|e| matches!(e, MatchEvent::MatchOver { .. })));
}

/// `start_turn_with(ts)` rebuilds `self.game` from the snapshot (F4) and
/// sends `GoState`. The host-side hash must match what bots compute from
/// the same `(MatchConfig, TurnState)`, so bots accept the GoState and the
/// turn proceeds normally to a `Continue`.
#[tokio::test]
async fn start_turn_with_injects_state_and_continues() {
    let game = make_game();
    let cfg = build_match_config(&game, TimingMode::Wait, 200, 500);

    let (event_tx, mut event_rx) = mpsc::unbounded_channel::<MatchEvent>();
    let sink = EventSink::new(event_tx.clone());
    let p1 = Box::new(
        EmbeddedPlayer::accept(StayBot, identity(PlayerSlot::Player1, "p1"), sink.clone())
            .await
            .expect("accept p1"),
    ) as Box<dyn Player>;
    let p2 = Box::new(
        EmbeddedPlayer::accept(StayBot, identity(PlayerSlot::Player2, "p2"), sink)
            .await
            .expect("accept p2"),
    ) as Box<dyn Player>;

    let m = Match::new(
        game,
        [p1, p2],
        cfg,
        [vec![], vec![]],
        fast_setup(),
        fast_playing(),
        Some(event_tx.clone()),
    );

    let ready = timeout(Duration::from_secs(3), m.setup())
        .await
        .expect("setup hung")
        .expect("setup failed");
    let playing = ready.start();

    // Inject: turn 2, players moved off corners, cheese still at center.
    let injected = pyrat_protocol::TurnState {
        turn: 2,
        player1_position: Coordinates::new(1, 1),
        player2_position: Coordinates::new(3, 3),
        player1_score: 0.0,
        player2_score: 0.0,
        player1_mud_turns: 0,
        player2_mud_turns: 0,
        cheese: vec![Coordinates::new(2, 2)],
        player1_last_move: Direction::Up,
        player2_last_move: Direction::Down,
    };

    let thinking = playing
        .start_turn_with(injected)
        .await
        .expect("start_turn_with");

    // Host's engine reflects the injected state.
    assert_eq!(thinking.game().turn, 2);
    assert_eq!(thinking.game().player1.current_pos, Coordinates::new(1, 1));
    assert_eq!(thinking.game().player2.current_pos, Coordinates::new(3, 3));

    // Bots accept GoState (host hash = bot hash from same rebuild path),
    // commit Stay quickly. stop_and_collect picks them up.
    let collected = timeout(Duration::from_secs(2), thinking.stop_and_collect())
        .await
        .expect("stop_and_collect hung")
        .expect("stop_and_collect");

    use pyrat_host::match_host::ActionOutcome;
    for outcome in collected.outcomes() {
        assert!(
            matches!(
                outcome,
                ActionOutcome::Committed {
                    direction: Direction::Stay,
                    ..
                }
            ),
            "expected Committed(Stay), got {outcome:?}"
        );
    }

    // Advance once: turn 2 → turn 3, no cheese, max_turns=5 ⇒ Continue.
    let next = collected.advance().await.expect("advance");
    let playing = match next {
        StepResult::Continue(p) => p,
        StepResult::GameOver(_) => panic!("unexpected GameOver at injected turn 2"),
    };
    assert_eq!(playing.game().turn, 3);
    assert_eq!(playing.game().player1.current_pos, Coordinates::new(1, 1));

    drop(playing); // dispatchers detach; harmless for the test runtime
    drop(event_tx);
    while event_rx.recv().await.is_some() {}
}

/// `advance_with(p1, p2)` overrides the bots' committed actions. Used by
/// GUI analysis-mode "play this move instead" navigation. The override
/// directions show up in `TurnPlayed`; `think_ms` reports as 0 since the
/// bots' computation isn't being used.
#[tokio::test]
async fn advance_with_override_replaces_committed_actions() {
    let game = make_game_one_turn();
    let cfg = build_match_config(&game, TimingMode::Wait, 200, 500);

    let (event_tx, mut event_rx) = mpsc::unbounded_channel::<MatchEvent>();
    let sink = EventSink::new(event_tx.clone());
    let p1 = Box::new(
        EmbeddedPlayer::accept(StayBot, identity(PlayerSlot::Player1, "p1"), sink.clone())
            .await
            .expect("accept p1"),
    ) as Box<dyn Player>;
    let p2 = Box::new(
        EmbeddedPlayer::accept(StayBot, identity(PlayerSlot::Player2, "p2"), sink)
            .await
            .expect("accept p2"),
    ) as Box<dyn Player>;

    let m = Match::new(
        game,
        [p1, p2],
        cfg,
        [vec![], vec![]],
        fast_setup(),
        fast_playing(),
        Some(event_tx.clone()),
    );

    let ready = timeout(Duration::from_secs(3), m.setup())
        .await
        .expect("setup hung")
        .expect("setup failed");
    let playing = ready.start();

    let thinking = playing.start_turn().await.expect("start_turn");
    let collected = timeout(Duration::from_secs(2), thinking.stop_and_collect())
        .await
        .expect("stop_and_collect hung")
        .expect("stop_and_collect");

    // Bots committed Stay; we override.
    let next = collected
        .advance_with(Direction::Up, Direction::Down)
        .await
        .expect("advance_with");

    // max_turns=1 ⇒ GameOver.
    let result = match next {
        StepResult::GameOver(finished) => finished.finalize().await,
        StepResult::Continue(_) => panic!("expected GameOver at max_turns=1"),
    };
    assert_eq!(result.turns_played, 1);

    drop(event_tx);
    let mut events = vec![];
    while let Some(ev) = event_rx.recv().await {
        events.push(ev);
    }
    let played = events
        .iter()
        .find_map(|e| match e {
            MatchEvent::TurnPlayed {
                p1_action,
                p2_action,
                p1_think_ms,
                p2_think_ms,
                ..
            } => Some((*p1_action, *p2_action, *p1_think_ms, *p2_think_ms)),
            _ => None,
        })
        .expect("expected TurnPlayed");
    assert_eq!(
        played,
        (Direction::Up, Direction::Down, 0, 0),
        "advance_with overrides directions and zeroes think_ms"
    );
}
