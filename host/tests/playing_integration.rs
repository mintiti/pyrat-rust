//! Integration tests for run_playing — the game loop turn cycle.

mod common;

use std::collections::HashMap;
use std::time::Duration;

use tokio::sync::mpsc;
use tokio::time::timeout;

use pyrat::game::game_logic::GameState;
use pyrat::{Coordinates, GameBuilder};

use pyrat_host::game_loop::{
    run_playing, run_setup, MatchSetup, PlayerEntry, PlayingConfig, SetupTiming,
};
use pyrat_host::session::messages::*;
use pyrat_host::session::{run_session, SessionConfig, SessionId};
use pyrat_host::wire::framing::{FrameReader, FrameWriter};
use pyrat_host::wire::*;

use common::*;

// ── Test infrastructure ─────────────────────────────

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

fn fast_playing_config() -> PlayingConfig {
    PlayingConfig {
        move_timeout: Duration::from_millis(500),
    }
}

/// Build a tiny 3×3 open game with one cheese at (1,1) and max_turns limit.
fn tiny_game(max_turns: u16) -> GameState {
    GameBuilder::new(3, 3)
        .with_max_turns(max_turns)
        .with_open_maze()
        .with_custom_positions(Coordinates::new(0, 0), Coordinates::new(2, 2))
        .with_custom_cheese(vec![Coordinates::new(1, 1)])
        .build()
        .create(Some(42))
        .expect("tiny game creation should not fail")
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

/// Drive a bot through the setup phase (Identify → Ready → consume MatchConfig + StartPreprocessing → PreprocessingDone).
async fn drive_bot_through_setup(
    writer: &mut FrameWriter<tokio::io::WriteHalf<tokio::io::DuplexStream>>,
    reader: &mut FrameReader<tokio::io::ReadHalf<tokio::io::DuplexStream>>,
    name: &str,
    author: &str,
    agent_id: &str,
) {
    writer
        .write_frame(&identify_frame_with_agent(name, author, agent_id))
        .await
        .unwrap();
    writer.write_frame(&ready_frame()).await.unwrap();

    // Consume host frames until StartPreprocessing.
    loop {
        let frame = reader.read_frame().await.unwrap();
        let packet = flatbuffers::root::<HostPacket>(frame).unwrap();
        if packet.message_type() == HostMessage::StartPreprocessing {
            break;
        }
    }

    writer
        .write_frame(&preprocessing_done_frame())
        .await
        .unwrap();
}

/// Run setup for a standard two-bot match and return the setup result + game_rx.
async fn setup_two_bots(
    game_tx: mpsc::Sender<SessionMsg>,
    game_rx: &mut mpsc::Receiver<SessionMsg>,
    w1: &mut FrameWriter<tokio::io::WriteHalf<tokio::io::DuplexStream>>,
    r1: &mut FrameReader<tokio::io::ReadHalf<tokio::io::DuplexStream>>,
    w2: &mut FrameWriter<tokio::io::WriteHalf<tokio::io::DuplexStream>>,
    r2: &mut FrameReader<tokio::io::ReadHalf<tokio::io::DuplexStream>>,
) -> Vec<pyrat_host::game_loop::SessionHandle> {
    drop(game_tx); // Only sessions hold senders.

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

    let setup_ref = &setup;
    let (_, result) = tokio::join!(
        async {
            tokio::join!(
                drive_bot_through_setup(w1, r1, "BotA", "AuthA", "bot-a"),
                drive_bot_through_setup(w2, r2, "BotB", "AuthB", "bot-b"),
            );
        },
        async {
            run_setup(setup_ref, game_rx, None)
                .await
                .expect("setup failed")
        },
    );

    result.sessions
}

/// Read the next TurnState from the bot reader.
async fn read_turn_state(
    reader: &mut FrameReader<tokio::io::ReadHalf<tokio::io::DuplexStream>>,
) -> (u16, Vec<(u8, u8)>) {
    let frame = timeout(Duration::from_secs(2), reader.read_frame())
        .await
        .expect("timed out waiting for TurnState")
        .unwrap();
    let packet = flatbuffers::root::<HostPacket>(frame).unwrap();
    assert_eq!(
        packet.message_type(),
        HostMessage::TurnState,
        "expected TurnState, got {:?}",
        packet.message_type()
    );
    let ts = packet.message_as_turn_state().unwrap();
    let cheese: Vec<(u8, u8)> = ts
        .cheese()
        .map(|cs| {
            (0..cs.len())
                .map(|i| (cs.get(i).x(), cs.get(i).y()))
                .collect()
        })
        .unwrap_or_default();
    (ts.turn(), cheese)
}

/// Read the next frame, expecting GameOver.
async fn read_game_over(
    reader: &mut FrameReader<tokio::io::ReadHalf<tokio::io::DuplexStream>>,
) -> GameResult {
    let frame = timeout(Duration::from_secs(2), reader.read_frame())
        .await
        .expect("timed out waiting for GameOver")
        .unwrap();
    let packet = flatbuffers::root::<HostPacket>(frame).unwrap();
    assert_eq!(
        packet.message_type(),
        HostMessage::GameOver,
        "expected GameOver, got {:?}",
        packet.message_type()
    );
    packet.message_as_game_over().unwrap().result()
}

/// Read the next frame, expecting Timeout.
async fn read_timeout(reader: &mut FrameReader<tokio::io::ReadHalf<tokio::io::DuplexStream>>) {
    let frame = timeout(Duration::from_secs(2), reader.read_frame())
        .await
        .expect("timed out waiting for Timeout")
        .unwrap();
    let packet = flatbuffers::root::<HostPacket>(frame).unwrap();
    assert_eq!(
        packet.message_type(),
        HostMessage::Timeout,
        "expected Timeout, got {:?}",
        packet.message_type()
    );
}

// ── Tests ───────────────────────────────────────────

/// Both bots act every turn. Player1 walks to the cheese and collects it, game ends.
#[tokio::test]
async fn happy_path_both_respond() {
    let (game_tx, mut game_rx) = mpsc::channel(64);
    let (mut w1, mut r1, h1) = spawn_session(SessionId(1), game_tx.clone());
    let (mut w2, mut r2, h2) = spawn_session(SessionId(2), game_tx.clone());

    let sessions = setup_two_bots(game_tx, &mut game_rx, &mut w1, &mut r1, &mut w2, &mut r2).await;

    // Tiny 3×3 game: P1 at (0,0), P2 at (2,2), cheese at (1,1), max 10 turns.
    let mut game = tiny_game(10);
    let config = fast_playing_config();

    let play_task = tokio::spawn(async move {
        run_playing(&mut game, &sessions, &mut game_rx, &config, None).await
    });

    // P1 needs to go Right then Up to reach (1,1).
    // Turn 0: both get TurnState.
    let _ = read_turn_state(&mut r1).await;
    let _ = read_turn_state(&mut r2).await;
    // P1 goes Right, P2 stays.
    w1.write_frame(&action_frame(Direction::Right, Player::Player1))
        .await
        .unwrap();
    w2.write_frame(&action_frame(Direction::Stay, Player::Player2))
        .await
        .unwrap();

    // Turn 1: P1 at (1,0), go Up.
    let _ = read_turn_state(&mut r1).await;
    let _ = read_turn_state(&mut r2).await;
    w1.write_frame(&action_frame(Direction::Up, Player::Player1))
        .await
        .unwrap();
    w2.write_frame(&action_frame(Direction::Stay, Player::Player2))
        .await
        .unwrap();

    // P1 now at (1,1), cheese collected, game should end.
    // Read GameOver.
    let result_msg = read_game_over(&mut r1).await;
    assert_eq!(result_msg, GameResult::Player1);
    let _ = read_game_over(&mut r2).await;

    let result = timeout(Duration::from_secs(5), play_task)
        .await
        .expect("play timed out")
        .expect("play panicked")
        .expect("play returned error");

    assert_eq!(result.result, GameResult::Player1);
    assert_eq!(result.player1_score, 1.0);
    assert_eq!(result.player2_score, 0.0);

    // Cleanup.
    drop(w1);
    drop(r1);
    drop(w2);
    drop(r2);
    let _ = h1.await;
    let _ = h2.await;
}

/// Both bots STAY every turn. Game ends at max_turns with a Draw.
#[tokio::test]
async fn both_stay_reaches_max_turns() {
    let (game_tx, mut game_rx) = mpsc::channel(64);
    let (mut w1, mut r1, h1) = spawn_session(SessionId(1), game_tx.clone());
    let (mut w2, mut r2, h2) = spawn_session(SessionId(2), game_tx.clone());

    let sessions = setup_two_bots(game_tx, &mut game_rx, &mut w1, &mut r1, &mut w2, &mut r2).await;

    let mut game = tiny_game(5); // Only 5 turns.
    let config = fast_playing_config();

    let play_task = tokio::spawn(async move {
        run_playing(&mut game, &sessions, &mut game_rx, &config, None).await
    });

    for _ in 0..5 {
        let _ = read_turn_state(&mut r1).await;
        let _ = read_turn_state(&mut r2).await;
        w1.write_frame(&action_frame(Direction::Stay, Player::Player1))
            .await
            .unwrap();
        w2.write_frame(&action_frame(Direction::Stay, Player::Player2))
            .await
            .unwrap();
    }

    let result_msg = read_game_over(&mut r1).await;
    assert_eq!(result_msg, GameResult::Draw);
    let _ = read_game_over(&mut r2).await;

    let result = timeout(Duration::from_secs(5), play_task)
        .await
        .expect("play timed out")
        .expect("play panicked")
        .expect("play returned error");

    assert_eq!(result.result, GameResult::Draw);
    assert_eq!(result.turns_played, 5);

    drop(w1);
    drop(r1);
    drop(w2);
    drop(r2);
    let _ = h1.await;
    let _ = h2.await;
}

/// One bot is silent (never sends Action). It times out and gets STAY + Timeout message.
#[tokio::test]
async fn timeout_defaults_to_stay() {
    let (game_tx, mut game_rx) = mpsc::channel(64);
    let (mut w1, mut r1, h1) = spawn_session(SessionId(1), game_tx.clone());
    let (mut w2, mut r2, h2) = spawn_session(SessionId(2), game_tx.clone());

    let sessions = setup_two_bots(game_tx, &mut game_rx, &mut w1, &mut r1, &mut w2, &mut r2).await;

    let mut game = tiny_game(3);
    let config = PlayingConfig {
        move_timeout: Duration::from_millis(100), // Short timeout for fast test.
    };

    let play_task = tokio::spawn(async move {
        run_playing(&mut game, &sessions, &mut game_rx, &config, None).await
    });

    // Turn 0: P1 responds, P2 silent.
    let _ = read_turn_state(&mut r1).await;
    let _ = read_turn_state(&mut r2).await;
    w1.write_frame(&action_frame(Direction::Stay, Player::Player1))
        .await
        .unwrap();
    // P2 doesn't respond — timeout fires.

    // P2 should receive Timeout.
    read_timeout(&mut r2).await;

    // Remaining turns: P1 responds, P2 silent.
    for _ in 1..3 {
        let _ = read_turn_state(&mut r1).await;
        let _ = read_turn_state(&mut r2).await;
        w1.write_frame(&action_frame(Direction::Stay, Player::Player1))
            .await
            .unwrap();
        // P2 silent again — timeout.
        read_timeout(&mut r2).await;
    }

    let _ = read_game_over(&mut r1).await;
    let _ = read_game_over(&mut r2).await;

    let result = timeout(Duration::from_secs(5), play_task)
        .await
        .expect("play timed out")
        .expect("play panicked")
        .expect("play returned error");

    assert_eq!(result.result, GameResult::Draw);

    drop(w1);
    drop(r1);
    drop(w2);
    drop(r2);
    let _ = h1.await;
    let _ = h2.await;
}

/// Bot disconnects mid-game. Game continues with STAY for that bot.
#[tokio::test]
async fn disconnect_mid_game() {
    let (game_tx, mut game_rx) = mpsc::channel(64);
    let (mut w1, mut r1, h1) = spawn_session(SessionId(1), game_tx.clone());
    let (mut w2, mut r2, h2) = spawn_session(SessionId(2), game_tx.clone());

    let sessions = setup_two_bots(game_tx, &mut game_rx, &mut w1, &mut r1, &mut w2, &mut r2).await;

    let mut game = tiny_game(5);
    let config = PlayingConfig {
        move_timeout: Duration::from_millis(200),
    };

    let play_task = tokio::spawn(async move {
        run_playing(&mut game, &sessions, &mut game_rx, &config, None).await
    });

    // Turn 0: both respond.
    let _ = read_turn_state(&mut r1).await;
    let _ = read_turn_state(&mut r2).await;
    w1.write_frame(&action_frame(Direction::Stay, Player::Player1))
        .await
        .unwrap();
    w2.write_frame(&action_frame(Direction::Stay, Player::Player2))
        .await
        .unwrap();

    // Turn 1: P2 disconnects.
    let _ = read_turn_state(&mut r1).await;
    let _ = read_turn_state(&mut r2).await;
    drop(w2);
    drop(r2);
    // P1 responds normally.
    w1.write_frame(&action_frame(Direction::Stay, Player::Player1))
        .await
        .unwrap();

    // Game should continue. Remaining turns P2 gets STAY automatically.
    for _ in 2..5 {
        let _ = read_turn_state(&mut r1).await;
        w1.write_frame(&action_frame(Direction::Stay, Player::Player1))
            .await
            .unwrap();
    }

    let _ = read_game_over(&mut r1).await;

    let result = timeout(Duration::from_secs(5), play_task)
        .await
        .expect("play timed out")
        .expect("play panicked")
        .expect("play returned error");

    assert_eq!(result.result, GameResult::Draw);
    assert_eq!(result.turns_played, 5);

    drop(w1);
    drop(r1);
    let _ = h1.await;
    let _ = h2.await;
}

/// Both bots disconnect. Game runs to max_turns with STAY/STAY.
#[tokio::test]
async fn both_disconnect() {
    let (game_tx, mut game_rx) = mpsc::channel(64);
    let (mut w1, mut r1, h1) = spawn_session(SessionId(1), game_tx.clone());
    let (mut w2, mut r2, h2) = spawn_session(SessionId(2), game_tx.clone());

    let sessions = setup_two_bots(game_tx, &mut game_rx, &mut w1, &mut r1, &mut w2, &mut r2).await;

    let mut game = tiny_game(3);
    let config = PlayingConfig {
        move_timeout: Duration::from_millis(200),
    };

    let play_task = tokio::spawn(async move {
        run_playing(&mut game, &sessions, &mut game_rx, &config, None).await
    });

    // Turn 0: both respond.
    let _ = read_turn_state(&mut r1).await;
    let _ = read_turn_state(&mut r2).await;
    w1.write_frame(&action_frame(Direction::Stay, Player::Player1))
        .await
        .unwrap();
    w2.write_frame(&action_frame(Direction::Stay, Player::Player2))
        .await
        .unwrap();

    // Turn 1: both disconnect.
    let _ = read_turn_state(&mut r1).await;
    let _ = read_turn_state(&mut r2).await;
    drop(w1);
    drop(r1);
    drop(w2);
    drop(r2);

    // Game runs to completion with STAY/STAY.
    let result = timeout(Duration::from_secs(5), play_task)
        .await
        .expect("play timed out")
        .expect("play panicked")
        .expect("play returned error");

    assert_eq!(result.result, GameResult::Draw);
    assert_eq!(result.turns_played, 3);

    let _ = h1.await;
    let _ = h2.await;
}

/// Hivemind: one session controls both players, sends two Actions per turn.
#[tokio::test]
async fn hivemind_two_actions() {
    let (game_tx, mut game_rx) = mpsc::channel(64);
    let (mut w1, mut r1, h1) = spawn_session(SessionId(1), game_tx.clone());
    drop(game_tx);

    // Hivemind setup: single bot controls both players.
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

    let setup_ref = &setup;
    let (_, setup_result) = tokio::join!(
        drive_bot_through_setup(&mut w1, &mut r1, "Hive", "Auth", "hive"),
        async {
            run_setup(setup_ref, &mut game_rx, None)
                .await
                .expect("setup failed")
        },
    );

    let sessions = setup_result.sessions;
    assert_eq!(sessions.len(), 1);
    assert_eq!(sessions[0].controlled_players.len(), 2);

    let mut game = tiny_game(3);
    let config = fast_playing_config();

    let play_task = tokio::spawn(async move {
        run_playing(&mut game, &sessions, &mut game_rx, &config, None).await
    });

    for _ in 0..3 {
        let _ = read_turn_state(&mut r1).await;
        // One session sends two actions — one per player.
        w1.write_frame(&action_frame(Direction::Stay, Player::Player1))
            .await
            .unwrap();
        w1.write_frame(&action_frame(Direction::Stay, Player::Player2))
            .await
            .unwrap();
    }

    let _ = read_game_over(&mut r1).await;

    let result = timeout(Duration::from_secs(5), play_task)
        .await
        .expect("play timed out")
        .expect("play panicked")
        .expect("play returned error");

    assert_eq!(result.result, GameResult::Draw);
    assert_eq!(result.turns_played, 3);

    drop(w1);
    drop(r1);
    let _ = h1.await;
}

/// GameOver is sent to all connected sessions.
#[tokio::test]
async fn game_over_sent() {
    let (game_tx, mut game_rx) = mpsc::channel(64);
    let (mut w1, mut r1, h1) = spawn_session(SessionId(1), game_tx.clone());
    let (mut w2, mut r2, h2) = spawn_session(SessionId(2), game_tx.clone());

    let sessions = setup_two_bots(game_tx, &mut game_rx, &mut w1, &mut r1, &mut w2, &mut r2).await;

    let mut game = tiny_game(1); // Just 1 turn.
    let config = fast_playing_config();

    let play_task = tokio::spawn(async move {
        run_playing(&mut game, &sessions, &mut game_rx, &config, None).await
    });

    let _ = read_turn_state(&mut r1).await;
    let _ = read_turn_state(&mut r2).await;
    w1.write_frame(&action_frame(Direction::Stay, Player::Player1))
        .await
        .unwrap();
    w2.write_frame(&action_frame(Direction::Stay, Player::Player2))
        .await
        .unwrap();

    // Both should receive GameOver.
    let r1_result = read_game_over(&mut r1).await;
    let r2_result = read_game_over(&mut r2).await;
    assert_eq!(r1_result, GameResult::Draw);
    assert_eq!(r2_result, GameResult::Draw);

    let _ = timeout(Duration::from_secs(5), play_task)
        .await
        .expect("play timed out")
        .expect("play panicked")
        .expect("play returned error");

    drop(w1);
    drop(r1);
    drop(w2);
    drop(r2);
    let _ = h1.await;
    let _ = h2.await;
}

/// When P1 collects cheese, the next TurnState reflects the updated cheese list.
#[tokio::test]
async fn cheese_updates_in_state() {
    let (game_tx, mut game_rx) = mpsc::channel(64);
    let (mut w1, mut r1, h1) = spawn_session(SessionId(1), game_tx.clone());
    let (mut w2, mut r2, h2) = spawn_session(SessionId(2), game_tx.clone());

    let sessions = setup_two_bots(game_tx, &mut game_rx, &mut w1, &mut r1, &mut w2, &mut r2).await;

    // 3×3 open maze, P1 at (0,0), P2 at (2,2).
    // 3 cheese: (1,0), (0,2), (2,0) — P2 doesn't start on any cheese.
    // P1 collects (1,0) on turn 0 (score 1.0 < 1.5 threshold), game continues.
    let mut game = GameBuilder::new(3, 3)
        .with_max_turns(2)
        .with_open_maze()
        .with_custom_positions(Coordinates::new(0, 0), Coordinates::new(2, 2))
        .with_custom_cheese(vec![
            Coordinates::new(1, 0),
            Coordinates::new(0, 2),
            Coordinates::new(2, 0),
        ])
        .build()
        .create(Some(42))
        .expect("game creation failed");

    let config = fast_playing_config();

    let play_task = tokio::spawn(async move {
        run_playing(&mut game, &sessions, &mut game_rx, &config, None).await
    });

    // Turn 0: 3 cheese present.
    let (turn0, cheese0) = read_turn_state(&mut r1).await;
    let _ = read_turn_state(&mut r2).await;
    assert_eq!(turn0, 0);
    assert_eq!(cheese0.len(), 3, "turn 0 should have 3 cheese");

    // P1 moves Right to (1,0) to collect cheese. P2 stays.
    w1.write_frame(&action_frame(Direction::Right, Player::Player1))
        .await
        .unwrap();
    w2.write_frame(&action_frame(Direction::Stay, Player::Player2))
        .await
        .unwrap();

    // Turn 1: P1 collected (1,0), 2 cheese remain.
    let (turn1, cheese1) = read_turn_state(&mut r1).await;
    let _ = read_turn_state(&mut r2).await;
    assert_eq!(turn1, 1);
    assert_eq!(cheese1.len(), 2, "turn 1 should have 2 cheese");
    assert!(
        !cheese1.contains(&(1, 0)),
        "(1,0) should be gone after collection"
    );

    // Both stay. Game ends at max_turns = 2.
    w1.write_frame(&action_frame(Direction::Stay, Player::Player1))
        .await
        .unwrap();
    w2.write_frame(&action_frame(Direction::Stay, Player::Player2))
        .await
        .unwrap();

    let result_msg = read_game_over(&mut r1).await;
    let _ = read_game_over(&mut r2).await;
    assert_eq!(result_msg, GameResult::Player1);

    let result = timeout(Duration::from_secs(5), play_task)
        .await
        .expect("play timed out")
        .expect("play panicked")
        .expect("play returned error");

    assert_eq!(result.result, GameResult::Player1);
    assert_eq!(result.player1_score, 1.0);
    assert_eq!(result.player2_score, 0.0);
    assert_eq!(result.turns_played, 2);

    drop(w1);
    drop(r1);
    drop(w2);
    drop(r2);
    let _ = h1.await;
    let _ = h2.await;
}
