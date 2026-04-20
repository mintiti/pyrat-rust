//! Integration tests for run_session — async tests using tokio::io::duplex.

mod common;

use std::time::Duration;

use tokio::sync::mpsc;
use tokio::time::timeout;

use pyrat::Coordinates;
use pyrat::Direction as EngineDirection;

use pyrat_host::session::messages::*;
use pyrat_host::session::{run_session, SessionConfig};
use pyrat_host::wire::framing::{FrameReader, FrameWriter};
use pyrat_host::wire::*;
use pyrat_protocol::{HashedTurnState, OwnedMatchConfig, OwnedTurnState};

use common::*;

// ── Session-specific helpers ────────────────────────

fn action_frame(direction: Direction, player: Player, turn: u16) -> Vec<u8> {
    build_bot_frame(BotMessage::Action, move |fbb| {
        Action::create(
            fbb,
            &ActionArgs {
                direction,
                player,
                turn,
                provisional: false,
                think_ms: 0,
            },
        )
        .as_union_value()
    })
}

fn pong_frame() -> Vec<u8> {
    build_bot_frame(BotMessage::Pong, |fbb| {
        Pong::create(fbb, &PongArgs {}).as_union_value()
    })
}

fn info_frame(message: &str) -> Vec<u8> {
    let message = message.to_owned();
    build_bot_frame(BotMessage::Info, move |fbb| {
        let msg = fbb.create_string(&message);
        Info::create(
            fbb,
            &InfoArgs {
                message: Some(msg),
                ..Default::default()
            },
        )
        .as_union_value()
    })
}

/// Receive next SessionMsg with a timeout.
async fn recv(rx: &mut mpsc::Receiver<SessionMsg>) -> SessionMsg {
    timeout(Duration::from_secs(2), rx.recv())
        .await
        .expect("timed out waiting for SessionMsg")
        .expect("channel closed")
}

/// Try to receive, returning None if nothing arrives quickly.
async fn try_recv(rx: &mut mpsc::Receiver<SessionMsg>) -> Option<SessionMsg> {
    timeout(Duration::from_millis(100), rx.recv())
        .await
        .ok()
        .flatten()
}

fn session_match_config() -> OwnedMatchConfig {
    let mut cfg = simple_match_config();
    cfg.controlled_players = vec![Player::Player1];
    cfg
}

// ── Tests ───────────────────────────────────────────

#[tokio::test]
async fn happy_path_full_lifecycle() {
    let (bot_io, session_io) = tokio::io::duplex(8192);
    let (bot_read, bot_write) = tokio::io::split(bot_io);
    let (session_read, session_write) = tokio::io::split(session_io);

    let (game_tx, mut game_rx) = mpsc::channel(32);
    let session_id = SessionId(1);

    let session_handle = tokio::spawn(run_session(
        session_id,
        session_read,
        session_write,
        game_tx,
        SessionConfig::default(),
    ));

    let mut bot_writer = FrameWriter::with_default_max(bot_write);
    let mut bot_reader = FrameReader::with_default_max(bot_read);

    // 1. Connected
    let msg = recv(&mut game_rx).await;
    let cmd_tx = match msg {
        SessionMsg::Connected {
            session_id: id,
            cmd_tx,
        } => {
            assert_eq!(id, session_id);
            cmd_tx
        },
        other => panic!("expected Connected, got {other:?}"),
    };

    // 2. Bot sends Identify
    bot_writer
        .write_frame(&identify_frame("TestBot", "Author"))
        .await
        .unwrap();
    let msg = recv(&mut game_rx).await;
    match msg {
        SessionMsg::Identified {
            session_id: id,
            name,
            author,
            ..
        } => {
            assert_eq!(id, session_id);
            assert_eq!(name, "TestBot");
            assert_eq!(author, "Author");
        },
        other => panic!("expected Identified, got {other:?}"),
    }

    // 3. Bot sends Ready
    bot_writer.write_frame(&ready_frame()).await.unwrap();
    let msg = recv(&mut game_rx).await;
    assert!(matches!(msg, SessionMsg::Ready { .. }));

    // 4. Host sends MatchConfig
    cmd_tx
        .send(HostCommand::MatchConfig(Box::new(session_match_config())))
        .await
        .unwrap();
    // Bot should receive a MatchConfig packet.
    let frame = bot_reader.read_frame().await.unwrap();
    let packet = flatbuffers::root::<HostPacket>(frame).unwrap();
    assert_eq!(packet.message_type(), HostMessage::MatchConfig);

    // 5. Host sends StartPreprocessing
    cmd_tx
        .send(HostCommand::StartPreprocessing { state_hash: 0 })
        .await
        .unwrap();
    let frame = bot_reader.read_frame().await.unwrap();
    let packet = flatbuffers::root::<HostPacket>(frame).unwrap();
    assert_eq!(packet.message_type(), HostMessage::StartPreprocessing);

    // 6. Bot sends PreprocessingDone
    bot_writer
        .write_frame(&preprocessing_done_frame())
        .await
        .unwrap();
    let msg = recv(&mut game_rx).await;
    assert!(matches!(msg, SessionMsg::PreprocessingDone { .. }));

    // 7. Host sends TurnState, bot sends Action
    cmd_tx
        .send(HostCommand::TurnState(Box::new(HashedTurnState::new(
            OwnedTurnState {
                turn: 1,
                player1_position: Coordinates::new(20, 14),
                player2_position: Coordinates::new(0, 0),
                player1_score: 0.0,
                player2_score: 0.0,
                player1_mud_turns: 0,
                player2_mud_turns: 0,
                cheese: vec![Coordinates::new(10, 7)],
                player1_last_move: EngineDirection::Stay,
                player2_last_move: EngineDirection::Stay,
            },
        ))))
        .await
        .unwrap();

    let frame = bot_reader.read_frame().await.unwrap();
    let packet = flatbuffers::root::<HostPacket>(frame).unwrap();
    assert_eq!(packet.message_type(), HostMessage::TurnState);

    bot_writer
        .write_frame(&action_frame(Direction::Left, Player::Player1, 1))
        .await
        .unwrap();
    let msg = recv(&mut game_rx).await;
    match msg {
        SessionMsg::Action {
            player,
            direction,
            turn,
            ..
        } => {
            assert_eq!(player, Player::Player1);
            assert_eq!(direction, EngineDirection::Left);
            assert_eq!(turn, 1);
        },
        other => panic!("expected Action, got {other:?}"),
    }

    // 8. Host sends GameOver
    cmd_tx
        .send(HostCommand::GameOver {
            result: GameResult::Player1,
            player1_score: 1.0,
            player2_score: 0.0,
        })
        .await
        .unwrap();

    let frame = bot_reader.read_frame().await.unwrap();
    let packet = flatbuffers::root::<HostPacket>(frame).unwrap();
    assert_eq!(packet.message_type(), HostMessage::GameOver);

    // 9. Drop bot side → Disconnected
    drop(bot_writer);
    drop(bot_reader);

    let msg = recv(&mut game_rx).await;
    assert!(matches!(msg, SessionMsg::Disconnected { .. }));

    session_handle.await.unwrap();
}

#[tokio::test]
async fn wrong_state_rejection_action_before_playing() {
    let (bot_io, session_io) = tokio::io::duplex(8192);
    let (bot_read, bot_write) = tokio::io::split(bot_io);
    let (session_read, session_write) = tokio::io::split(session_io);

    let (game_tx, mut game_rx) = mpsc::channel(32);

    let session_handle = tokio::spawn(run_session(
        SessionId(2),
        session_read,
        session_write,
        game_tx,
        SessionConfig::default(),
    ));

    let mut bot_writer = FrameWriter::with_default_max(bot_write);
    let _bot_reader = FrameReader::with_default_max(bot_read);

    // Get Connected
    let _connected = recv(&mut game_rx).await;

    // Send Action in Connected state — should be rejected silently
    bot_writer
        .write_frame(&action_frame(Direction::Up, Player::Player1, 0))
        .await
        .unwrap();

    // Send Identify to advance state
    bot_writer
        .write_frame(&identify_frame("Bot", "Auth"))
        .await
        .unwrap();

    // We should only get Identified, not Action
    let msg = recv(&mut game_rx).await;
    assert!(matches!(msg, SessionMsg::Identified { .. }));

    // No Action message should have been forwarded
    let maybe = try_recv(&mut game_rx).await;
    assert!(maybe.is_none(), "expected no message, got {maybe:?}");

    drop(bot_writer);
    drop(_bot_reader);

    let msg = recv(&mut game_rx).await;
    assert!(matches!(msg, SessionMsg::Disconnected { .. }));

    session_handle.await.unwrap();
}

#[tokio::test]
async fn ownership_validation_rejects_non_controlled_player() {
    let (bot_io, session_io) = tokio::io::duplex(8192);
    let (bot_read, bot_write) = tokio::io::split(bot_io);
    let (session_read, session_write) = tokio::io::split(session_io);

    let (game_tx, mut game_rx) = mpsc::channel(32);

    let session_handle = tokio::spawn(run_session(
        SessionId(3),
        session_read,
        session_write,
        game_tx,
        SessionConfig::default(),
    ));

    let mut bot_writer = FrameWriter::with_default_max(bot_write);
    let mut bot_reader = FrameReader::with_default_max(bot_read);

    // Connected
    let cmd_tx = match recv(&mut game_rx).await {
        SessionMsg::Connected { cmd_tx, .. } => cmd_tx,
        other => panic!("expected Connected, got {other:?}"),
    };

    // Identify → Ready → MatchConfig (controls Rat only) → StartPreprocessing → PreprocessingDone
    bot_writer
        .write_frame(&identify_frame("Bot", "Auth"))
        .await
        .unwrap();
    let _ = recv(&mut game_rx).await; // Identified

    bot_writer.write_frame(&ready_frame()).await.unwrap();
    let _ = recv(&mut game_rx).await; // Ready

    cmd_tx
        .send(HostCommand::MatchConfig(Box::new(session_match_config())))
        .await
        .unwrap();
    let _ = bot_reader.read_frame().await.unwrap(); // MatchConfig

    cmd_tx
        .send(HostCommand::StartPreprocessing { state_hash: 0 })
        .await
        .unwrap();
    let _ = bot_reader.read_frame().await.unwrap(); // StartPreprocessing

    bot_writer
        .write_frame(&preprocessing_done_frame())
        .await
        .unwrap();
    let _ = recv(&mut game_rx).await; // PreprocessingDone

    // Now Playing — send Action for Python (not controlled)
    bot_writer
        .write_frame(&action_frame(Direction::Up, Player::Player2, 0))
        .await
        .unwrap();

    // Should not be forwarded — try to get something else
    let maybe = try_recv(&mut game_rx).await;
    assert!(
        maybe.is_none(),
        "action for non-controlled player should be rejected"
    );

    // Send valid Action for Rat
    bot_writer
        .write_frame(&action_frame(Direction::Down, Player::Player1, 0))
        .await
        .unwrap();
    let msg = recv(&mut game_rx).await;
    match msg {
        SessionMsg::Action {
            player, direction, ..
        } => {
            assert_eq!(player, Player::Player1);
            assert_eq!(direction, EngineDirection::Down);
        },
        other => panic!("expected Action, got {other:?}"),
    }

    drop(bot_writer);
    drop(bot_reader);

    let msg = recv(&mut game_rx).await;
    assert!(matches!(msg, SessionMsg::Disconnected { .. }));

    session_handle.await.unwrap();
}

#[tokio::test]
async fn shutdown_drains_and_disconnects() {
    let (bot_io, session_io) = tokio::io::duplex(8192);
    let (bot_read, bot_write) = tokio::io::split(bot_io);
    let (session_read, session_write) = tokio::io::split(session_io);

    let (game_tx, mut game_rx) = mpsc::channel(32);

    let session_handle = tokio::spawn(run_session(
        SessionId(4),
        session_read,
        session_write,
        game_tx,
        SessionConfig::default(),
    ));

    let mut bot_writer = FrameWriter::with_default_max(bot_write);
    let mut bot_reader = FrameReader::with_default_max(bot_read);

    let cmd_tx = match recv(&mut game_rx).await {
        SessionMsg::Connected { cmd_tx, .. } => cmd_tx,
        other => panic!("expected Connected, got {other:?}"),
    };

    // Send Shutdown before any handshake
    cmd_tx.send(HostCommand::Shutdown).await.unwrap();

    // Bot should receive Stop
    let frame = bot_reader.read_frame().await.unwrap();
    let packet = flatbuffers::root::<HostPacket>(frame).unwrap();
    assert_eq!(packet.message_type(), HostMessage::Stop);

    // Bot sends Identify after shutdown — should be drained, not forwarded
    bot_writer
        .write_frame(&identify_frame("Bot", "Auth"))
        .await
        .unwrap();

    let maybe = try_recv(&mut game_rx).await;
    assert!(maybe.is_none(), "messages after shutdown should be drained");

    // Drop bot → session exits
    drop(bot_writer);
    drop(bot_reader);

    let msg = recv(&mut game_rx).await;
    assert!(matches!(msg, SessionMsg::Disconnected { .. }));

    session_handle.await.unwrap();
}

#[tokio::test]
async fn tcp_disconnect_sends_disconnected() {
    let (bot_io, session_io) = tokio::io::duplex(8192);
    let (session_read, session_write) = tokio::io::split(session_io);

    let (game_tx, mut game_rx) = mpsc::channel(32);

    let session_handle = tokio::spawn(run_session(
        SessionId(5),
        session_read,
        session_write,
        game_tx,
        SessionConfig::default(),
    ));

    // Connected
    let _connected = recv(&mut game_rx).await;

    // Drop bot side immediately
    drop(bot_io);

    let msg = recv(&mut game_rx).await;
    assert!(matches!(msg, SessionMsg::Disconnected { .. }));

    session_handle.await.unwrap();
}

#[tokio::test]
async fn channel_closed_exits_cleanly() {
    let (bot_io, session_io) = tokio::io::duplex(8192);
    let (_bot_read, _bot_write) = tokio::io::split(bot_io);
    let (session_read, session_write) = tokio::io::split(session_io);

    let (game_tx, mut game_rx) = mpsc::channel(32);

    let session_handle = tokio::spawn(run_session(
        SessionId(6),
        session_read,
        session_write,
        game_tx,
        SessionConfig::default(),
    ));

    // Get Connected and the cmd_tx
    let cmd_tx = match recv(&mut game_rx).await {
        SessionMsg::Connected { cmd_tx, .. } => cmd_tx,
        other => panic!("expected Connected, got {other:?}"),
    };

    // Drop the command channel sender — simulates game loop going away
    drop(cmd_tx);

    // Session should exit and send Disconnected
    let msg = recv(&mut game_rx).await;
    match msg {
        SessionMsg::Disconnected { reason, .. } => {
            assert_eq!(reason, DisconnectReason::ChannelClosed);
        },
        other => panic!("expected Disconnected, got {other:?}"),
    }

    session_handle.await.unwrap();
}

#[tokio::test]
async fn pong_and_info_accepted_in_any_non_done_state() {
    let (bot_io, session_io) = tokio::io::duplex(8192);
    let (bot_read, bot_write) = tokio::io::split(bot_io);
    let (session_read, session_write) = tokio::io::split(session_io);

    let (game_tx, mut game_rx) = mpsc::channel(32);

    let session_handle = tokio::spawn(run_session(
        SessionId(7),
        session_read,
        session_write,
        game_tx,
        SessionConfig::default(),
    ));

    let mut bot_writer = FrameWriter::with_default_max(bot_write);
    let _bot_reader = FrameReader::with_default_max(bot_read);

    // Connected
    let _connected = recv(&mut game_rx).await;

    // Pong in Connected state — accepted but not forwarded
    bot_writer.write_frame(&pong_frame()).await.unwrap();
    let maybe = try_recv(&mut game_rx).await;
    assert!(maybe.is_none(), "Pong should not be forwarded");

    // Info in Connected state — forwarded
    bot_writer.write_frame(&info_frame("hello")).await.unwrap();
    let msg = recv(&mut game_rx).await;
    match msg {
        SessionMsg::Info { info, .. } => assert_eq!(info.message, "hello"),
        other => panic!("expected Info, got {other:?}"),
    }

    drop(bot_writer);
    drop(_bot_reader);

    let msg = recv(&mut game_rx).await;
    assert!(matches!(msg, SessionMsg::Disconnected { .. }));

    session_handle.await.unwrap();
}

#[tokio::test]
async fn default_player_inference_single_bot() {
    let (bot_io, session_io) = tokio::io::duplex(8192);
    let (bot_read, bot_write) = tokio::io::split(bot_io);
    let (session_read, session_write) = tokio::io::split(session_io);

    let (game_tx, mut game_rx) = mpsc::channel(32);

    let session_handle = tokio::spawn(run_session(
        SessionId(8),
        session_read,
        session_write,
        game_tx,
        SessionConfig::default(),
    ));

    let mut bot_writer = FrameWriter::with_default_max(bot_write);
    let mut bot_reader = FrameReader::with_default_max(bot_read);

    let cmd_tx = match recv(&mut game_rx).await {
        SessionMsg::Connected { cmd_tx, .. } => cmd_tx,
        other => panic!("expected Connected, got {other:?}"),
    };

    // Full handshake with Python as the only controlled player
    bot_writer
        .write_frame(&identify_frame("Bot", "Auth"))
        .await
        .unwrap();
    let _ = recv(&mut game_rx).await;

    bot_writer.write_frame(&ready_frame()).await.unwrap();
    let _ = recv(&mut game_rx).await;

    let mut config = simple_match_config();
    config.controlled_players = vec![Player::Player2]; // Only Python
    cmd_tx
        .send(HostCommand::MatchConfig(Box::new(config)))
        .await
        .unwrap();
    let _ = bot_reader.read_frame().await.unwrap();

    cmd_tx
        .send(HostCommand::StartPreprocessing { state_hash: 0 })
        .await
        .unwrap();
    let _ = bot_reader.read_frame().await.unwrap();

    bot_writer
        .write_frame(&preprocessing_done_frame())
        .await
        .unwrap();
    let _ = recv(&mut game_rx).await;

    // Send Action with default player (Rat/0) — should be inferred as Python
    bot_writer
        .write_frame(&action_frame(Direction::Up, Player::Player1, 0))
        .await
        .unwrap();
    let msg = recv(&mut game_rx).await;
    match msg {
        SessionMsg::Action {
            player, direction, ..
        } => {
            assert_eq!(player, Player::Player2, "should be inferred as Python");
            assert_eq!(direction, EngineDirection::Up);
        },
        other => panic!("expected Action, got {other:?}"),
    }

    drop(bot_writer);
    drop(bot_reader);

    let msg = recv(&mut game_rx).await;
    assert!(matches!(msg, SessionMsg::Disconnected { .. }));

    session_handle.await.unwrap();
}

// ── Stop command ────────────────────────────────────

#[tokio::test]
async fn stop_sends_wire_stop_and_session_stays_alive() {
    let (bot_io, session_io) = tokio::io::duplex(8192);
    let (bot_read, bot_write) = tokio::io::split(bot_io);
    let (session_read, session_write) = tokio::io::split(session_io);

    let (game_tx, mut game_rx) = mpsc::channel(32);

    let session_handle = tokio::spawn(run_session(
        SessionId(20),
        session_read,
        session_write,
        game_tx,
        SessionConfig::default(),
    ));

    let mut bot_writer = FrameWriter::with_default_max(bot_write);
    let mut bot_reader = FrameReader::with_default_max(bot_read);

    let cmd_tx = match recv(&mut game_rx).await {
        SessionMsg::Connected { cmd_tx, .. } => cmd_tx,
        other => panic!("expected Connected, got {other:?}"),
    };

    // Advance through setup to Playing.
    bot_writer
        .write_frame(&identify_frame("Bot", "Auth"))
        .await
        .unwrap();
    let _ = recv(&mut game_rx).await; // Identified

    bot_writer.write_frame(&ready_frame()).await.unwrap();
    let _ = recv(&mut game_rx).await; // Ready

    cmd_tx
        .send(HostCommand::MatchConfig(Box::new(session_match_config())))
        .await
        .unwrap();
    let _ = bot_reader.read_frame().await.unwrap(); // MatchConfig

    cmd_tx
        .send(HostCommand::StartPreprocessing { state_hash: 0 })
        .await
        .unwrap();
    let _ = bot_reader.read_frame().await.unwrap(); // StartPreprocessing

    bot_writer
        .write_frame(&preprocessing_done_frame())
        .await
        .unwrap();
    let _ = recv(&mut game_rx).await; // PreprocessingDone

    // Send Stop (not Shutdown) — bot should receive Stop frame.
    cmd_tx.send(HostCommand::Stop).await.unwrap();
    let frame = bot_reader.read_frame().await.unwrap();
    let packet = flatbuffers::root::<HostPacket>(frame).unwrap();
    assert_eq!(packet.message_type(), HostMessage::Stop);

    // Session stays alive — send TurnState after Stop.
    cmd_tx
        .send(HostCommand::TurnState(Box::new(HashedTurnState::new(
            OwnedTurnState {
                turn: 1,
                player1_position: Coordinates::new(20, 14),
                player2_position: Coordinates::new(0, 0),
                player1_score: 0.0,
                player2_score: 0.0,
                player1_mud_turns: 0,
                player2_mud_turns: 0,
                cheese: vec![Coordinates::new(10, 7)],
                player1_last_move: EngineDirection::Stay,
                player2_last_move: EngineDirection::Stay,
            },
        ))))
        .await
        .unwrap();

    let frame = bot_reader.read_frame().await.unwrap();
    let packet = flatbuffers::root::<HostPacket>(frame).unwrap();
    assert_eq!(
        packet.message_type(),
        HostMessage::TurnState,
        "session should still be alive after Stop"
    );

    // Bot can still send actions.
    bot_writer
        .write_frame(&action_frame(Direction::Left, Player::Player1, 0))
        .await
        .unwrap();
    let msg = recv(&mut game_rx).await;
    assert!(
        matches!(msg, SessionMsg::Action { .. }),
        "bot should still be able to send actions after Stop"
    );

    drop(bot_writer);
    drop(bot_reader);

    let msg = recv(&mut game_rx).await;
    assert!(matches!(msg, SessionMsg::Disconnected { .. }));

    session_handle.await.unwrap();
}

// ── New tests ───────────────────────────────────────

#[tokio::test]
async fn game_over_then_bot_message_rejected() {
    let (bot_io, session_io) = tokio::io::duplex(8192);
    let (bot_read, bot_write) = tokio::io::split(bot_io);
    let (session_read, session_write) = tokio::io::split(session_io);

    let (game_tx, mut game_rx) = mpsc::channel(32);

    let session_handle = tokio::spawn(run_session(
        SessionId(10),
        session_read,
        session_write,
        game_tx,
        SessionConfig::default(),
    ));

    let mut bot_writer = FrameWriter::with_default_max(bot_write);
    let mut bot_reader = FrameReader::with_default_max(bot_read);

    let cmd_tx = match recv(&mut game_rx).await {
        SessionMsg::Connected { cmd_tx, .. } => cmd_tx,
        other => panic!("expected Connected, got {other:?}"),
    };

    // Advance to Playing
    bot_writer
        .write_frame(&identify_frame("Bot", "Auth"))
        .await
        .unwrap();
    let _ = recv(&mut game_rx).await;

    bot_writer.write_frame(&ready_frame()).await.unwrap();
    let _ = recv(&mut game_rx).await;

    cmd_tx
        .send(HostCommand::MatchConfig(Box::new(session_match_config())))
        .await
        .unwrap();
    let _ = bot_reader.read_frame().await.unwrap();

    cmd_tx
        .send(HostCommand::StartPreprocessing { state_hash: 0 })
        .await
        .unwrap();
    let _ = bot_reader.read_frame().await.unwrap();

    bot_writer
        .write_frame(&preprocessing_done_frame())
        .await
        .unwrap();
    let _ = recv(&mut game_rx).await;

    // Send GameOver
    cmd_tx
        .send(HostCommand::GameOver {
            result: GameResult::Draw,
            player1_score: 0.0,
            player2_score: 0.0,
        })
        .await
        .unwrap();
    let _ = bot_reader.read_frame().await.unwrap(); // GameOver frame

    // Bot sends Action after GameOver — should be drained, not forwarded
    bot_writer
        .write_frame(&action_frame(Direction::Up, Player::Player1, 0))
        .await
        .unwrap();

    let maybe = try_recv(&mut game_rx).await;
    assert!(
        maybe.is_none(),
        "action after GameOver should be drained, got {maybe:?}"
    );

    drop(bot_writer);
    drop(bot_reader);

    let msg = recv(&mut game_rx).await;
    assert!(matches!(msg, SessionMsg::Disconnected { .. }));

    session_handle.await.unwrap();
}

#[tokio::test]
async fn multiple_controlled_players_no_inference() {
    let (bot_io, session_io) = tokio::io::duplex(8192);
    let (bot_read, bot_write) = tokio::io::split(bot_io);
    let (session_read, session_write) = tokio::io::split(session_io);

    let (game_tx, mut game_rx) = mpsc::channel(32);

    let session_handle = tokio::spawn(run_session(
        SessionId(11),
        session_read,
        session_write,
        game_tx,
        SessionConfig::default(),
    ));

    let mut bot_writer = FrameWriter::with_default_max(bot_write);
    let mut bot_reader = FrameReader::with_default_max(bot_read);

    let cmd_tx = match recv(&mut game_rx).await {
        SessionMsg::Connected { cmd_tx, .. } => cmd_tx,
        other => panic!("expected Connected, got {other:?}"),
    };

    // Full handshake with BOTH players controlled
    bot_writer
        .write_frame(&identify_frame("Bot", "Auth"))
        .await
        .unwrap();
    let _ = recv(&mut game_rx).await;

    bot_writer.write_frame(&ready_frame()).await.unwrap();
    let _ = recv(&mut game_rx).await;

    let mut config = simple_match_config();
    config.controlled_players = vec![Player::Player1, Player::Player2];
    cmd_tx
        .send(HostCommand::MatchConfig(Box::new(config)))
        .await
        .unwrap();
    let _ = bot_reader.read_frame().await.unwrap();

    cmd_tx
        .send(HostCommand::StartPreprocessing { state_hash: 0 })
        .await
        .unwrap();
    let _ = bot_reader.read_frame().await.unwrap();

    bot_writer
        .write_frame(&preprocessing_done_frame())
        .await
        .unwrap();
    let _ = recv(&mut game_rx).await;

    // Bot sends Rat action — should NOT be inferred, stays as Rat
    bot_writer
        .write_frame(&action_frame(Direction::Left, Player::Player1, 0))
        .await
        .unwrap();
    let msg = recv(&mut game_rx).await;
    match msg {
        SessionMsg::Action { player, .. } => {
            assert_eq!(
                player,
                Player::Player1,
                "no inference with 2 controlled players"
            );
        },
        other => panic!("expected Action, got {other:?}"),
    }

    drop(bot_writer);
    drop(bot_reader);

    let msg = recv(&mut game_rx).await;
    assert!(matches!(msg, SessionMsg::Disconnected { .. }));

    session_handle.await.unwrap();
}

#[tokio::test]
async fn malformed_flatbuffers_continues() {
    let (bot_io, session_io) = tokio::io::duplex(8192);
    let (bot_read, bot_write) = tokio::io::split(bot_io);
    let (session_read, session_write) = tokio::io::split(session_io);

    let (game_tx, mut game_rx) = mpsc::channel(32);

    let session_handle = tokio::spawn(run_session(
        SessionId(12),
        session_read,
        session_write,
        game_tx,
        SessionConfig::default(),
    ));

    let mut bot_writer = FrameWriter::with_default_max(bot_write);
    let _bot_reader = FrameReader::with_default_max(bot_read);

    let _connected = recv(&mut game_rx).await;

    // Send garbage bytes as a frame
    bot_writer
        .write_frame(&[0xDE, 0xAD, 0xBE, 0xEF])
        .await
        .unwrap();

    // Session should survive — send a valid Identify after
    bot_writer
        .write_frame(&identify_frame("Bot", "Auth"))
        .await
        .unwrap();

    let msg = recv(&mut game_rx).await;
    assert!(
        matches!(msg, SessionMsg::Identified { .. }),
        "session should survive malformed frame"
    );

    drop(bot_writer);
    drop(_bot_reader);

    let msg = recv(&mut game_rx).await;
    assert!(matches!(msg, SessionMsg::Disconnected { .. }));

    session_handle.await.unwrap();
}

#[tokio::test]
async fn empty_controlled_players_skips_ownership_check() {
    let (bot_io, session_io) = tokio::io::duplex(8192);
    let (bot_read, bot_write) = tokio::io::split(bot_io);
    let (session_read, session_write) = tokio::io::split(session_io);

    let (game_tx, mut game_rx) = mpsc::channel(32);

    let session_handle = tokio::spawn(run_session(
        SessionId(13),
        session_read,
        session_write,
        game_tx,
        SessionConfig::default(),
    ));

    let mut bot_writer = FrameWriter::with_default_max(bot_write);
    let mut bot_reader = FrameReader::with_default_max(bot_read);

    let cmd_tx = match recv(&mut game_rx).await {
        SessionMsg::Connected { cmd_tx, .. } => cmd_tx,
        other => panic!("expected Connected, got {other:?}"),
    };

    bot_writer
        .write_frame(&identify_frame("Bot", "Auth"))
        .await
        .unwrap();
    let _ = recv(&mut game_rx).await;

    bot_writer.write_frame(&ready_frame()).await.unwrap();
    let _ = recv(&mut game_rx).await;

    // MatchConfig with empty controlled_players
    let mut config = simple_match_config();
    config.controlled_players = vec![];
    cmd_tx
        .send(HostCommand::MatchConfig(Box::new(config)))
        .await
        .unwrap();
    let _ = bot_reader.read_frame().await.unwrap();

    cmd_tx
        .send(HostCommand::StartPreprocessing { state_hash: 0 })
        .await
        .unwrap();
    let _ = bot_reader.read_frame().await.unwrap();

    bot_writer
        .write_frame(&preprocessing_done_frame())
        .await
        .unwrap();
    let _ = recv(&mut game_rx).await;

    // Any player should be accepted when controlled_players is empty
    bot_writer
        .write_frame(&action_frame(Direction::Up, Player::Player2, 0))
        .await
        .unwrap();
    let msg = recv(&mut game_rx).await;
    match msg {
        SessionMsg::Action { player, .. } => {
            assert_eq!(player, Player::Player2);
        },
        other => panic!("expected Action, got {other:?}"),
    }

    drop(bot_writer);
    drop(bot_reader);

    let msg = recv(&mut game_rx).await;
    assert!(matches!(msg, SessionMsg::Disconnected { .. }));

    session_handle.await.unwrap();
}

#[tokio::test]
async fn handshake_timeout_disconnects() {
    let (bot_io, session_io) = tokio::io::duplex(8192);
    let (_bot_read, _bot_write) = tokio::io::split(bot_io);
    let (session_read, session_write) = tokio::io::split(session_io);

    let (game_tx, mut game_rx) = mpsc::channel(32);

    let config = SessionConfig {
        handshake_timeout: Duration::from_millis(50),
        ..SessionConfig::default()
    };

    let session_handle = tokio::spawn(run_session(
        SessionId(14),
        session_read,
        session_write,
        game_tx,
        config,
    ));

    // Connected arrives
    let _connected = recv(&mut game_rx).await;

    // Don't send Identify — wait for timeout
    let msg = recv(&mut game_rx).await;
    match msg {
        SessionMsg::Disconnected { reason, .. } => {
            assert_eq!(reason, DisconnectReason::HandshakeTimeout);
        },
        other => panic!("expected Disconnected with HandshakeTimeout, got {other:?}"),
    }

    session_handle.await.unwrap();
}

#[tokio::test]
async fn drain_budget_exhausted_breaks_loop() {
    let (bot_io, session_io) = tokio::io::duplex(8192);
    let (bot_read, bot_write) = tokio::io::split(bot_io);
    let (session_read, session_write) = tokio::io::split(session_io);

    let (game_tx, mut game_rx) = mpsc::channel(32);

    let config = SessionConfig {
        drain_max_frames: 3,
        drain_timeout: Duration::from_secs(10), // long timeout — budget should trigger first
        ..SessionConfig::default()
    };

    let session_handle = tokio::spawn(run_session(
        SessionId(15),
        session_read,
        session_write,
        game_tx,
        config,
    ));

    let mut bot_writer = FrameWriter::with_default_max(bot_write);
    let mut bot_reader = FrameReader::with_default_max(bot_read);

    let cmd_tx = match recv(&mut game_rx).await {
        SessionMsg::Connected { cmd_tx, .. } => cmd_tx,
        other => panic!("expected Connected, got {other:?}"),
    };

    // Send Shutdown
    cmd_tx.send(HostCommand::Shutdown).await.unwrap();
    let _ = bot_reader.read_frame().await.unwrap(); // Stop

    // Flood more frames than the drain budget (3)
    for _ in 0..5 {
        // Ignore write errors — session may close mid-flood
        let _ = bot_writer.write_frame(&identify_frame("Bot", "Auth")).await;
    }

    // Session should exit with DrainComplete
    let msg = recv(&mut game_rx).await;
    match msg {
        SessionMsg::Disconnected { reason, .. } => {
            assert_eq!(reason, DisconnectReason::DrainComplete);
        },
        other => panic!("expected Disconnected with DrainComplete, got {other:?}"),
    }

    session_handle.await.unwrap();
}

#[tokio::test]
async fn write_error_reports_frame_error() {
    // Use separate duplex pairs for each direction so we can independently
    // break the write path without affecting reads.
    // bot_write → session_read (bot sends to session)
    let (session_read, bot_write_stream) = tokio::io::duplex(8192);
    // session_write → bot_read (session sends to bot)
    let (bot_read_stream, session_write) = tokio::io::duplex(8192);

    let (game_tx, mut game_rx) = mpsc::channel(32);

    let session_handle = tokio::spawn(run_session(
        SessionId(16),
        session_read,
        session_write,
        game_tx,
        SessionConfig::default(),
    ));

    let mut bot_writer = FrameWriter::with_default_max(bot_write_stream);

    let cmd_tx = match recv(&mut game_rx).await {
        SessionMsg::Connected { cmd_tx, .. } => cmd_tx,
        other => panic!("expected Connected, got {other:?}"),
    };

    // Bot identifies so the session is alive and the cmd channel is active.
    bot_writer
        .write_frame(&identify_frame("Bot", "Auth"))
        .await
        .unwrap();
    let _ = recv(&mut game_rx).await; // Identified

    // Drop the bot's read stream — session's write side will get BrokenPipe.
    drop(bot_read_stream);

    // Host sends a command that requires writing to the bot.
    cmd_tx
        .send(HostCommand::MatchConfig(Box::new(session_match_config())))
        .await
        .unwrap();

    // Session should exit with FrameError, not PeerClosed.
    let msg = recv(&mut game_rx).await;
    match msg {
        SessionMsg::Disconnected { reason, .. } => {
            assert_eq!(
                reason,
                DisconnectReason::FrameError,
                "write failure should report FrameError, not PeerClosed"
            );
        },
        other => panic!("expected Disconnected, got {other:?}"),
    }

    session_handle.await.unwrap();
}
