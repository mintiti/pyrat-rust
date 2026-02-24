//! Integration tests for run_setup — async tests using tokio::io::duplex + real session tasks.

mod common;

use std::collections::HashMap;
use std::time::Duration;

use tokio::sync::mpsc;
use tokio::time::timeout;

use pyrat_host::game_loop::{run_setup, MatchSetup, PlayerEntry, SetupError, SetupTiming};
use pyrat_host::session::messages::*;
use pyrat_host::session::{run_session, SessionConfig, SessionId};
use pyrat_host::wire::framing::{FrameReader, FrameWriter};
use pyrat_host::wire::*;

use common::*;

fn fast_timing() -> SetupTiming {
    SetupTiming {
        startup_timeout: Duration::from_secs(5),
        preprocessing_timeout: Duration::from_secs(2),
    }
}

fn fast_session_config() -> SessionConfig {
    SessionConfig {
        handshake_timeout: Duration::from_secs(5),
        ..SessionConfig::default()
    }
}

/// Spawn a session task connected via duplex, returning the bot-side reader/writer.
fn spawn_session(
    session_id: SessionId,
    game_tx: mpsc::Sender<SessionMsg>,
) -> (
    FrameWriter<tokio::io::WriteHalf<tokio::io::DuplexStream>>,
    FrameReader<tokio::io::ReadHalf<tokio::io::DuplexStream>>,
    tokio::task::JoinHandle<()>,
) {
    let (bot_io, session_io) = tokio::io::duplex(8192);
    let (bot_read, bot_write) = tokio::io::split(bot_io);
    let (session_read, session_write) = tokio::io::split(session_io);

    let handle = tokio::spawn(run_session(
        session_id,
        session_read,
        session_write,
        game_tx,
        fast_session_config(),
    ));

    let bot_writer = FrameWriter::with_default_max(bot_write);
    let bot_reader = FrameReader::with_default_max(bot_read);

    (bot_writer, bot_reader, handle)
}

/// Drive a bot through Identify → Ready → (receive MatchConfig) → (receive StartPreprocessing) → PreprocessingDone.
async fn drive_bot_through_setup(
    writer: &mut FrameWriter<tokio::io::WriteHalf<tokio::io::DuplexStream>>,
    reader: &mut FrameReader<tokio::io::ReadHalf<tokio::io::DuplexStream>>,
    name: &str,
    author: &str,
    agent_id: &str,
) {
    // Send Identify
    writer
        .write_frame(&identify_frame_with_agent(name, author, agent_id))
        .await
        .unwrap();

    // Send Ready
    writer.write_frame(&ready_frame()).await.unwrap();

    // Read frames from host: could be SetOption(s) + MatchConfig + StartPreprocessing.
    // We need to consume until we see StartPreprocessing.
    loop {
        let frame = reader.read_frame().await.unwrap();
        let packet = flatbuffers::root::<HostPacket>(frame).unwrap();
        if packet.message_type() == HostMessage::StartPreprocessing {
            break;
        }
    }

    // Send PreprocessingDone
    writer
        .write_frame(&preprocessing_done_frame())
        .await
        .unwrap();
}

// ── Tests ───────────────────────────────────────────

#[tokio::test]
async fn happy_path_two_bots() {
    let (game_tx, mut game_rx) = mpsc::channel(64);

    // Spawn two bot sessions.
    let (mut w1, mut r1, h1) = spawn_session(SessionId(1), game_tx.clone());
    let (mut w2, mut r2, h2) = spawn_session(SessionId(2), game_tx.clone());
    drop(game_tx); // Only sessions hold senders now.

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

    let setup_task = tokio::spawn(async move { run_setup(&setup, &mut game_rx, None).await });

    // Drive both bots concurrently.
    let ((), ()) = tokio::join!(
        async { drive_bot_through_setup(&mut w1, &mut r1, "BotA", "AuthA", "bot-a").await },
        async { drive_bot_through_setup(&mut w2, &mut r2, "BotB", "AuthB", "bot-b").await },
    );

    let result = timeout(Duration::from_secs(5), setup_task)
        .await
        .expect("setup timed out")
        .expect("setup panicked")
        .expect("setup returned error");

    assert_eq!(result.sessions.len(), 2);

    // Verify controlled_players are assigned correctly.
    for s in &result.sessions {
        if s.agent_id == "bot-a" {
            assert_eq!(s.controlled_players, vec![Player::Player1]);
            assert_eq!(s.name, "BotA");
        } else if s.agent_id == "bot-b" {
            assert_eq!(s.controlled_players, vec![Player::Player2]);
            assert_eq!(s.name, "BotB");
        } else {
            panic!("unexpected agent_id: {}", s.agent_id);
        }
    }

    // Clean up.
    drop(w1);
    drop(r1);
    drop(w2);
    drop(r2);
    for s in result.sessions {
        drop(s.cmd_tx);
    }
    let _ = h1.await;
    let _ = h2.await;
}

#[tokio::test]
async fn hivemind_single_bot() {
    let (game_tx, mut game_rx) = mpsc::channel(64);

    let (mut w1, mut r1, h1) = spawn_session(SessionId(1), game_tx.clone());
    drop(game_tx);

    let setup = MatchSetup {
        players: vec![
            PlayerEntry {
                player: Player::Player1,
                agent_id: "hive".into(),
            },
            PlayerEntry {
                player: Player::Player2,
                agent_id: "hive".into(),
            },
        ],
        match_config: simple_match_config(),
        bot_options: HashMap::new(),
        timing: fast_timing(),
    };

    let setup_task = tokio::spawn(async move { run_setup(&setup, &mut game_rx, None).await });

    drive_bot_through_setup(&mut w1, &mut r1, "HiveBot", "Auth", "hive").await;

    let result = timeout(Duration::from_secs(5), setup_task)
        .await
        .expect("setup timed out")
        .expect("setup panicked")
        .expect("setup returned error");

    assert_eq!(result.sessions.len(), 1);
    let s = &result.sessions[0];
    assert_eq!(s.controlled_players, vec![Player::Player1, Player::Player2]);

    drop(w1);
    drop(r1);
    for s in result.sessions {
        drop(s.cmd_tx);
    }
    let _ = h1.await;
}

#[tokio::test]
async fn startup_timeout_no_bots() {
    let (game_tx, mut game_rx) = mpsc::channel(64);
    // Keep game_tx alive so channel doesn't close.
    let _keep = game_tx;

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
        timing: SetupTiming {
            startup_timeout: Duration::from_millis(100),
            preprocessing_timeout: Duration::from_millis(100),
        },
    };

    let result = run_setup(&setup, &mut game_rx, None).await;
    assert!(matches!(result, Err(SetupError::StartupTimeout)));
}

#[tokio::test]
async fn startup_timeout_one_bot() {
    let (game_tx, mut game_rx) = mpsc::channel(64);

    // Only one bot connects.
    let (mut w1, mut _r1, _h1) = spawn_session(SessionId(1), game_tx.clone());
    // Keep game_tx alive.
    let _keep = game_tx;

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
        timing: SetupTiming {
            startup_timeout: Duration::from_millis(200),
            preprocessing_timeout: Duration::from_millis(100),
        },
    };

    let setup_task = tokio::spawn(async move { run_setup(&setup, &mut game_rx, None).await });

    // Bot A identifies, but bot B never connects.
    w1.write_frame(&identify_frame_with_agent("BotA", "Auth", "bot-a"))
        .await
        .unwrap();

    let result = timeout(Duration::from_secs(5), setup_task)
        .await
        .expect("test timed out")
        .expect("setup panicked");

    assert!(matches!(result, Err(SetupError::StartupTimeout)));

    drop(w1);
    drop(_r1);
    let _ = _h1.await;
}

#[tokio::test]
async fn preprocessing_timeout_errors() {
    let (game_tx, mut game_rx) = mpsc::channel(64);

    let (mut w1, mut r1, h1) = spawn_session(SessionId(1), game_tx.clone());
    let (mut w2, mut r2, h2) = spawn_session(SessionId(2), game_tx.clone());
    drop(game_tx);

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
        timing: SetupTiming {
            startup_timeout: Duration::from_secs(5),
            preprocessing_timeout: Duration::from_millis(200),
        },
    };

    let setup_task = tokio::spawn(async move { run_setup(&setup, &mut game_rx, None).await });

    // Run both bots concurrently. Bot A completes fully; Bot B stops
    // before PreprocessingDone to trigger the timeout.
    let bot_a = async {
        drive_bot_through_setup(&mut w1, &mut r1, "BotA", "AuthA", "bot-a").await;
    };
    let bot_b = async {
        // Identify + Ready
        w2.write_frame(&identify_frame_with_agent("BotB", "AuthB", "bot-b"))
            .await
            .unwrap();
        w2.write_frame(&ready_frame()).await.unwrap();
        // Consume host frames until StartPreprocessing.
        loop {
            let frame = r2.read_frame().await.unwrap();
            let packet = flatbuffers::root::<HostPacket>(frame).unwrap();
            if packet.message_type() == HostMessage::StartPreprocessing {
                break;
            }
        }
        // Don't send PreprocessingDone — let the timeout fire.
    };
    tokio::join!(bot_a, bot_b);

    let result = timeout(Duration::from_secs(5), setup_task)
        .await
        .expect("test timed out")
        .expect("setup panicked");

    assert!(
        matches!(result, Err(SetupError::PreprocessingTimeout)),
        "expected PreprocessingTimeout, got {result:?}"
    );

    drop(w1);
    drop(r1);
    drop(w2);
    drop(r2);
    let _ = h1.await;
    let _ = h2.await;
}

#[tokio::test]
async fn disconnect_during_setup() {
    let (game_tx, mut game_rx) = mpsc::channel(64);

    let (mut w1, _r1, h1) = spawn_session(SessionId(1), game_tx.clone());
    let (mut w2, mut r2, h2) = spawn_session(SessionId(2), game_tx.clone());

    // Spawn a third session that will take bot-a's slot after w1 disconnects.
    let (mut w3, mut r3, h3) = spawn_session(SessionId(3), game_tx.clone());
    drop(game_tx);

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

    let setup_task = tokio::spawn(async move { run_setup(&setup, &mut game_rx, None).await });

    // Bot A identifies then disconnects.
    w1.write_frame(&identify_frame_with_agent("BotA", "Auth", "bot-a"))
        .await
        .unwrap();
    // Small delay to let session process the identify.
    tokio::time::sleep(Duration::from_millis(50)).await;
    drop(w1);
    drop(_r1);

    // Small delay to let disconnect propagate.
    tokio::time::sleep(Duration::from_millis(50)).await;

    // Bot B identifies normally.
    // Bot A2 (session 3) reconnects and takes the slot.
    tokio::join!(
        async { drive_bot_through_setup(&mut w2, &mut r2, "BotB", "AuthB", "bot-b").await },
        async { drive_bot_through_setup(&mut w3, &mut r3, "BotA2", "AuthA2", "bot-a").await },
    );

    let result = timeout(Duration::from_secs(5), setup_task)
        .await
        .expect("test timed out")
        .expect("setup panicked")
        .expect("setup returned error");

    assert_eq!(result.sessions.len(), 2);
    let bot_a = result
        .sessions
        .iter()
        .find(|s| s.agent_id == "bot-a")
        .unwrap();
    assert_eq!(
        bot_a.name, "BotA2",
        "reconnected bot should replace original"
    );

    drop(w2);
    drop(r2);
    drop(w3);
    drop(r3);
    for s in result.sessions {
        drop(s.cmd_tx);
    }
    let _ = h1.await;
    let _ = h2.await;
    let _ = h3.await;
}

#[tokio::test]
async fn unknown_agent_id_ignored() {
    let (game_tx, mut game_rx) = mpsc::channel(64);

    // Bad bot with wrong agent_id.
    let (mut w_bad, _r_bad, h_bad) = spawn_session(SessionId(1), game_tx.clone());
    // Good bots.
    let (mut w1, mut r1, h1) = spawn_session(SessionId(2), game_tx.clone());
    let (mut w2, mut r2, h2) = spawn_session(SessionId(3), game_tx.clone());
    drop(game_tx);

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

    let setup_task = tokio::spawn(async move { run_setup(&setup, &mut game_rx, None).await });

    // Bad bot identifies with wrong agent_id.
    w_bad
        .write_frame(&identify_frame_with_agent("BadBot", "Auth", "wrong-id"))
        .await
        .unwrap();

    // Good bots drive through setup normally.
    tokio::join!(
        async { drive_bot_through_setup(&mut w1, &mut r1, "BotA", "AuthA", "bot-a").await },
        async { drive_bot_through_setup(&mut w2, &mut r2, "BotB", "AuthB", "bot-b").await },
    );

    let result = timeout(Duration::from_secs(5), setup_task)
        .await
        .expect("test timed out")
        .expect("setup panicked")
        .expect("setup returned error");

    assert_eq!(result.sessions.len(), 2);
    // Bad bot should not be in the result.
    assert!(result.sessions.iter().all(|s| s.agent_id != "wrong-id"));

    drop(w_bad);
    drop(_r_bad);
    drop(w1);
    drop(r1);
    drop(w2);
    drop(r2);
    for s in result.sessions {
        drop(s.cmd_tx);
    }
    let _ = h_bad.await;
    let _ = h1.await;
    let _ = h2.await;
}

#[tokio::test]
async fn set_options_arrive_before_match_config() {
    let (game_tx, mut game_rx) = mpsc::channel(64);

    let (mut w1, mut r1, h1) = spawn_session(SessionId(1), game_tx.clone());
    let (mut w2, mut r2, h2) = spawn_session(SessionId(2), game_tx.clone());
    drop(game_tx);

    let mut bot_options = HashMap::new();
    bot_options.insert(
        "bot-a".to_string(),
        vec![("Hash".to_string(), "256".to_string())],
    );

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
        bot_options,
        timing: fast_timing(),
    };

    let setup_task = tokio::spawn(async move { run_setup(&setup, &mut game_rx, None).await });

    // Both bots identify and send Ready.
    w1.write_frame(&identify_frame_with_agent("BotA", "AuthA", "bot-a"))
        .await
        .unwrap();
    w2.write_frame(&identify_frame_with_agent("BotB", "AuthB", "bot-b"))
        .await
        .unwrap();
    w1.write_frame(&ready_frame()).await.unwrap();
    w2.write_frame(&ready_frame()).await.unwrap();

    // Bot A should receive: SetOption, then MatchConfig, then StartPreprocessing.
    let frame1 = r1.read_frame().await.unwrap();
    let pkt1 = flatbuffers::root::<HostPacket>(frame1).unwrap();
    assert_eq!(
        pkt1.message_type(),
        HostMessage::SetOption,
        "first frame to bot-a should be SetOption"
    );
    let so = pkt1.message_as_set_option().unwrap();
    assert_eq!(so.name(), Some("Hash"));
    assert_eq!(so.value(), Some("256"));

    let frame2 = r1.read_frame().await.unwrap();
    let pkt2 = flatbuffers::root::<HostPacket>(frame2).unwrap();
    assert_eq!(
        pkt2.message_type(),
        HostMessage::MatchConfig,
        "second frame to bot-a should be MatchConfig"
    );
    // Verify controlled_players was filled in.
    let mc = pkt2.message_as_match_config().unwrap();
    let cp: Vec<Player> = mc.controlled_players().unwrap().iter().collect();
    assert_eq!(cp, vec![Player::Player1]);

    let frame3 = r1.read_frame().await.unwrap();
    let pkt3 = flatbuffers::root::<HostPacket>(frame3).unwrap();
    assert_eq!(pkt3.message_type(), HostMessage::StartPreprocessing);

    // Bot B should receive: MatchConfig, then StartPreprocessing (no SetOption).
    let frame_b1 = r2.read_frame().await.unwrap();
    let pkt_b1 = flatbuffers::root::<HostPacket>(frame_b1).unwrap();
    assert_eq!(
        pkt_b1.message_type(),
        HostMessage::MatchConfig,
        "first frame to bot-b should be MatchConfig (no options)"
    );

    let frame_b2 = r2.read_frame().await.unwrap();
    let pkt_b2 = flatbuffers::root::<HostPacket>(frame_b2).unwrap();
    assert_eq!(pkt_b2.message_type(), HostMessage::StartPreprocessing);

    // Both bots send PreprocessingDone.
    w1.write_frame(&preprocessing_done_frame()).await.unwrap();
    w2.write_frame(&preprocessing_done_frame()).await.unwrap();

    let result = timeout(Duration::from_secs(5), setup_task)
        .await
        .expect("test timed out")
        .expect("setup panicked")
        .expect("setup returned error");

    assert_eq!(result.sessions.len(), 2);

    drop(w1);
    drop(r1);
    drop(w2);
    drop(r2);
    for s in result.sessions {
        drop(s.cmd_tx);
    }
    let _ = h1.await;
    let _ = h2.await;
}

// ── New strict-mode tests ───────────────────────────

#[tokio::test]
async fn disconnect_during_phase_b() {
    let (game_tx, mut game_rx) = mpsc::channel(64);

    let (mut w1, _r1, h1) = spawn_session(SessionId(1), game_tx.clone());
    let (mut w2, _r2, h2) = spawn_session(SessionId(2), game_tx.clone());
    drop(game_tx);

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

    let setup_task = tokio::spawn(async move { run_setup(&setup, &mut game_rx, None).await });

    // Both bots identify.
    w1.write_frame(&identify_frame_with_agent("BotA", "AuthA", "bot-a"))
        .await
        .unwrap();
    w2.write_frame(&identify_frame_with_agent("BotB", "AuthB", "bot-b"))
        .await
        .unwrap();

    // Bot A sends Ready.
    w1.write_frame(&ready_frame()).await.unwrap();

    // Bot B disconnects during Phase B.
    tokio::time::sleep(Duration::from_millis(50)).await;
    drop(w2);
    drop(_r2);

    let result = timeout(Duration::from_secs(5), setup_task)
        .await
        .expect("test timed out")
        .expect("setup panicked");

    assert!(
        matches!(result, Err(SetupError::BotDisconnected)),
        "expected BotDisconnected, got {result:?}"
    );

    drop(w1);
    drop(_r1);
    let _ = h1.await;
    let _ = h2.await;
}

#[tokio::test]
async fn disconnect_during_phase_c() {
    let (game_tx, mut game_rx) = mpsc::channel(64);

    let (mut w1, mut r1, h1) = spawn_session(SessionId(1), game_tx.clone());
    let (mut w2, mut r2, h2) = spawn_session(SessionId(2), game_tx.clone());
    drop(game_tx);

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
        timing: SetupTiming {
            startup_timeout: Duration::from_secs(5),
            preprocessing_timeout: Duration::from_secs(5),
        },
    };

    let setup_task = tokio::spawn(async move { run_setup(&setup, &mut game_rx, None).await });

    // Both bots identify and send Ready.
    let bot_a_setup = async {
        w1.write_frame(&identify_frame_with_agent("BotA", "AuthA", "bot-a"))
            .await
            .unwrap();
        w1.write_frame(&ready_frame()).await.unwrap();
        // Consume host frames until StartPreprocessing.
        loop {
            let frame = r1.read_frame().await.unwrap();
            let packet = flatbuffers::root::<HostPacket>(frame).unwrap();
            if packet.message_type() == HostMessage::StartPreprocessing {
                break;
            }
        }
    };
    let bot_b_setup = async {
        w2.write_frame(&identify_frame_with_agent("BotB", "AuthB", "bot-b"))
            .await
            .unwrap();
        w2.write_frame(&ready_frame()).await.unwrap();
        // Consume host frames until StartPreprocessing.
        loop {
            let frame = r2.read_frame().await.unwrap();
            let packet = flatbuffers::root::<HostPacket>(frame).unwrap();
            if packet.message_type() == HostMessage::StartPreprocessing {
                break;
            }
        }
    };
    tokio::join!(bot_a_setup, bot_b_setup);

    // Bot B disconnects during preprocessing.
    drop(w2);
    drop(r2);

    let result = timeout(Duration::from_secs(5), setup_task)
        .await
        .expect("test timed out")
        .expect("setup panicked");

    assert!(
        matches!(result, Err(SetupError::BotDisconnected)),
        "expected BotDisconnected, got {result:?}"
    );

    drop(w1);
    drop(r1);
    let _ = h1.await;
    let _ = h2.await;
}

#[tokio::test]
async fn all_disconnected_channel_closed() {
    let (game_tx, mut game_rx) = mpsc::channel(64);

    let (mut w1, _r1, h1) = spawn_session(SessionId(1), game_tx.clone());
    let (mut w2, _r2, h2) = spawn_session(SessionId(2), game_tx.clone());
    drop(game_tx);

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

    let setup_task = tokio::spawn(async move { run_setup(&setup, &mut game_rx, None).await });

    // Both bots identify.
    w1.write_frame(&identify_frame_with_agent("BotA", "AuthA", "bot-a"))
        .await
        .unwrap();
    w2.write_frame(&identify_frame_with_agent("BotB", "AuthB", "bot-b"))
        .await
        .unwrap();

    // Drop all bot-side I/O — sessions will disconnect, senders will close.
    tokio::time::sleep(Duration::from_millis(50)).await;
    drop(w1);
    drop(_r1);
    drop(w2);
    drop(_r2);

    let result = timeout(Duration::from_secs(5), setup_task)
        .await
        .expect("test timed out")
        .expect("setup panicked");

    // Could be BotDisconnected (first disconnect arrives) or AllDisconnected
    // (channel closes before any message). Both are acceptable failures.
    assert!(
        result.is_err(),
        "expected error after all bots disconnected, got {result:?}"
    );

    let _ = h1.await;
    let _ = h2.await;
}
