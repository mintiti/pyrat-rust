//! Shared test helpers for host integration tests.
#![allow(dead_code)]

use std::time::Duration;

use flatbuffers::FlatBufferBuilder;
use tokio::sync::mpsc;

use pyrat::game::game_logic::GameState;
use pyrat::{Coordinates, GameBuilder};

use pyrat_host::game_loop::{PlayingConfig, SetupTiming};
use pyrat_host::session::messages::*;
use pyrat_host::session::{run_session, SessionConfig, SessionId};
use pyrat_host::wire::framing::{FrameReader, FrameWriter};
use pyrat_host::wire::*;

/// Build a framed BotPacket from a closure that builds the inner message.
pub fn build_bot_frame<F>(msg_type: BotMessage, build_msg: F) -> Vec<u8>
where
    F: FnOnce(&mut FlatBufferBuilder) -> flatbuffers::WIPOffset<flatbuffers::UnionWIPOffset>,
{
    let mut fbb = FlatBufferBuilder::new();
    let msg_offset = build_msg(&mut fbb);
    let packet = BotPacket::create(
        &mut fbb,
        &BotPacketArgs {
            message_type: msg_type,
            message: Some(msg_offset),
        },
    );
    fbb.finish(packet, None);
    fbb.finished_data().to_vec()
}

pub fn identify_frame(name: &str, author: &str) -> Vec<u8> {
    identify_frame_with_agent(name, author, "")
}

pub fn identify_frame_with_agent(name: &str, author: &str, agent_id: &str) -> Vec<u8> {
    let name = name.to_owned();
    let author = author.to_owned();
    let agent_id = agent_id.to_owned();
    build_bot_frame(BotMessage::Identify, move |fbb| {
        let n = fbb.create_string(&name);
        let a = fbb.create_string(&author);
        let aid = if agent_id.is_empty() {
            None
        } else {
            Some(fbb.create_string(&agent_id))
        };
        Identify::create(
            fbb,
            &IdentifyArgs {
                name: Some(n),
                author: Some(a),
                options: None,
                agent_id: aid,
            },
        )
        .as_union_value()
    })
}

pub fn ready_frame() -> Vec<u8> {
    build_bot_frame(BotMessage::Ready, |fbb| {
        Ready::create(fbb, &ReadyArgs {}).as_union_value()
    })
}

pub fn preprocessing_done_frame() -> Vec<u8> {
    build_bot_frame(BotMessage::PreprocessingDone, |fbb| {
        PreprocessingDone::create(fbb, &PreprocessingDoneArgs {}).as_union_value()
    })
}

pub fn action_frame(direction: Direction, player: Player) -> Vec<u8> {
    build_bot_frame(BotMessage::Action, move |fbb| {
        Action::create(fbb, &ActionArgs { direction, player }).as_union_value()
    })
}

pub fn simple_match_config() -> OwnedMatchConfig {
    OwnedMatchConfig {
        width: 21,
        height: 15,
        max_turns: 300,
        walls: vec![],
        mud: vec![],
        cheese: vec![(10, 7)],
        player1_start: (20, 14),
        player2_start: (0, 0),
        controlled_players: vec![], // setup phase fills this
        timing: TimingMode::Wait,
        move_timeout_ms: 1000,
        preprocessing_timeout_ms: 5000,
    }
}

// ── Shared test infrastructure ──────────────────────

pub fn fast_timing() -> SetupTiming {
    SetupTiming {
        startup_timeout: Duration::from_secs(5),
        preprocessing_timeout: Duration::from_secs(2),
    }
}

pub fn fast_session_config() -> SessionConfig {
    SessionConfig {
        handshake_timeout: Duration::from_secs(5),
        ..SessionConfig::default()
    }
}

pub fn fast_playing_config() -> PlayingConfig {
    PlayingConfig {
        move_timeout: Duration::from_millis(500),
    }
}

/// Build a tiny 3×3 open game with one cheese at (1,1) and max_turns limit.
pub fn tiny_game(max_turns: u16) -> GameState {
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
pub fn spawn_session(
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

    (
        FrameWriter::with_default_max(bot_write),
        FrameReader::with_default_max(bot_read),
        handle,
    )
}

/// Drive a bot through Identify → Ready → (consume host frames until StartPreprocessing) → PreprocessingDone.
pub async fn drive_bot_through_setup(
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
