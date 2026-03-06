//! Full-flow orchestration tests using in-process mock bots (duplex pairs).
//!
//! Tests the host library's setup + playing pipeline end-to-end without
//! subprocesses or TCP.

mod common;

use std::collections::HashMap;
use std::time::Duration;

use tokio::sync::mpsc;
use tokio::time::timeout;

use pyrat_host::game_loop::{
    run_playing, run_setup, MatchEvent, MatchSetup, PlayerEntry, PlayingConfig,
};
use pyrat_host::session::SessionId;
use pyrat_host::wire::*;

use common::*;

// ── Helpers ──────────────────────────────────────────

async fn read_turn_state(
    reader: &mut pyrat_host::wire::framing::FrameReader<
        tokio::io::ReadHalf<tokio::io::DuplexStream>,
    >,
) -> u16 {
    let frame = timeout(Duration::from_secs(2), reader.read_frame())
        .await
        .expect("timed out waiting for TurnState")
        .unwrap();
    let packet = flatbuffers::root::<HostPacket>(frame).unwrap();
    assert_eq!(packet.message_type(), HostMessage::TurnState);
    packet.message_as_turn_state().unwrap().turn()
}

async fn read_game_over(
    reader: &mut pyrat_host::wire::framing::FrameReader<
        tokio::io::ReadHalf<tokio::io::DuplexStream>,
    >,
) -> GameResult {
    let frame = timeout(Duration::from_secs(2), reader.read_frame())
        .await
        .expect("timed out waiting for GameOver")
        .unwrap();
    let packet = flatbuffers::root::<HostPacket>(frame).unwrap();
    assert_eq!(packet.message_type(), HostMessage::GameOver);
    packet.message_as_game_over().unwrap().result()
}

// ── Tests ────────────────────────────────────────────

/// Full flow: both bots connect, identify, ready, preprocess, play, game over.
/// Events are collected and verified.
#[tokio::test]
async fn full_flow_with_events() {
    let (game_tx, mut game_rx) = mpsc::channel(64);
    let (mut w1, mut r1, h1) = spawn_session(SessionId(1), game_tx.clone());
    let (mut w2, mut r2, h2) = spawn_session(SessionId(2), game_tx.clone());
    drop(game_tx);

    let (event_tx, mut event_rx) = mpsc::unbounded_channel::<MatchEvent>();

    let setup = MatchSetup {
        players: vec![
            PlayerEntry {
                player: Player::Player1,
                agent_id: "bot-a".into(),
            },
            PlayerEntry {
                player: Player::Player2,
                agent_id: "bot-b".into(),
            },
        ],
        match_config: simple_match_config(),
        bot_options: HashMap::new(),
        timing: fast_timing(),
    };

    // Setup phase — use a clone for setup so we can move the original into playing.
    let setup_event_tx = event_tx.clone();
    let setup_ref = &setup;
    let setup_tx_ref = &setup_event_tx;
    let (_, setup_result) = tokio::join!(
        async {
            tokio::join!(
                drive_bot_through_setup(&mut w1, &mut r1, "BotA", "AuthA", "bot-a"),
                drive_bot_through_setup(&mut w2, &mut r2, "BotB", "AuthB", "bot-b"),
            );
        },
        async {
            run_setup(setup_ref, &mut game_rx, Some(setup_tx_ref))
                .await
                .expect("setup failed")
        },
    );
    drop(setup_event_tx);

    let sessions = setup_result.sessions;

    // Playing phase: max 3 turns, both STAY → Draw.
    let mut game = tiny_game(3);
    let config = PlayingConfig {
        move_timeout: Duration::from_millis(500),
    };

    let play_task = tokio::spawn(async move {
        run_playing(&mut game, &sessions, &mut game_rx, &config, Some(&event_tx)).await
    });

    for _ in 0..3 {
        let _ = read_turn_state(&mut r1).await;
        let _ = read_turn_state(&mut r2).await;
        w1.write_frame(&action_frame(Direction::Stay, Player::Player1))
            .await
            .unwrap();
        w2.write_frame(&action_frame(Direction::Stay, Player::Player2))
            .await
            .unwrap();
    }

    let _ = read_game_over(&mut r1).await;
    let _ = read_game_over(&mut r2).await;

    let result = timeout(Duration::from_secs(5), play_task)
        .await
        .expect("play timed out")
        .expect("play panicked")
        .expect("play returned error");

    assert_eq!(result.result, GameResult::Draw);
    assert_eq!(result.turns_played, 3);

    // Collect events — event_tx was moved into play_task, which is done.
    let mut events = Vec::new();
    while let Ok(event) = event_rx.try_recv() {
        events.push(event);
    }

    // Verify BotIdentified payloads
    let identified: Vec<_> = events
        .iter()
        .filter_map(|e| match e {
            MatchEvent::BotIdentified { player, name, .. } => Some((*player, name.as_str())),
            _ => None,
        })
        .collect();
    assert_eq!(identified.len(), 2, "expected 2 BotIdentified events");
    assert!(
        identified.contains(&(Player::Player1, "BotA")),
        "expected Player1→BotA, got {identified:?}"
    );
    assert!(
        identified.contains(&(Player::Player2, "BotB")),
        "expected Player2→BotB, got {identified:?}"
    );

    // Verify SetupComplete
    assert!(
        events
            .iter()
            .any(|e| matches!(e, MatchEvent::SetupComplete)),
        "expected SetupComplete event"
    );

    // Verify TurnPlayed turn numbers are sequential
    let turn_numbers: Vec<u16> = events
        .iter()
        .filter_map(|e| match e {
            MatchEvent::TurnPlayed { state, .. } => Some(state.turn),
            _ => None,
        })
        .collect();
    assert_eq!(
        turn_numbers,
        vec![1, 2, 3],
        "expected sequential turns [1, 2, 3]"
    );

    // Verify MatchOver payload
    let match_over = events
        .iter()
        .find_map(|e| match e {
            MatchEvent::MatchOver { result } => Some(result),
            _ => None,
        })
        .expect("expected MatchOver event");
    assert_eq!(match_over.result, GameResult::Draw);
    assert_eq!(match_over.turns_played, 3);

    // Verify event ordering:
    // BotIdentified < SetupComplete < MatchStarted < TurnPlayed < MatchOver (last)
    let last_identified_pos = events
        .iter()
        .rposition(|e| matches!(e, MatchEvent::BotIdentified { .. }))
        .expect("expected BotIdentified events");
    let setup_pos = events
        .iter()
        .position(|e| matches!(e, MatchEvent::SetupComplete))
        .unwrap();
    let match_started_pos = events
        .iter()
        .position(|e| matches!(e, MatchEvent::MatchStarted { .. }))
        .expect("expected MatchStarted event");
    let first_turn_pos = events
        .iter()
        .position(|e| matches!(e, MatchEvent::TurnPlayed { .. }))
        .unwrap();
    let match_over_pos = events
        .iter()
        .position(|e| matches!(e, MatchEvent::MatchOver { .. }))
        .unwrap();
    assert!(
        last_identified_pos < setup_pos,
        "all BotIdentified events should precede SetupComplete"
    );
    assert_eq!(
        match_started_pos,
        setup_pos + 1,
        "MatchStarted should immediately follow SetupComplete"
    );
    assert!(
        match_started_pos < first_turn_pos,
        "MatchStarted should precede first TurnPlayed"
    );
    assert_eq!(
        match_over_pos,
        events.len() - 1,
        "MatchOver should be the last event"
    );

    drop(w1);
    drop(r1);
    drop(w2);
    drop(r2);
    let _ = h1.await;
    let _ = h2.await;
}

/// Bot sends Info during play — BotInfo event is emitted.
#[tokio::test]
async fn info_forwarded_as_event() {
    let (game_tx, mut game_rx) = mpsc::channel(64);
    let (mut w1, mut r1, h1) = spawn_session(SessionId(1), game_tx.clone());
    let (mut w2, mut r2, h2) = spawn_session(SessionId(2), game_tx.clone());
    drop(game_tx);

    let (event_tx, mut event_rx) = mpsc::unbounded_channel::<MatchEvent>();

    let setup = MatchSetup {
        players: vec![
            PlayerEntry {
                player: Player::Player1,
                agent_id: "bot-a".into(),
            },
            PlayerEntry {
                player: Player::Player2,
                agent_id: "bot-b".into(),
            },
        ],
        match_config: simple_match_config(),
        bot_options: HashMap::new(),
        timing: fast_timing(),
    };

    let setup_event_tx = event_tx.clone();
    let setup_ref = &setup;
    let setup_tx_ref = &setup_event_tx;
    let (_, setup_result) = tokio::join!(
        async {
            tokio::join!(
                drive_bot_through_setup(&mut w1, &mut r1, "BotA", "AuthA", "bot-a"),
                drive_bot_through_setup(&mut w2, &mut r2, "BotB", "AuthB", "bot-b"),
            );
        },
        async {
            run_setup(setup_ref, &mut game_rx, Some(setup_tx_ref))
                .await
                .expect("setup failed")
        },
    );
    drop(setup_event_tx);

    let sessions = setup_result.sessions;
    let mut game = tiny_game(1);
    let config = PlayingConfig {
        move_timeout: Duration::from_millis(500),
    };

    let play_task = tokio::spawn(async move {
        run_playing(&mut game, &sessions, &mut game_rx, &config, Some(&event_tx)).await
    });

    let _ = read_turn_state(&mut r1).await;
    let _ = read_turn_state(&mut r2).await;

    // Send Info from bot 1 before sending action
    let info_frame = build_bot_frame(BotMessage::Info, |fbb| {
        let msg = fbb.create_string("test info");
        Info::create(
            fbb,
            &InfoArgs {
                player: Player::Player1,
                multipv: 1,
                target: None,
                depth: 5,
                nodes: 100,
                score: 0.5,
                pv: None,
                message: Some(msg),
            },
        )
        .as_union_value()
    });
    w1.write_frame(&info_frame).await.unwrap();

    // Now send actions
    w1.write_frame(&action_frame(Direction::Stay, Player::Player1))
        .await
        .unwrap();
    w2.write_frame(&action_frame(Direction::Stay, Player::Player2))
        .await
        .unwrap();

    let _ = read_game_over(&mut r1).await;
    let _ = read_game_over(&mut r2).await;

    let _ = timeout(Duration::from_secs(5), play_task)
        .await
        .expect("play timed out")
        .expect("play panicked")
        .expect("play returned error");

    let mut events = Vec::new();
    while let Ok(event) = event_rx.try_recv() {
        events.push(event);
    }

    let info_event = events
        .iter()
        .find_map(|e| match e {
            MatchEvent::BotInfo { player, info, .. } => Some((*player, info)),
            _ => None,
        })
        .expect("expected at least one BotInfo event");
    assert_eq!(
        info_event.0,
        Player::Player1,
        "info should come from Player1"
    );
    assert_eq!(info_event.1.message, "test info");
    assert_eq!(info_event.1.depth, 5);
    assert_eq!(info_event.1.nodes, 100);

    drop(w1);
    drop(r1);
    drop(w2);
    drop(r2);
    let _ = h1.await;
    let _ = h2.await;
}

/// Bot times out → BotTimeout event emitted.
#[tokio::test]
async fn timeout_emits_event() {
    let (game_tx, mut game_rx) = mpsc::channel(64);
    let (mut w1, mut r1, h1) = spawn_session(SessionId(1), game_tx.clone());
    let (mut w2, mut r2, h2) = spawn_session(SessionId(2), game_tx.clone());
    drop(game_tx);

    let (event_tx, mut event_rx) = mpsc::unbounded_channel::<MatchEvent>();

    let setup = MatchSetup {
        players: vec![
            PlayerEntry {
                player: Player::Player1,
                agent_id: "bot-a".into(),
            },
            PlayerEntry {
                player: Player::Player2,
                agent_id: "bot-b".into(),
            },
        ],
        match_config: simple_match_config(),
        bot_options: HashMap::new(),
        timing: fast_timing(),
    };

    let setup_event_tx = event_tx.clone();
    let setup_ref = &setup;
    let setup_tx_ref = &setup_event_tx;
    let (_, setup_result) = tokio::join!(
        async {
            tokio::join!(
                drive_bot_through_setup(&mut w1, &mut r1, "BotA", "AuthA", "bot-a"),
                drive_bot_through_setup(&mut w2, &mut r2, "BotB", "AuthB", "bot-b"),
            );
        },
        async {
            run_setup(setup_ref, &mut game_rx, Some(setup_tx_ref))
                .await
                .expect("setup failed")
        },
    );
    drop(setup_event_tx);

    let sessions = setup_result.sessions;
    let mut game = tiny_game(1);
    let config = PlayingConfig {
        move_timeout: Duration::from_millis(100),
    };

    let play_task = tokio::spawn(async move {
        run_playing(&mut game, &sessions, &mut game_rx, &config, Some(&event_tx)).await
    });

    // Turn 0: P1 responds, P2 silent → timeout
    let _ = read_turn_state(&mut r1).await;
    let _ = read_turn_state(&mut r2).await;
    w1.write_frame(&action_frame(Direction::Stay, Player::Player1))
        .await
        .unwrap();
    // P2 silent

    // Consume Timeout message on r2
    let frame = timeout(Duration::from_secs(2), r2.read_frame())
        .await
        .expect("timed out waiting for Timeout")
        .unwrap();
    let packet = flatbuffers::root::<HostPacket>(frame).unwrap();
    assert_eq!(packet.message_type(), HostMessage::Timeout);

    let _ = read_game_over(&mut r1).await;
    let _ = read_game_over(&mut r2).await;

    let _ = timeout(Duration::from_secs(5), play_task)
        .await
        .expect("play timed out")
        .expect("play panicked")
        .expect("play returned error");

    let mut events = Vec::new();
    while let Ok(event) = event_rx.try_recv() {
        events.push(event);
    }

    let (timeout_player, timeout_turn) = events
        .iter()
        .find_map(|e| match e {
            MatchEvent::BotTimeout { player, turn } => Some((*player, *turn)),
            _ => None,
        })
        .expect("expected at least one BotTimeout event");
    assert_eq!(
        timeout_player,
        Player::Player2,
        "Player2 was silent and should have timed out"
    );
    assert_eq!(timeout_turn, 0, "timeout should occur on first turn");

    drop(w1);
    drop(r1);
    drop(w2);
    drop(r2);
    let _ = h1.await;
    let _ = h2.await;
}

/// Bot disconnects during play → BotDisconnected event emitted.
#[tokio::test]
async fn disconnect_emits_event() {
    let (game_tx, mut game_rx) = mpsc::channel(64);
    let (mut w1, mut r1, h1) = spawn_session(SessionId(1), game_tx.clone());
    let (mut w2, mut r2, h2) = spawn_session(SessionId(2), game_tx.clone());
    drop(game_tx);

    let (event_tx, mut event_rx) = mpsc::unbounded_channel::<MatchEvent>();

    let setup = MatchSetup {
        players: vec![
            PlayerEntry {
                player: Player::Player1,
                agent_id: "bot-a".into(),
            },
            PlayerEntry {
                player: Player::Player2,
                agent_id: "bot-b".into(),
            },
        ],
        match_config: simple_match_config(),
        bot_options: HashMap::new(),
        timing: fast_timing(),
    };

    let setup_event_tx = event_tx.clone();
    let setup_ref = &setup;
    let setup_tx_ref = &setup_event_tx;
    let (_, setup_result) = tokio::join!(
        async {
            tokio::join!(
                drive_bot_through_setup(&mut w1, &mut r1, "BotA", "AuthA", "bot-a"),
                drive_bot_through_setup(&mut w2, &mut r2, "BotB", "AuthB", "bot-b"),
            );
        },
        async {
            run_setup(setup_ref, &mut game_rx, Some(setup_tx_ref))
                .await
                .expect("setup failed")
        },
    );
    drop(setup_event_tx);

    let sessions = setup_result.sessions;
    let mut game = tiny_game(3);
    let config = PlayingConfig {
        move_timeout: Duration::from_millis(200),
    };

    let play_task = tokio::spawn(async move {
        run_playing(&mut game, &sessions, &mut game_rx, &config, Some(&event_tx)).await
    });

    // Turn 0: both respond
    let _ = read_turn_state(&mut r1).await;
    let _ = read_turn_state(&mut r2).await;
    w1.write_frame(&action_frame(Direction::Stay, Player::Player1))
        .await
        .unwrap();
    w2.write_frame(&action_frame(Direction::Stay, Player::Player2))
        .await
        .unwrap();

    // Turn 1: P2 disconnects
    let _ = read_turn_state(&mut r1).await;
    let _ = read_turn_state(&mut r2).await;
    drop(w2);
    drop(r2);
    w1.write_frame(&action_frame(Direction::Stay, Player::Player1))
        .await
        .unwrap();

    // Remaining turns
    for _ in 2..3 {
        let _ = read_turn_state(&mut r1).await;
        w1.write_frame(&action_frame(Direction::Stay, Player::Player1))
            .await
            .unwrap();
    }

    let _ = read_game_over(&mut r1).await;

    let _ = timeout(Duration::from_secs(5), play_task)
        .await
        .expect("play timed out")
        .expect("play panicked")
        .expect("play returned error");

    let mut events = Vec::new();
    while let Ok(event) = event_rx.try_recv() {
        events.push(event);
    }

    let disconnect_players: Vec<Player> = events
        .iter()
        .filter_map(|e| match e {
            MatchEvent::BotDisconnected { player, .. } => Some(*player),
            _ => None,
        })
        .collect();
    assert_eq!(
        disconnect_players.len(),
        1,
        "expected exactly one BotDisconnected event, got {disconnect_players:?}"
    );
    assert_eq!(
        disconnect_players[0],
        Player::Player2,
        "expected Player2 disconnect, got {disconnect_players:?}"
    );

    drop(w1);
    drop(r1);
    let _ = h1.await;
    let _ = h2.await;
}
