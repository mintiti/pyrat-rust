//! Wire codec: extract HostPackets to owned types, build BotPackets.
//!
//! Extraction of per-table data is delegated to `pyrat_protocol::codec`.
//! This module handles packet dispatch and bot packet builders.

use flatbuffers::FlatBufferBuilder;
use pyrat_protocol::{
    engine_to_wire_direction, extract_game_over, extract_match_config, extract_turn_state,
    wire_to_engine_direction, GameOver, HashedTurnState, MatchConfig,
};
use pyrat_wire::{self as wire, BotMessage, HostMessage, Vec2};

// ── Extracted host messages ──────────────────────────

/// Parsed host message.
#[derive(Debug)]
#[allow(dead_code)] // Fields are extracted for completeness; not all are consumed.
pub enum HostMsg {
    SetOption { name: String, value: String },
    MatchConfig(MatchConfig),
    StartPreprocessing { state_hash: u64 },
    TurnState(HashedTurnState),
    Timeout { default_move: pyrat::Direction },
    GameOver(GameOver),
    Ping,
    Stop,
}

// ── Extraction ───────────────────────────────────────

/// Parse a raw frame as a HostPacket and extract to an owned `HostMsg`.
pub fn extract_host_msg(buf: &[u8]) -> Result<HostMsg, String> {
    let packet = flatbuffers::root::<wire::HostPacket>(buf).map_err(|e| format!("{e}"))?;
    let msg_type = packet.message_type();

    match msg_type {
        HostMessage::SetOption => {
            let so = packet
                .message_as_set_option()
                .ok_or("missing SetOption body")?;
            Ok(HostMsg::SetOption {
                name: so.name().unwrap_or("").to_owned(),
                value: so.value().unwrap_or("").to_owned(),
            })
        },
        HostMessage::MatchConfig => {
            let mc = packet
                .message_as_match_config()
                .ok_or("missing MatchConfig body")?;
            Ok(HostMsg::MatchConfig(extract_match_config(&mc)))
        },
        HostMessage::StartPreprocessing => {
            let sp = packet
                .message_as_start_preprocessing()
                .ok_or("missing StartPreprocessing body")?;
            Ok(HostMsg::StartPreprocessing {
                state_hash: sp.state_hash(),
            })
        },
        HostMessage::TurnState => {
            let ts = packet
                .message_as_turn_state()
                .ok_or("missing TurnState body")?;
            Ok(HostMsg::TurnState(extract_turn_state(&ts)))
        },
        HostMessage::Timeout => {
            let t = packet.message_as_timeout().ok_or("missing Timeout body")?;
            Ok(HostMsg::Timeout {
                default_move: wire_to_engine_direction(t.default_move()),
            })
        },
        HostMessage::GameOver => {
            let go = packet
                .message_as_game_over()
                .ok_or("missing GameOver body")?;
            Ok(HostMsg::GameOver(extract_game_over(&go)))
        },
        HostMessage::Ping => Ok(HostMsg::Ping),
        HostMessage::Stop => Ok(HostMsg::Stop),
        _ => Err(format!("unknown HostMessage type: {}", msg_type.0)),
    }
}

// ── Bot packet builders ──────────────────────────────

fn build_bot_frame<F>(msg_type: BotMessage, build_msg: F) -> Vec<u8>
where
    F: FnOnce(&mut FlatBufferBuilder) -> flatbuffers::WIPOffset<flatbuffers::UnionWIPOffset>,
{
    let mut fbb = FlatBufferBuilder::new();
    let msg_offset = build_msg(&mut fbb);
    let packet = wire::BotPacket::create(
        &mut fbb,
        &wire::BotPacketArgs {
            message_type: msg_type,
            message: Some(msg_offset),
        },
    );
    fbb.finish(packet, None);
    fbb.finished_data().to_vec()
}

/// Build an Identify bot packet.
pub fn build_identify(
    name: &str,
    author: &str,
    agent_id: &str,
    option_defs: &[crate::options::SdkOptionDef],
) -> Vec<u8> {
    build_bot_frame(BotMessage::Identify, |fbb| {
        let n = fbb.create_string(name);
        let a = fbb.create_string(author);
        let aid = if agent_id.is_empty() {
            None
        } else {
            Some(fbb.create_string(agent_id))
        };

        let opts = if option_defs.is_empty() {
            None
        } else {
            let defs: Vec<_> = option_defs
                .iter()
                .map(|def| {
                    let opt_name = fbb.create_string(&def.name);
                    let default = fbb.create_string(&def.default_value);
                    let choices = if def.choices.is_empty() {
                        None
                    } else {
                        let strs: Vec<_> =
                            def.choices.iter().map(|s| fbb.create_string(s)).collect();
                        Some(fbb.create_vector(&strs))
                    };
                    wire::OptionDef::create(
                        fbb,
                        &wire::OptionDefArgs {
                            name: Some(opt_name),
                            type_: def.option_type,
                            default_value: Some(default),
                            min: def.min,
                            max: def.max,
                            choices,
                        },
                    )
                })
                .collect();
            Some(fbb.create_vector(&defs))
        };

        wire::Identify::create(
            fbb,
            &wire::IdentifyArgs {
                name: Some(n),
                author: Some(a),
                options: opts,
                agent_id: aid,
            },
        )
        .as_union_value()
    })
}

/// Build a Ready bot packet.
pub fn build_ready() -> Vec<u8> {
    build_bot_frame(BotMessage::Ready, |fbb| {
        wire::Ready::create(fbb, &wire::ReadyArgs {}).as_union_value()
    })
}

/// Build a PreprocessingDone bot packet.
pub fn build_preprocessing_done() -> Vec<u8> {
    build_bot_frame(BotMessage::PreprocessingDone, |fbb| {
        wire::PreprocessingDone::create(fbb, &wire::PreprocessingDoneArgs {}).as_union_value()
    })
}

/// Build a Pong bot packet.
pub fn build_pong() -> Vec<u8> {
    build_bot_frame(BotMessage::Pong, |fbb| {
        wire::Pong::create(fbb, &wire::PongArgs {}).as_union_value()
    })
}

/// Build an Action bot packet (convenience — committed, think_ms=0).
#[allow(dead_code)]
pub fn build_action(player: wire::Player, direction: pyrat::Direction, turn: u16) -> Vec<u8> {
    build_action_full(player, direction, turn, false, 0)
}

/// Build an Action bot packet with provisional and think_ms fields.
pub fn build_action_full(
    player: wire::Player,
    direction: pyrat::Direction,
    turn: u16,
    provisional: bool,
    think_ms: u32,
) -> Vec<u8> {
    let wire_dir = engine_to_wire_direction(direction);
    build_bot_frame(BotMessage::Action, move |fbb| {
        wire::Action::create(
            fbb,
            &wire::ActionArgs {
                direction: wire_dir,
                player,
                turn,
                provisional,
                think_ms,
            },
        )
        .as_union_value()
    })
}

/// Build an Info bot packet (search telemetry for GUI / debugging).
#[allow(clippy::too_many_arguments)]
pub fn build_info(
    player: wire::Player,
    multipv: u16,
    target: Option<pyrat::Coordinates>,
    depth: u16,
    nodes: u32,
    score: Option<f32>,
    pv: &[pyrat::Direction],
    message: &str,
    turn: u16,
    state_hash: u64,
) -> Vec<u8> {
    build_bot_frame(BotMessage::Info, |fbb| {
        let msg = if message.is_empty() {
            None
        } else {
            Some(fbb.create_string(message))
        };

        let pv_vec: Vec<wire::Direction> =
            pv.iter().map(|&d| engine_to_wire_direction(d)).collect();
        let pv_off = if pv_vec.is_empty() {
            None
        } else {
            Some(fbb.create_vector(&pv_vec))
        };

        let target_v = target.map(|c| Vec2::new(c.x, c.y));

        wire::Info::create(
            fbb,
            &wire::InfoArgs {
                player,
                multipv,
                target: target_v.as_ref(),
                depth,
                nodes,
                score,
                pv: pv_off,
                message: msg,
                turn,
                state_hash,
            },
        )
        .as_union_value()
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use pyrat::Coordinates;
    use pyrat_wire::{Direction as WireDir, GameResult, Player, TimingMode};

    fn build_host_packet<F>(msg_type: HostMessage, build_msg: F) -> Vec<u8>
    where
        F: FnOnce(&mut FlatBufferBuilder) -> flatbuffers::WIPOffset<flatbuffers::UnionWIPOffset>,
    {
        let mut fbb = FlatBufferBuilder::new();
        let msg_offset = build_msg(&mut fbb);
        let packet = wire::HostPacket::create(
            &mut fbb,
            &wire::HostPacketArgs {
                message_type: msg_type,
                message: Some(msg_offset),
            },
        );
        fbb.finish(packet, None);
        fbb.finished_data().to_vec()
    }

    #[test]
    fn extract_set_option() {
        let buf = build_host_packet(HostMessage::SetOption, |fbb| {
            let n = fbb.create_string("Hash");
            let v = fbb.create_string("128");
            wire::SetOption::create(
                fbb,
                &wire::SetOptionArgs {
                    name: Some(n),
                    value: Some(v),
                },
            )
            .as_union_value()
        });
        match extract_host_msg(&buf).unwrap() {
            HostMsg::SetOption { name, value } => {
                assert_eq!(name, "Hash");
                assert_eq!(value, "128");
            },
            _ => panic!("expected SetOption"),
        }
    }

    #[test]
    fn extract_match_config_roundtrip() {
        let buf = build_host_packet(HostMessage::MatchConfig, |fbb| {
            let walls_data = vec![wire::Wall::create(
                fbb,
                &wire::WallArgs {
                    pos1: Some(&Vec2::new(0, 0)),
                    pos2: Some(&Vec2::new(0, 1)),
                },
            )];
            let walls = fbb.create_vector(&walls_data);

            let muds_data = vec![wire::Mud::create(
                fbb,
                &wire::MudArgs {
                    pos1: Some(&Vec2::new(3, 3)),
                    pos2: Some(&Vec2::new(3, 4)),
                    value: 5,
                },
            )];
            let muds = fbb.create_vector(&muds_data);

            let cheese_vec = vec![Vec2::new(10, 7), Vec2::new(5, 3)];
            let cheese = fbb.create_vector(&cheese_vec);

            let players = fbb.create_vector(&[Player::Player1]);

            wire::MatchConfig::create(
                fbb,
                &wire::MatchConfigArgs {
                    width: 21,
                    height: 15,
                    max_turns: 300,
                    walls: Some(walls),
                    mud: Some(muds),
                    cheese: Some(cheese),
                    player1_start: Some(&Vec2::new(20, 14)),
                    player2_start: Some(&Vec2::new(0, 0)),
                    controlled_players: Some(players),
                    timing: TimingMode::Wait,
                    move_timeout_ms: 1000,
                    preprocessing_timeout_ms: 5000,
                },
            )
            .as_union_value()
        });

        match extract_host_msg(&buf).unwrap() {
            HostMsg::MatchConfig(cfg) => {
                assert_eq!(cfg.width, 21);
                assert_eq!(cfg.height, 15);
                assert_eq!(cfg.max_turns, 300);
                assert_eq!(cfg.walls.len(), 1);
                assert_eq!(cfg.walls[0].0, Coordinates::new(0, 0));
                assert_eq!(cfg.walls[0].1, Coordinates::new(0, 1));
                assert_eq!(cfg.mud.len(), 1);
                assert_eq!(cfg.mud[0].turns, 5);
                assert_eq!(cfg.cheese.len(), 2);
                assert_eq!(cfg.player1_start, Coordinates::new(20, 14));
                assert_eq!(cfg.player2_start, Coordinates::new(0, 0));
                assert_eq!(cfg.controlled_players, vec![Player::Player1]);
                assert_eq!(cfg.move_timeout_ms, 1000);
                assert_eq!(cfg.preprocessing_timeout_ms, 5000);
            },
            _ => panic!("expected MatchConfig"),
        }
    }

    #[test]
    fn extract_turn_state_roundtrip() {
        let buf = build_host_packet(HostMessage::TurnState, |fbb| {
            let cheese_vec = vec![Vec2::new(5, 5), Vec2::new(15, 10)];
            let cheese = fbb.create_vector(&cheese_vec);
            wire::TurnState::create(
                fbb,
                &wire::TurnStateArgs {
                    turn: 42,
                    player1_position: Some(&Vec2::new(10, 7)),
                    player2_position: Some(&Vec2::new(0, 0)),
                    player1_score: 3.0,
                    player2_score: 2.5,
                    player1_mud_turns: 0,
                    player2_mud_turns: 2,
                    cheese: Some(cheese),
                    player1_last_move: WireDir::Up,
                    player2_last_move: WireDir::Right,
                    state_hash: 0xFEED_FACE_1234_5678,
                },
            )
            .as_union_value()
        });

        match extract_host_msg(&buf).unwrap() {
            HostMsg::TurnState(hts) => {
                assert_eq!(hts.turn, 42);
                assert_eq!(hts.player1_position, Coordinates::new(10, 7));
                assert_eq!(hts.player2_position, Coordinates::new(0, 0));
                assert!((hts.player1_score - 3.0).abs() < f32::EPSILON);
                assert!((hts.player2_score - 2.5).abs() < f32::EPSILON);
                assert_eq!(hts.player1_mud_turns, 0);
                assert_eq!(hts.player2_mud_turns, 2);
                assert_eq!(hts.cheese.len(), 2);
                assert_eq!(hts.player1_last_move, pyrat::Direction::Up);
                assert_eq!(hts.player2_last_move, pyrat::Direction::Right);
                assert_eq!(hts.state_hash(), 0xFEED_FACE_1234_5678);
            },
            _ => panic!("expected TurnState"),
        }
    }

    #[test]
    fn extract_game_over_test() {
        let buf = build_host_packet(HostMessage::GameOver, |fbb| {
            wire::GameOver::create(
                fbb,
                &wire::GameOverArgs {
                    result: GameResult::Draw,
                    player1_score: 5.0,
                    player2_score: 5.0,
                },
            )
            .as_union_value()
        });

        match extract_host_msg(&buf).unwrap() {
            HostMsg::GameOver(go) => {
                assert_eq!(go.result, GameResult::Draw);
                assert!((go.player1_score - 5.0).abs() < f32::EPSILON);
            },
            _ => panic!("expected GameOver"),
        }
    }

    #[test]
    fn extract_ping() {
        let buf = build_host_packet(HostMessage::Ping, |fbb| {
            wire::Ping::create(fbb, &wire::PingArgs {}).as_union_value()
        });
        assert!(matches!(extract_host_msg(&buf).unwrap(), HostMsg::Ping));
    }

    #[test]
    fn extract_stop() {
        let buf = build_host_packet(HostMessage::Stop, |fbb| {
            wire::Stop::create(fbb, &wire::StopArgs {}).as_union_value()
        });
        assert!(matches!(extract_host_msg(&buf).unwrap(), HostMsg::Stop));
    }

    #[test]
    fn extract_start_preprocessing() {
        let buf = build_host_packet(HostMessage::StartPreprocessing, |fbb| {
            wire::StartPreprocessing::create(
                fbb,
                &wire::StartPreprocessingArgs {
                    state_hash: 0xDEAD_BEEF,
                },
            )
            .as_union_value()
        });
        match extract_host_msg(&buf).unwrap() {
            HostMsg::StartPreprocessing { state_hash } => {
                assert_eq!(state_hash, 0xDEAD_BEEF);
            },
            _ => panic!("expected StartPreprocessing"),
        }
    }

    #[test]
    fn extract_timeout() {
        let buf = build_host_packet(HostMessage::Timeout, |fbb| {
            wire::Timeout::create(
                fbb,
                &wire::TimeoutArgs {
                    default_move: WireDir::Stay,
                },
            )
            .as_union_value()
        });
        match extract_host_msg(&buf).unwrap() {
            HostMsg::Timeout { default_move } => {
                assert_eq!(default_move, pyrat::Direction::Stay);
            },
            _ => panic!("expected Timeout"),
        }
    }

    #[test]
    fn build_and_extract_action_roundtrip() {
        let bytes = build_action(Player::Player1, pyrat::Direction::Left, 42);
        let packet = flatbuffers::root::<wire::BotPacket>(&bytes).unwrap();
        assert_eq!(packet.message_type(), BotMessage::Action);
        let action = packet.message_as_action().unwrap();
        assert_eq!(action.player(), Player::Player1);
        assert_eq!(action.direction(), WireDir::Left);
        assert_eq!(action.turn(), 42);
    }

    #[test]
    fn build_identify_roundtrip() {
        let bytes = build_identify("TestBot", "Author", "agent-1", &[]);
        let packet = flatbuffers::root::<wire::BotPacket>(&bytes).unwrap();
        assert_eq!(packet.message_type(), BotMessage::Identify);
        let id = packet.message_as_identify().unwrap();
        assert_eq!(id.name(), Some("TestBot"));
        assert_eq!(id.author(), Some("Author"));
        assert_eq!(id.agent_id(), Some("agent-1"));
    }

    #[test]
    fn build_ready_roundtrip() {
        let bytes = build_ready();
        let packet = flatbuffers::root::<wire::BotPacket>(&bytes).unwrap();
        assert_eq!(packet.message_type(), BotMessage::Ready);
    }

    #[test]
    fn build_pong_roundtrip() {
        let bytes = build_pong();
        let packet = flatbuffers::root::<wire::BotPacket>(&bytes).unwrap();
        assert_eq!(packet.message_type(), BotMessage::Pong);
    }

    #[test]
    fn build_preprocessing_done_roundtrip() {
        let bytes = build_preprocessing_done();
        let packet = flatbuffers::root::<wire::BotPacket>(&bytes).unwrap();
        assert_eq!(packet.message_type(), BotMessage::PreprocessingDone);
    }

    #[test]
    fn build_info_roundtrip() {
        let bytes = build_info(
            Player::Player2,
            3,
            Some(Coordinates::new(10, 7)),
            5,
            42000,
            Some(2.5),
            &[pyrat::Direction::Up, pyrat::Direction::Left],
            "depth 5",
            7,
            0xCAFE_BABE,
        );
        let packet = flatbuffers::root::<wire::BotPacket>(&bytes).unwrap();
        assert_eq!(packet.message_type(), BotMessage::Info);
        let info = packet.message_as_info().unwrap();
        assert_eq!(info.player(), Player::Player2);
        assert_eq!(info.multipv(), 3);
        let t = info.target().unwrap();
        assert_eq!((t.x(), t.y()), (10, 7));
        assert_eq!(info.depth(), 5);
        assert_eq!(info.nodes(), 42000);
        assert!((info.score().unwrap() - 2.5).abs() < f32::EPSILON);
        let pv = info.pv().unwrap();
        assert_eq!(pv.len(), 2);
        assert_eq!(pv.get(0), WireDir::Up);
        assert_eq!(pv.get(1), WireDir::Left);
        assert_eq!(info.message(), Some("depth 5"));
        assert_eq!(info.turn(), 7);
        assert_eq!(info.state_hash(), 0xCAFE_BABE);
    }

    #[test]
    fn build_info_empty_optional_fields() {
        let bytes = build_info(Player::Player1, 0, None, 0, 0, None, &[], "", 0, 0);
        let packet = flatbuffers::root::<wire::BotPacket>(&bytes).unwrap();
        assert_eq!(packet.message_type(), BotMessage::Info);
        let info = packet.message_as_info().unwrap();
        assert_eq!(info.player(), Player::Player1);
        assert_eq!(info.multipv(), 0);
        assert!(info.target().is_none());
        assert!(info.pv().is_none());
        assert!(info.message().is_none());
    }
}
