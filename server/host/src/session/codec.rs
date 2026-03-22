//! FlatBuffers codec: borrowed→owned extraction and owned→bytes serialization.

use flatbuffers::FlatBufferBuilder;

use crate::session::messages::{HostCommand, OwnedInfo, OwnedMatchConfig, OwnedOptionDef};
use pyrat_wire::{self as wire, BotMessage, HostMessage, HostPacket, HostPacketArgs, Vec2};

// ── Helpers ─────────────────────────────────────────

fn vec2_to_tuple(v: &Vec2) -> (u8, u8) {
    (v.x(), v.y())
}

#[allow(dead_code)] // Used by extract_match_config; consumed by game loop later.
fn vec2_opt(v: Option<&Vec2>) -> (u8, u8) {
    v.map_or((0, 0), vec2_to_tuple)
}

// ── Extraction: borrowed FlatBuffers → owned types ──

/// Parsed bot message ready to be forwarded through channels.
pub enum BotPayload {
    Identify {
        name: String,
        author: String,
        options: Vec<OwnedOptionDef>,
        agent_id: String,
    },
    Ready,
    PreprocessingDone,
    Action {
        player: wire::Player,
        direction: wire::Direction,
        turn: u16,
        provisional: bool,
        think_ms: u32,
    },
    Pong,
    Info(OwnedInfo),
    RenderCommands,
}

/// Parse a raw frame payload as a BotPacket and extract owned data.
///
/// Returns `(BotMessage discriminant, payload)` or an error string.
pub fn extract_bot_packet(buf: &[u8]) -> Result<(BotMessage, BotPayload), String> {
    let packet = flatbuffers::root::<wire::BotPacket>(buf).map_err(|e| format!("{e}"))?;
    let msg_type = packet.message_type();

    let payload = if msg_type == BotMessage::Identify {
        let id = packet
            .message_as_identify()
            .ok_or("missing Identify body")?;
        let options = id
            .options()
            .map(|opts| {
                (0..opts.len())
                    .map(|i| {
                        let o = opts.get(i);
                        OwnedOptionDef {
                            name: o.name().unwrap_or("").to_owned(),
                            option_type: o.type_(),
                            default_value: o.default_value().unwrap_or("").to_owned(),
                            min: o.min(),
                            max: o.max(),
                            choices: o
                                .choices()
                                .map(|c| (0..c.len()).map(|j| c.get(j).to_owned()).collect())
                                .unwrap_or_default(),
                        }
                    })
                    .collect()
            })
            .unwrap_or_default();
        BotPayload::Identify {
            name: id.name().unwrap_or("").to_owned(),
            author: id.author().unwrap_or("").to_owned(),
            options,
            agent_id: id.agent_id().unwrap_or("").to_owned(),
        }
    } else if msg_type == BotMessage::Ready {
        BotPayload::Ready
    } else if msg_type == BotMessage::PreprocessingDone {
        BotPayload::PreprocessingDone
    } else if msg_type == BotMessage::Action {
        let a = packet.message_as_action().ok_or("missing Action body")?;
        BotPayload::Action {
            player: a.player(),
            direction: a.direction(),
            turn: a.turn(),
            provisional: a.provisional(),
            think_ms: a.think_ms(),
        }
    } else if msg_type == BotMessage::Pong {
        BotPayload::Pong
    } else if msg_type == BotMessage::Info {
        let info = packet.message_as_info().ok_or("missing Info body")?;
        BotPayload::Info(OwnedInfo {
            player: info.player(),
            multipv: info.multipv(),
            target: info.target().map(vec2_to_tuple),
            depth: info.depth(),
            nodes: info.nodes(),
            score: info.score(),
            pv: info.pv().map(|p| p.iter().collect()).unwrap_or_default(),
            message: info.message().unwrap_or("").to_owned(),
        })
    } else if msg_type == BotMessage::RenderCommands {
        BotPayload::RenderCommands
    } else {
        return Err(format!("unknown BotMessage type: {}", msg_type.0));
    };

    Ok((msg_type, payload))
}

// ── Serialization: HostCommand → FlatBuffer bytes ───

/// Serialize a `HostCommand` into a finished FlatBuffer, reusing the given builder.
///
/// Returns the finished bytes as a `Vec<u8>`. The builder is reset before use.
pub fn serialize_host_command(fbb: &mut FlatBufferBuilder<'_>, cmd: &HostCommand) -> Vec<u8> {
    fbb.reset();

    let (msg_type, msg_offset) = match cmd {
        HostCommand::SetOption { name, value } => {
            let name = fbb.create_string(name);
            let value = fbb.create_string(value);
            let off = wire::SetOption::create(
                fbb,
                &wire::SetOptionArgs {
                    name: Some(name),
                    value: Some(value),
                },
            );
            (HostMessage::SetOption, off.as_union_value())
        },
        HostCommand::MatchConfig(cfg) => {
            // Pre-create all vectors and strings before table creation.
            let walls: Vec<_> = cfg
                .walls
                .iter()
                .map(|&((x1, y1), (x2, y2))| {
                    wire::Wall::create(
                        fbb,
                        &wire::WallArgs {
                            pos1: Some(&Vec2::new(x1, y1)),
                            pos2: Some(&Vec2::new(x2, y2)),
                        },
                    )
                })
                .collect();
            let walls = fbb.create_vector(&walls);

            let muds: Vec<_> = cfg
                .mud
                .iter()
                .map(|&((x1, y1), (x2, y2), v)| {
                    wire::Mud::create(
                        fbb,
                        &wire::MudArgs {
                            pos1: Some(&Vec2::new(x1, y1)),
                            pos2: Some(&Vec2::new(x2, y2)),
                            value: v,
                        },
                    )
                })
                .collect();
            let muds = fbb.create_vector(&muds);

            let cheese_vec: Vec<Vec2> = cfg.cheese.iter().map(|&(x, y)| Vec2::new(x, y)).collect();
            let cheese = fbb.create_vector(&cheese_vec);

            let players = fbb.create_vector(&cfg.controlled_players);

            let off = wire::MatchConfig::create(
                fbb,
                &wire::MatchConfigArgs {
                    width: cfg.width,
                    height: cfg.height,
                    max_turns: cfg.max_turns,
                    walls: Some(walls),
                    mud: Some(muds),
                    cheese: Some(cheese),
                    player1_start: Some(&Vec2::new(cfg.player1_start.0, cfg.player1_start.1)),
                    player2_start: Some(&Vec2::new(cfg.player2_start.0, cfg.player2_start.1)),
                    controlled_players: Some(players),
                    timing: cfg.timing,
                    move_timeout_ms: cfg.move_timeout_ms,
                    preprocessing_timeout_ms: cfg.preprocessing_timeout_ms,
                },
            );
            (HostMessage::MatchConfig, off.as_union_value())
        },
        HostCommand::StartPreprocessing => {
            let off = wire::StartPreprocessing::create(fbb, &wire::StartPreprocessingArgs {});
            (HostMessage::StartPreprocessing, off.as_union_value())
        },
        HostCommand::TurnState(ts) => {
            let cheese_vec: Vec<Vec2> = ts.cheese.iter().map(|&(x, y)| Vec2::new(x, y)).collect();
            let cheese = fbb.create_vector(&cheese_vec);

            let off = wire::TurnState::create(
                fbb,
                &wire::TurnStateArgs {
                    turn: ts.turn,
                    player1_position: Some(&Vec2::new(
                        ts.player1_position.0,
                        ts.player1_position.1,
                    )),
                    player2_position: Some(&Vec2::new(
                        ts.player2_position.0,
                        ts.player2_position.1,
                    )),
                    player1_score: ts.player1_score,
                    player2_score: ts.player2_score,
                    player1_mud_turns: ts.player1_mud_turns,
                    player2_mud_turns: ts.player2_mud_turns,
                    cheese: Some(cheese),
                    player1_last_move: ts.player1_last_move,
                    player2_last_move: ts.player2_last_move,
                },
            );
            (HostMessage::TurnState, off.as_union_value())
        },
        HostCommand::Timeout { default_move } => {
            let off = wire::Timeout::create(
                fbb,
                &wire::TimeoutArgs {
                    default_move: *default_move,
                },
            );
            (HostMessage::Timeout, off.as_union_value())
        },
        HostCommand::GameOver {
            result,
            player1_score,
            player2_score,
        } => {
            let off = wire::GameOver::create(
                fbb,
                &wire::GameOverArgs {
                    result: *result,
                    player1_score: *player1_score,
                    player2_score: *player2_score,
                },
            );
            (HostMessage::GameOver, off.as_union_value())
        },
        HostCommand::Ping => {
            let off = wire::Ping::create(fbb, &wire::PingArgs {});
            (HostMessage::Ping, off.as_union_value())
        },
        HostCommand::Stop => {
            let off = wire::Stop::create(fbb, &wire::StopArgs {});
            (HostMessage::Stop, off.as_union_value())
        },
        HostCommand::Shutdown => {
            // Shutdown is session-internal — serializes the same as Stop on the wire,
            // but the session loop also enters drain mode.
            let off = wire::Stop::create(fbb, &wire::StopArgs {});
            (HostMessage::Stop, off.as_union_value())
        },
    };

    let packet = HostPacket::create(
        fbb,
        &HostPacketArgs {
            message_type: msg_type,
            message: Some(msg_offset),
        },
    );
    fbb.finish(packet, None);
    fbb.finished_data().to_vec()
}

// ── Round-trip helper for MatchConfig extraction ────

/// Extract an `OwnedMatchConfig` from a wire `MatchConfig` table.
#[allow(dead_code)] // Will be consumed by game loop; currently tested only.
pub fn extract_match_config(mc: &wire::MatchConfig<'_>) -> OwnedMatchConfig {
    OwnedMatchConfig {
        width: mc.width(),
        height: mc.height(),
        max_turns: mc.max_turns(),
        walls: mc
            .walls()
            .map(|ws| {
                (0..ws.len())
                    .map(|i| {
                        let w = ws.get(i);
                        (vec2_opt(w.pos1()), vec2_opt(w.pos2()))
                    })
                    .collect()
            })
            .unwrap_or_default(),
        mud: mc
            .mud()
            .map(|ms| {
                (0..ms.len())
                    .map(|i| {
                        let m = ms.get(i);
                        (vec2_opt(m.pos1()), vec2_opt(m.pos2()), m.value())
                    })
                    .collect()
            })
            .unwrap_or_default(),
        cheese: mc
            .cheese()
            .map(|cs| (0..cs.len()).map(|i| vec2_to_tuple(cs.get(i))).collect())
            .unwrap_or_default(),
        player1_start: vec2_opt(mc.player1_start()),
        player2_start: vec2_opt(mc.player2_start()),
        controlled_players: mc
            .controlled_players()
            .map(|ps| ps.iter().collect())
            .unwrap_or_default(),
        timing: mc.timing(),
        move_timeout_ms: mc.move_timeout_ms(),
        preprocessing_timeout_ms: mc.preprocessing_timeout_ms(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::messages::OwnedTurnState;
    use pyrat_wire::{Direction, GameResult, Player, TimingMode};

    // Helper: build a BotPacket with Identify
    fn build_identify(name: &str, author: &str) -> Vec<u8> {
        let mut fbb = FlatBufferBuilder::new();
        let name = fbb.create_string(name);
        let author = fbb.create_string(author);
        let id = wire::Identify::create(
            &mut fbb,
            &wire::IdentifyArgs {
                name: Some(name),
                author: Some(author),
                options: None,
                agent_id: None,
            },
        );
        let packet = wire::BotPacket::create(
            &mut fbb,
            &wire::BotPacketArgs {
                message_type: BotMessage::Identify,
                message: Some(id.as_union_value()),
            },
        );
        fbb.finish(packet, None);
        fbb.finished_data().to_vec()
    }

    fn build_action(direction: Direction, player: Player, turn: u16) -> Vec<u8> {
        let mut fbb = FlatBufferBuilder::new();
        let action = wire::Action::create(
            &mut fbb,
            &wire::ActionArgs {
                direction,
                player,
                turn,
                provisional: false,
                think_ms: 0,
            },
        );
        let packet = wire::BotPacket::create(
            &mut fbb,
            &wire::BotPacketArgs {
                message_type: BotMessage::Action,
                message: Some(action.as_union_value()),
            },
        );
        fbb.finish(packet, None);
        fbb.finished_data().to_vec()
    }

    fn build_ready() -> Vec<u8> {
        let mut fbb = FlatBufferBuilder::new();
        let ready = wire::Ready::create(&mut fbb, &wire::ReadyArgs {});
        let packet = wire::BotPacket::create(
            &mut fbb,
            &wire::BotPacketArgs {
                message_type: BotMessage::Ready,
                message: Some(ready.as_union_value()),
            },
        );
        fbb.finish(packet, None);
        fbb.finished_data().to_vec()
    }

    fn build_pong() -> Vec<u8> {
        let mut fbb = FlatBufferBuilder::new();
        let pong = wire::Pong::create(&mut fbb, &wire::PongArgs {});
        let packet = wire::BotPacket::create(
            &mut fbb,
            &wire::BotPacketArgs {
                message_type: BotMessage::Pong,
                message: Some(pong.as_union_value()),
            },
        );
        fbb.finish(packet, None);
        fbb.finished_data().to_vec()
    }

    fn build_preprocessing_done() -> Vec<u8> {
        let mut fbb = FlatBufferBuilder::new();
        let pd = wire::PreprocessingDone::create(&mut fbb, &wire::PreprocessingDoneArgs {});
        let packet = wire::BotPacket::create(
            &mut fbb,
            &wire::BotPacketArgs {
                message_type: BotMessage::PreprocessingDone,
                message: Some(pd.as_union_value()),
            },
        );
        fbb.finish(packet, None);
        fbb.finished_data().to_vec()
    }

    #[test]
    fn extract_identify() {
        let buf = build_identify("TestBot", "Author");
        let (msg_type, payload) = extract_bot_packet(&buf).unwrap();
        assert_eq!(msg_type, BotMessage::Identify);
        match payload {
            BotPayload::Identify {
                name,
                author,
                options,
                agent_id,
            } => {
                assert_eq!(name, "TestBot");
                assert_eq!(author, "Author");
                assert!(options.is_empty());
                assert!(agent_id.is_empty());
            },
            _ => panic!("expected Identify"),
        }
    }

    #[test]
    fn extract_action() {
        let buf = build_action(Direction::Left, Player::Player2, 7);
        let (msg_type, payload) = extract_bot_packet(&buf).unwrap();
        assert_eq!(msg_type, BotMessage::Action);
        match payload {
            BotPayload::Action {
                player,
                direction,
                turn,
                provisional,
                think_ms,
            } => {
                assert_eq!(player, Player::Player2);
                assert_eq!(direction, Direction::Left);
                assert_eq!(turn, 7);
                assert!(!provisional);
                assert_eq!(think_ms, 0);
            },
            _ => panic!("expected Action"),
        }
    }

    #[test]
    fn extract_ready() {
        let buf = build_ready();
        let (msg_type, _) = extract_bot_packet(&buf).unwrap();
        assert_eq!(msg_type, BotMessage::Ready);
    }

    #[test]
    fn extract_pong() {
        let buf = build_pong();
        let (msg_type, _) = extract_bot_packet(&buf).unwrap();
        assert_eq!(msg_type, BotMessage::Pong);
    }

    #[test]
    fn extract_preprocessing_done() {
        let buf = build_preprocessing_done();
        let (msg_type, _) = extract_bot_packet(&buf).unwrap();
        assert_eq!(msg_type, BotMessage::PreprocessingDone);
    }

    // ── Serialization round-trip ────────────────────

    #[test]
    fn round_trip_set_option() {
        let mut fbb = FlatBufferBuilder::new();
        let cmd = HostCommand::SetOption {
            name: "Hash".into(),
            value: "128".into(),
        };
        let bytes = serialize_host_command(&mut fbb, &cmd);
        let packet = flatbuffers::root::<HostPacket>(&bytes).unwrap();
        assert_eq!(packet.message_type(), HostMessage::SetOption);
        let so = packet.message_as_set_option().unwrap();
        assert_eq!(so.name(), Some("Hash"));
        assert_eq!(so.value(), Some("128"));
    }

    #[test]
    fn round_trip_match_config() {
        let mut fbb = FlatBufferBuilder::new();
        let cfg = OwnedMatchConfig {
            width: 21,
            height: 15,
            max_turns: 300,
            walls: vec![((0, 0), (0, 1)), ((1, 2), (2, 2))],
            mud: vec![((3, 3), (3, 4), 5)],
            cheese: vec![(10, 7), (5, 3)],
            player1_start: (20, 14),
            player2_start: (0, 0),
            controlled_players: vec![Player::Player1],
            timing: TimingMode::Wait,
            move_timeout_ms: 1000,
            preprocessing_timeout_ms: 5000,
        };
        let cmd = HostCommand::MatchConfig(Box::new(cfg));
        let bytes = serialize_host_command(&mut fbb, &cmd);

        // Parse it back.
        let packet = flatbuffers::root::<HostPacket>(&bytes).unwrap();
        assert_eq!(packet.message_type(), HostMessage::MatchConfig);
        let mc = packet.message_as_match_config().unwrap();
        assert_eq!(mc.width(), 21);
        assert_eq!(mc.height(), 15);
        assert_eq!(mc.max_turns(), 300);
        assert_eq!(mc.walls().unwrap().len(), 2);
        assert_eq!(mc.mud().unwrap().len(), 1);
        assert_eq!(mc.cheese().unwrap().len(), 2);
        assert_eq!(mc.player1_start().unwrap().x(), 20);
        assert_eq!(mc.player1_start().unwrap().y(), 14);
        assert_eq!(mc.move_timeout_ms(), 1000);
        assert_eq!(mc.preprocessing_timeout_ms(), 5000);

        // Also test extract_match_config round-trip.
        let owned = extract_match_config(&mc);
        assert_eq!(owned.width, 21);
        assert_eq!(owned.walls.len(), 2);
        assert_eq!(owned.controlled_players, vec![Player::Player1]);
    }

    #[test]
    fn round_trip_turn_state() {
        let mut fbb = FlatBufferBuilder::new();
        let ts = OwnedTurnState {
            turn: 42,
            player1_position: (10, 7),
            player2_position: (0, 0),
            player1_score: 3.0,
            player2_score: 2.5,
            player1_mud_turns: 0,
            player2_mud_turns: 2,
            cheese: vec![(5, 5), (15, 10)],
            player1_last_move: Direction::Up,
            player2_last_move: Direction::Right,
        };
        let cmd = HostCommand::TurnState(Box::new(ts));
        let bytes = serialize_host_command(&mut fbb, &cmd);

        let packet = flatbuffers::root::<HostPacket>(&bytes).unwrap();
        assert_eq!(packet.message_type(), HostMessage::TurnState);
        let ts = packet.message_as_turn_state().unwrap();
        assert_eq!(ts.turn(), 42);
        assert_eq!(ts.player1_position().unwrap().x(), 10);
        assert_eq!(ts.player2_score(), 2.5);
        assert_eq!(ts.cheese().unwrap().len(), 2);
    }

    #[test]
    fn round_trip_game_over() {
        let mut fbb = FlatBufferBuilder::new();
        let cmd = HostCommand::GameOver {
            result: GameResult::Draw,
            player1_score: 5.0,
            player2_score: 5.0,
        };
        let bytes = serialize_host_command(&mut fbb, &cmd);
        let packet = flatbuffers::root::<HostPacket>(&bytes).unwrap();
        assert_eq!(packet.message_type(), HostMessage::GameOver);
        let go = packet.message_as_game_over().unwrap();
        assert_eq!(go.result(), GameResult::Draw);
        assert_eq!(go.player1_score(), 5.0);
    }

    #[test]
    fn round_trip_ping() {
        let mut fbb = FlatBufferBuilder::new();
        let bytes = serialize_host_command(&mut fbb, &HostCommand::Ping);
        let packet = flatbuffers::root::<HostPacket>(&bytes).unwrap();
        assert_eq!(packet.message_type(), HostMessage::Ping);
    }

    #[test]
    fn round_trip_stop() {
        let mut fbb = FlatBufferBuilder::new();
        let bytes = serialize_host_command(&mut fbb, &HostCommand::Stop);
        let packet = flatbuffers::root::<HostPacket>(&bytes).unwrap();
        assert_eq!(packet.message_type(), HostMessage::Stop);
    }

    #[test]
    fn round_trip_shutdown_serializes_as_stop() {
        let mut fbb = FlatBufferBuilder::new();
        let bytes = serialize_host_command(&mut fbb, &HostCommand::Shutdown);
        let packet = flatbuffers::root::<HostPacket>(&bytes).unwrap();
        assert_eq!(packet.message_type(), HostMessage::Stop);
    }

    #[test]
    fn round_trip_timeout() {
        let mut fbb = FlatBufferBuilder::new();
        let cmd = HostCommand::Timeout {
            default_move: Direction::Stay,
        };
        let bytes = serialize_host_command(&mut fbb, &cmd);
        let packet = flatbuffers::root::<HostPacket>(&bytes).unwrap();
        assert_eq!(packet.message_type(), HostMessage::Timeout);
        let t = packet.message_as_timeout().unwrap();
        assert_eq!(t.default_move(), Direction::Stay);
    }

    // ── Info extraction round-trip ────────────────────

    #[allow(clippy::too_many_arguments)]
    fn build_info(
        player: Player,
        multipv: u16,
        target: Option<(u8, u8)>,
        depth: u16,
        nodes: u32,
        score: Option<f32>,
        pv: &[Direction],
        message: &str,
    ) -> Vec<u8> {
        let mut fbb = FlatBufferBuilder::new();

        let msg = if message.is_empty() {
            None
        } else {
            Some(fbb.create_string(message))
        };
        let pv_off = if pv.is_empty() {
            None
        } else {
            Some(fbb.create_vector(pv))
        };
        let target_v = target.map(|(x, y)| Vec2::new(x, y));

        let info = wire::Info::create(
            &mut fbb,
            &wire::InfoArgs {
                player,
                multipv,
                target: target_v.as_ref(),
                depth,
                nodes,
                score,
                pv: pv_off,
                message: msg,
            },
        );
        let packet = wire::BotPacket::create(
            &mut fbb,
            &wire::BotPacketArgs {
                message_type: BotMessage::Info,
                message: Some(info.as_union_value()),
            },
        );
        fbb.finish(packet, None);
        fbb.finished_data().to_vec()
    }

    #[test]
    fn extract_info_all_fields() {
        let buf = build_info(
            Player::Player2,
            3,
            Some((10, 7)),
            5,
            42000,
            Some(2.5),
            &[Direction::Up, Direction::Left],
            "depth 5",
        );
        let (msg_type, payload) = extract_bot_packet(&buf).unwrap();
        assert_eq!(msg_type, BotMessage::Info);
        match payload {
            BotPayload::Info(info) => {
                assert_eq!(info.player, Player::Player2);
                assert_eq!(info.multipv, 3);
                assert_eq!(info.target, Some((10, 7)));
                assert_eq!(info.depth, 5);
                assert_eq!(info.nodes, 42000);
                assert!((info.score.unwrap() - 2.5).abs() < f32::EPSILON);
                assert_eq!(info.pv, vec![Direction::Up, Direction::Left]);
                assert_eq!(info.message, "depth 5");
            },
            _ => panic!("expected Info"),
        }
    }

    #[test]
    fn extract_info_empty_optional_fields() {
        let buf = build_info(Player::Player1, 0, None, 0, 0, None, &[], "");
        let (_, payload) = extract_bot_packet(&buf).unwrap();
        match payload {
            BotPayload::Info(info) => {
                assert_eq!(info.player, Player::Player1);
                assert_eq!(info.multipv, 0);
                assert!(info.target.is_none());
                assert!(info.pv.is_empty());
                assert!(info.message.is_empty());
            },
            _ => panic!("expected Info"),
        }
    }

    #[test]
    fn fbb_reuse_across_calls() {
        let mut fbb = FlatBufferBuilder::new();
        let _ = serialize_host_command(&mut fbb, &HostCommand::Ping);
        let _ = serialize_host_command(&mut fbb, &HostCommand::Shutdown);
        // Just verifying no panic — builder resets cleanly.
        let bytes = serialize_host_command(
            &mut fbb,
            &HostCommand::Timeout {
                default_move: Direction::Down,
            },
        );
        let packet = flatbuffers::root::<HostPacket>(&bytes).unwrap();
        assert_eq!(packet.message_type(), HostMessage::Timeout);
    }
}
