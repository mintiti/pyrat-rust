//! Minimal test bot that speaks FlatBuffers over TCP and plays random moves.
//!
//! Reads `PYRAT_HOST_PORT` and `PYRAT_AGENT_ID` from environment, connects to
//! the host, completes the setup handshake, then sends a random action each turn
//! until the game ends.

use flatbuffers::FlatBufferBuilder;
use rand::RngExt;
use tokio::net::TcpStream;

use pyrat_host::wire::framing::{FrameReader, FrameWriter};
use pyrat_host::wire::*;

fn build_bot_frame<F>(msg_type: BotMessage, build_msg: F) -> Vec<u8>
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

#[tokio::main]
async fn main() {
    let port: u16 = std::env::var("PYRAT_HOST_PORT")
        .expect("PYRAT_HOST_PORT not set")
        .parse()
        .expect("PYRAT_HOST_PORT not a valid port");
    let agent_id = std::env::var("PYRAT_AGENT_ID").unwrap_or_default();

    let stream = TcpStream::connect(format!("127.0.0.1:{port}"))
        .await
        .expect("failed to connect to host");
    let (read, write) = tokio::io::split(stream);
    let mut reader = FrameReader::with_default_max(read);
    let mut writer = FrameWriter::with_default_max(write);

    // Send Identify
    let identify = build_bot_frame(BotMessage::Identify, |fbb| {
        let n = fbb.create_string("TestBot");
        let a = fbb.create_string("test");
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
    });
    writer.write_frame(&identify).await.unwrap();

    // Send Ready
    let ready = build_bot_frame(BotMessage::Ready, |fbb| {
        Ready::create(fbb, &ReadyArgs { state_hash: 0 }).as_union_value()
    });
    writer.write_frame(&ready).await.unwrap();

    // Wait for MatchConfig and StartPreprocessing
    loop {
        let frame = reader.read_frame().await.unwrap();
        let packet = flatbuffers::root::<HostPacket>(frame).unwrap();
        if packet.message_type() == HostMessage::StartPreprocessing {
            break;
        }
    }

    // Send PreprocessingDone
    let done = build_bot_frame(BotMessage::PreprocessingDone, |fbb| {
        PreprocessingDone::create(fbb, &PreprocessingDoneArgs {}).as_union_value()
    });
    writer.write_frame(&done).await.unwrap();

    // Play loop: receive TurnState, send random action
    let mut rng = rand::rng();
    loop {
        let frame = match reader.read_frame().await {
            Ok(f) => f,
            Err(_) => break,
        };
        let packet = flatbuffers::root::<HostPacket>(frame).unwrap();

        match packet.message_type() {
            HostMessage::TurnState => {
                let ts = packet.message_as_turn_state().unwrap();
                let turn = ts.turn();
                // Pick a random direction (0-4: Up, Right, Down, Left, Stay)
                let dir_val: u8 = rng.random_range(0..5);
                let direction = Direction(dir_val);
                let action = build_bot_frame(BotMessage::Action, move |fbb| {
                    Action::create(
                        fbb,
                        &ActionArgs {
                            direction,
                            player: Player::Player1, // Session infers the actual player
                            turn,
                            provisional: false,
                            think_ms: 1,
                            state_hash: 0,
                        },
                    )
                    .as_union_value()
                });
                if writer.write_frame(&action).await.is_err() {
                    break;
                }
            },
            HostMessage::GameOver | HostMessage::Stop => {
                break;
            },
            _ => {
                // Ignore other messages (Timeout, SetOption, Ping, etc.)
            },
        }
    }
}
