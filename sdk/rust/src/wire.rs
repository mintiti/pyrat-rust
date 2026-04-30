//! Wire codec adapter: thin layer over `pyrat_protocol::codec` for frame I/O.
//!
//! The SDK reads length-framed `HostPacket`s and writes length-framed
//! `BotPacket`s. All FlatBuffers extraction and serialization lives in
//! `pyrat_protocol::codec`; this module just wraps the bytes.

use pyrat_protocol::{extract_host_msg, serialize_bot_msg, BotMsg, HostMsg};
use pyrat_wire::{self as wire};

/// Parse a raw frame as a `HostPacket` and decode to an owned `HostMsg`.
pub fn parse_host_frame(buf: &[u8]) -> Result<HostMsg, String> {
    let packet =
        flatbuffers::root::<wire::HostPacket>(buf).map_err(|e| format!("verify error: {e}"))?;
    extract_host_msg(&packet).map_err(|e| format!("codec error: {e}"))
}

/// Serialize an owned `BotMsg` into a `BotPacket` byte buffer.
pub fn serialize(msg: &BotMsg) -> Vec<u8> {
    serialize_bot_msg(msg)
}

#[cfg(test)]
mod tests {
    use super::*;
    use pyrat::{Coordinates, Direction};
    use pyrat_protocol::{Info, MatchConfig};
    use pyrat_wire::{Player, TimingMode};

    #[test]
    fn welcome_round_trip() {
        let frame = serialize_bot_msg(&BotMsg::Identify {
            name: "T".into(),
            author: "A".into(),
            agent_id: "agent-1".into(),
            options: vec![],
        });
        let packet = flatbuffers::root::<wire::BotPacket>(&frame).unwrap();
        assert_eq!(packet.message_type(), wire::BotMessage::Identify);
        let id = packet.message_as_identify().unwrap();
        assert_eq!(id.name(), Some("T"));
        assert_eq!(id.agent_id(), Some("agent-1"));
    }

    #[test]
    fn parse_welcome_frame() {
        let bytes = pyrat_protocol::serialize_host_msg(&HostMsg::Welcome {
            player_slot: Player::Player2,
        });
        match parse_host_frame(&bytes).unwrap() {
            HostMsg::Welcome { player_slot } => assert_eq!(player_slot, Player::Player2),
            other => panic!("expected Welcome, got {other:?}"),
        }
    }

    #[test]
    fn parse_configure_frame() {
        let cfg = MatchConfig {
            width: 5,
            height: 5,
            max_turns: 100,
            walls: vec![],
            mud: vec![],
            cheese: vec![Coordinates::new(2, 2)],
            player1_start: Coordinates::new(0, 0),
            player2_start: Coordinates::new(4, 4),
            timing: TimingMode::Wait,
            move_timeout_ms: 1000,
            preprocessing_timeout_ms: 5000,
        };
        let bytes = pyrat_protocol::serialize_host_msg(&HostMsg::Configure {
            options: vec![("Hash".into(), "128".into())],
            match_config: Box::new(cfg),
        });
        match parse_host_frame(&bytes).unwrap() {
            HostMsg::Configure {
                options,
                match_config,
            } => {
                assert_eq!(options.len(), 1);
                assert_eq!(options[0].0, "Hash");
                assert_eq!(match_config.width, 5);
            },
            other => panic!("expected Configure, got {other:?}"),
        }
    }

    #[test]
    fn serialize_bot_msg_action() {
        let frame = serialize(&BotMsg::Action {
            direction: Direction::Right,
            player: Player::Player1,
            turn: 7,
            state_hash: 0xDEAD_BEEF,
            think_ms: 42,
        });
        let packet = flatbuffers::root::<wire::BotPacket>(&frame).unwrap();
        assert_eq!(packet.message_type(), wire::BotMessage::Action);
        let a = packet.message_as_action().unwrap();
        assert_eq!(a.player(), Player::Player1);
        assert_eq!(a.turn(), 7);
        assert_eq!(a.think_ms(), 42);
        assert_eq!(a.state_hash(), 0xDEAD_BEEF);
    }

    #[test]
    fn serialize_bot_msg_info() {
        let frame = serialize(&BotMsg::Info(Info {
            player: Player::Player2,
            multipv: 1,
            target: Some(Coordinates::new(3, 3)),
            depth: 5,
            nodes: 1000,
            score: Some(1.5),
            pv: vec![Direction::Up, Direction::Right],
            message: "depth 5".into(),
            turn: 10,
            state_hash: 0xCAFE,
        }));
        let packet = flatbuffers::root::<wire::BotPacket>(&frame).unwrap();
        assert_eq!(packet.message_type(), wire::BotMessage::Info);
    }
}
