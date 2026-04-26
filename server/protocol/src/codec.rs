//! FlatBuffers → owned type extraction.
//!
//! Shared extraction functions used by both the host and SDK codecs.
//! Host receives `BotPacket`, SDK receives `HostPacket` — the message-type
//! sets are disjoint, so dispatch is local. The per-table extraction logic
//! is identical and lives here.
//!
//! This module covers the **extraction direction only** (wire → owned).
//! Serialization (owned → wire) stays in each consumer because it's coupled
//! to their local enum structure (`HostCommand`, bot builders). [`coords_to_vec2`]
//! is the one serialization-side helper that lives here, as a mirror of
//! [`vec2_to_coords`].

use pyrat::Coordinates;
use pyrat_wire::{self as wire, Vec2};

use crate::{
    wire_to_engine_direction, HashedTurnState, MudEntry, OwnedGameOver, OwnedInfo,
    OwnedMatchConfig, OwnedOptionDef, OwnedTurnState,
};

// ── Coordinate helpers ──────────────────────────────

/// Convert a wire `Vec2` to engine `Coordinates`.
pub(crate) fn vec2_to_coords(v: &Vec2) -> Coordinates {
    Coordinates::new(v.x(), v.y())
}

/// Extract a required wire `Vec2` as engine `Coordinates`.
///
/// Panics in debug builds if the field is missing; returns `(0, 0)` in release
/// builds (the schema marks these fields required, so `None` indicates a
/// protocol violation that should not occur in practice).
pub(crate) fn vec2_opt(v: Option<&Vec2>) -> Coordinates {
    debug_assert!(v.is_some(), "expected required Vec2 field, got None");
    v.map_or(Coordinates::new(0, 0), vec2_to_coords)
}

/// Convert engine `Coordinates` to a wire `Vec2`.
///
/// Mirror of [`vec2_to_coords`]. Use at the serialization boundary.
pub fn coords_to_vec2(c: Coordinates) -> Vec2 {
    Vec2::new(c.x, c.y)
}

// ── Extraction functions ────────────────────────────

/// Extract an [`OwnedMatchConfig`] from a wire `MatchConfig` table.
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
                        MudEntry {
                            pos1: vec2_opt(m.pos1()),
                            pos2: vec2_opt(m.pos2()),
                            turns: m.value(),
                        }
                    })
                    .collect()
            })
            .unwrap_or_default(),
        cheese: mc
            .cheese()
            .map(|cs| (0..cs.len()).map(|i| vec2_to_coords(cs.get(i))).collect())
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

/// Extract a [`HashedTurnState`] from a wire `TurnState` table.
///
/// Trusts the wire-provided `state_hash` rather than recomputing it.
/// The host computed the hash from the same fields; recomputing would be
/// wasteful and would couple the consumer to the hash algorithm.
///
/// The hash is a correlation tag, not a trust boundary. Host/SDK agreement
/// on initial state is verified once at the setup-phase handshake (see the
/// SDK's `compute_initial_hash`); per-turn hashes ride along on the wire
/// after that.
pub fn extract_turn_state(ts: &wire::TurnState<'_>) -> HashedTurnState {
    let owned = OwnedTurnState {
        turn: ts.turn(),
        player1_position: vec2_opt(ts.player1_position()),
        player2_position: vec2_opt(ts.player2_position()),
        player1_score: ts.player1_score(),
        player2_score: ts.player2_score(),
        player1_mud_turns: ts.player1_mud_turns(),
        player2_mud_turns: ts.player2_mud_turns(),
        cheese: ts
            .cheese()
            .map(|cs| (0..cs.len()).map(|i| vec2_to_coords(cs.get(i))).collect())
            .unwrap_or_default(),
        player1_last_move: wire_to_engine_direction(ts.player1_last_move()),
        player2_last_move: wire_to_engine_direction(ts.player2_last_move()),
    };
    HashedTurnState::with_unverified_hash(owned, ts.state_hash())
}

/// Extract an [`OwnedInfo`] from a wire `Info` table.
pub fn extract_info(info: &wire::Info<'_>) -> OwnedInfo {
    OwnedInfo {
        player: info.player(),
        multipv: info.multipv(),
        target: info.target().map(vec2_to_coords),
        depth: info.depth(),
        nodes: info.nodes(),
        score: info.score(),
        pv: info
            .pv()
            .map(|p| p.iter().map(wire_to_engine_direction).collect())
            .unwrap_or_default(),
        message: info.message().unwrap_or("").to_owned(),
        turn: info.turn(),
        state_hash: info.state_hash(),
    }
}

/// Extract an [`OwnedGameOver`] from a wire `GameOver` table.
pub fn extract_game_over(go: &wire::GameOver<'_>) -> OwnedGameOver {
    OwnedGameOver {
        result: go.result(),
        player1_score: go.player1_score(),
        player2_score: go.player2_score(),
    }
}

/// Extract option definitions from a FlatBuffers vector of `OptionDef` tables.
pub fn extract_option_defs(
    opts: flatbuffers::Vector<'_, flatbuffers::ForwardsUOffset<wire::OptionDef<'_>>>,
) -> Vec<OwnedOptionDef> {
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use flatbuffers::FlatBufferBuilder;
    use pyrat_wire::{
        Direction as WireDir, GameResult, HostMessage, HostPacket, HostPacketArgs, Player,
        TimingMode,
    };

    // ── MatchConfig ─────────────────────────────────

    #[test]
    fn extract_match_config_roundtrip() {
        let mut fbb = FlatBufferBuilder::new();

        let walls_data = vec![wire::Wall::create(
            &mut fbb,
            &wire::WallArgs {
                pos1: Some(&Vec2::new(0, 0)),
                pos2: Some(&Vec2::new(0, 1)),
            },
        )];
        let walls = fbb.create_vector(&walls_data);

        let muds_data = vec![wire::Mud::create(
            &mut fbb,
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

        let mc = wire::MatchConfig::create(
            &mut fbb,
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
        );
        fbb.finish(mc, None);
        let buf = fbb.finished_data();
        let mc = flatbuffers::root::<wire::MatchConfig>(buf).unwrap();

        let cfg = extract_match_config(&mc);
        assert_eq!(cfg.width, 21);
        assert_eq!(cfg.height, 15);
        assert_eq!(cfg.max_turns, 300);
        assert_eq!(cfg.walls.len(), 1);
        assert_eq!(cfg.walls[0].0, Coordinates::new(0, 0));
        assert_eq!(cfg.walls[0].1, Coordinates::new(0, 1));
        assert_eq!(cfg.mud.len(), 1);
        assert_eq!(cfg.mud[0].turns, 5);
        assert_eq!(cfg.cheese.len(), 2);
        assert_eq!(cfg.cheese[0], Coordinates::new(10, 7));
        assert_eq!(cfg.player1_start, Coordinates::new(20, 14));
        assert_eq!(cfg.player2_start, Coordinates::new(0, 0));
        assert_eq!(cfg.controlled_players, vec![Player::Player1]);
        assert_eq!(cfg.move_timeout_ms, 1000);
        assert_eq!(cfg.preprocessing_timeout_ms, 5000);
    }

    // ── TurnState ───────────────────────────────────

    #[test]
    fn extract_turn_state_roundtrip() {
        let mut fbb = FlatBufferBuilder::new();

        let cheese_vec = vec![Vec2::new(5, 5), Vec2::new(15, 10)];
        let cheese = fbb.create_vector(&cheese_vec);

        let ts = wire::TurnState::create(
            &mut fbb,
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
        );
        fbb.finish(ts, None);
        let buf = fbb.finished_data();
        let ts = flatbuffers::root::<wire::TurnState>(buf).unwrap();

        let hts = extract_turn_state(&ts);
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
    }

    // ── Info ────────────────────────────────────────

    #[allow(clippy::too_many_arguments)]
    fn build_wire_info(
        fbb: &mut FlatBufferBuilder<'_>,
        player: Player,
        multipv: u16,
        target: Option<Coordinates>,
        depth: u16,
        nodes: u32,
        score: Option<f32>,
        pv: &[WireDir],
        message: &str,
        turn: u16,
        state_hash: u64,
    ) {
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
        let target_v = target.map(|coords| Vec2::new(coords.x, coords.y));

        let info = wire::Info::create(
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
        );
        fbb.finish(info, None);
    }

    #[test]
    fn extract_info_all_fields() {
        let mut fbb = FlatBufferBuilder::new();
        build_wire_info(
            &mut fbb,
            Player::Player2,
            3,
            Some(Coordinates::new(10, 7)),
            5,
            42000,
            Some(2.5),
            &[WireDir::Up, WireDir::Left],
            "depth 5",
            7,
            0xDEAD_BEEF_CAFE_BABE,
        );
        let buf = fbb.finished_data();
        let info_fb = flatbuffers::root::<wire::Info>(buf).unwrap();

        let info = extract_info(&info_fb);
        assert_eq!(info.player, Player::Player2);
        assert_eq!(info.multipv, 3);
        assert_eq!(info.target, Some(Coordinates::new(10, 7)));
        assert_eq!(info.depth, 5);
        assert_eq!(info.nodes, 42000);
        assert!((info.score.unwrap() - 2.5).abs() < f32::EPSILON);
        assert_eq!(info.pv, vec![pyrat::Direction::Up, pyrat::Direction::Left]);
        assert_eq!(info.message, "depth 5");
        assert_eq!(info.turn, 7);
        assert_eq!(info.state_hash, 0xDEAD_BEEF_CAFE_BABE);
    }

    #[test]
    fn extract_info_empty_optional_fields() {
        let mut fbb = FlatBufferBuilder::new();
        build_wire_info(
            &mut fbb,
            Player::Player1,
            0,
            None,
            0,
            0,
            None,
            &[],
            "",
            0,
            0,
        );
        let buf = fbb.finished_data();
        let info_fb = flatbuffers::root::<wire::Info>(buf).unwrap();

        let info = extract_info(&info_fb);
        assert!(info.target.is_none());
        assert!(info.score.is_none());
        assert!(info.pv.is_empty());
        assert!(info.message.is_empty());
    }

    // ── GameOver ────────────────────────────────────

    #[test]
    fn extract_game_over_roundtrip() {
        let mut fbb = FlatBufferBuilder::new();
        let go = wire::GameOver::create(
            &mut fbb,
            &wire::GameOverArgs {
                result: GameResult::Draw,
                player1_score: 5.0,
                player2_score: 5.0,
            },
        );
        fbb.finish(go, None);
        let buf = fbb.finished_data();
        let go_fb = flatbuffers::root::<wire::GameOver>(buf).unwrap();

        let go = extract_game_over(&go_fb);
        assert_eq!(go.result, GameResult::Draw);
        assert!((go.player1_score - 5.0).abs() < f32::EPSILON);
        assert!((go.player2_score - 5.0).abs() < f32::EPSILON);
    }

    // ── OptionDefs ──────────────────────────────────

    #[test]
    fn extract_option_defs_roundtrip() {
        let mut fbb = FlatBufferBuilder::new();

        let name = fbb.create_string("Hash");
        let default = fbb.create_string("128");
        let c1 = fbb.create_string("64");
        let c2 = fbb.create_string("128");
        let choices = fbb.create_vector(&[c1, c2]);

        let opt = wire::OptionDef::create(
            &mut fbb,
            &wire::OptionDefArgs {
                name: Some(name),
                type_: pyrat_wire::OptionType::Combo,
                default_value: Some(default),
                min: 0,
                max: 256,
                choices: Some(choices),
            },
        );
        let opts_vec = fbb.create_vector(&[opt]);

        // Wrap in an Identify to get a proper table we can root-parse.
        let bot_name = fbb.create_string("Bot");
        let bot_author = fbb.create_string("Author");
        let id = wire::Identify::create(
            &mut fbb,
            &wire::IdentifyArgs {
                name: Some(bot_name),
                author: Some(bot_author),
                options: Some(opts_vec),
                agent_id: None,
            },
        );
        fbb.finish(id, None);
        let buf = fbb.finished_data();
        let id_fb = flatbuffers::root::<wire::Identify>(buf).unwrap();

        let defs = extract_option_defs(id_fb.options().unwrap());
        assert_eq!(defs.len(), 1);
        assert_eq!(defs[0].name, "Hash");
        assert_eq!(defs[0].option_type, pyrat_wire::OptionType::Combo);
        assert_eq!(defs[0].default_value, "128");
        assert_eq!(defs[0].choices, vec!["64", "128"]);
    }

    // ── Cross-crate envelope test ───────────────────

    /// Build a full `HostPacket` envelope containing a `MatchConfig`,
    /// as the host serializer would produce. Extract through the shared
    /// function and verify fields match.
    #[test]
    fn extract_match_config_from_host_packet_envelope() {
        let mut fbb = FlatBufferBuilder::new();

        let cheese_vec = vec![Vec2::new(2, 2)];
        let cheese = fbb.create_vector(&cheese_vec);
        let players = fbb.create_vector(&[Player::Player1]);

        let mc = wire::MatchConfig::create(
            &mut fbb,
            &wire::MatchConfigArgs {
                width: 5,
                height: 5,
                max_turns: 100,
                walls: None,
                mud: None,
                cheese: Some(cheese),
                player1_start: Some(&Vec2::new(4, 4)),
                player2_start: Some(&Vec2::new(0, 0)),
                controlled_players: Some(players),
                timing: TimingMode::Wait,
                move_timeout_ms: 500,
                preprocessing_timeout_ms: 3000,
            },
        );

        let packet = HostPacket::create(
            &mut fbb,
            &HostPacketArgs {
                message_type: HostMessage::MatchConfig,
                message: Some(mc.as_union_value()),
            },
        );
        fbb.finish(packet, None);
        let buf = fbb.finished_data();

        // Parse the envelope, then extract the inner table.
        let packet = flatbuffers::root::<HostPacket>(buf).unwrap();
        assert_eq!(packet.message_type(), HostMessage::MatchConfig);
        let mc = packet.message_as_match_config().unwrap();

        let cfg = extract_match_config(&mc);
        assert_eq!(cfg.width, 5);
        assert_eq!(cfg.height, 5);
        assert_eq!(cfg.cheese.len(), 1);
        assert_eq!(cfg.cheese[0], Coordinates::new(2, 2));
        assert_eq!(cfg.player1_start, Coordinates::new(4, 4));
        assert_eq!(cfg.move_timeout_ms, 500);
    }

    /// Build a full `HostPacket` envelope containing a `TurnState`,
    /// extract through the shared function, verify hash is trusted from wire.
    #[test]
    fn extract_turn_state_from_host_packet_envelope() {
        let mut fbb = FlatBufferBuilder::new();

        let cheese_vec = vec![Vec2::new(2, 2)];
        let cheese = fbb.create_vector(&cheese_vec);

        let ts = wire::TurnState::create(
            &mut fbb,
            &wire::TurnStateArgs {
                turn: 10,
                player1_position: Some(&Vec2::new(1, 1)),
                player2_position: Some(&Vec2::new(3, 3)),
                player1_score: 1.0,
                player2_score: 0.0,
                player1_mud_turns: 0,
                player2_mud_turns: 0,
                cheese: Some(cheese),
                player1_last_move: WireDir::Right,
                player2_last_move: WireDir::Left,
                state_hash: 0xABCD_EF01,
            },
        );

        let packet = HostPacket::create(
            &mut fbb,
            &HostPacketArgs {
                message_type: HostMessage::TurnState,
                message: Some(ts.as_union_value()),
            },
        );
        fbb.finish(packet, None);
        let buf = fbb.finished_data();

        let packet = flatbuffers::root::<HostPacket>(buf).unwrap();
        let ts = packet.message_as_turn_state().unwrap();

        let hts = extract_turn_state(&ts);
        assert_eq!(hts.turn, 10);
        assert_eq!(hts.player1_position, Coordinates::new(1, 1));
        assert_eq!(hts.state_hash(), 0xABCD_EF01);
    }
}
