//! Shared test helpers for host integration tests.
#![allow(dead_code)]

use flatbuffers::FlatBufferBuilder;

use pyrat_host::session::messages::*;
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
