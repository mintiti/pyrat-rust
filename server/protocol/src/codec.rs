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

use flatbuffers::{FlatBufferBuilder, WIPOffset};
use pyrat::{Coordinates, Direction};
use pyrat_wire::{self as wire, BotMessage, HostMessage, Vec2};

use crate::{
    engine_to_wire_direction, wire_to_engine_direction, BotMsg, GameOver, HashedTurnState, HostMsg,
    Info, MatchConfig, MudEntry, OptionDef, SearchLimits, TurnState,
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

/// Extract a [`MatchConfig`] from a wire `MatchConfig` table.
pub fn extract_match_config(mc: &wire::MatchConfig<'_>) -> MatchConfig {
    MatchConfig {
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
        timing: mc.timing(),
        move_timeout_ms: mc.move_timeout_ms(),
        preprocessing_timeout_ms: mc.preprocessing_timeout_ms(),
    }
}

/// Extract a [`TurnState`] from a wire `TurnState` table.
///
/// The wire table carries no hash — the parent message (`GoState`,
/// `FullState`) provides one when needed, or the bot recomputes it
/// after rebuilding engine state.
pub fn extract_turn_state(ts: &wire::TurnState<'_>) -> TurnState {
    TurnState {
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
    }
}

/// Extract an [`Info`] from a wire `Info` table.
pub fn extract_info(info: &wire::Info<'_>) -> Info {
    Info {
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

/// Extract a [`GameOver`] from a wire `GameOver` table.
pub fn extract_game_over(go: &wire::GameOver<'_>) -> GameOver {
    GameOver {
        result: go.result(),
        player1_score: go.player1_score(),
        player2_score: go.player2_score(),
    }
}

/// Extract option definitions from a FlatBuffers vector of `OptionDef` tables.
pub fn extract_option_defs(
    opts: flatbuffers::Vector<'_, flatbuffers::ForwardsUOffset<wire::OptionDef<'_>>>,
) -> Vec<OptionDef> {
    (0..opts.len())
        .map(|i| {
            let o = opts.get(i);
            OptionDef {
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

// ── Top-level codec errors ──────────────────────────

/// Failure decoding a wire packet into an owned [`HostMsg`] / [`BotMsg`].
///
/// Surfaces violations a sender shouldn't have produced: missing union
/// payload, unknown message type, missing required inner table. Sideband
/// content errors (bad Info / RenderCommands payload shape) are not in this
/// enum — those are logged and dropped at the call site, not protocol faults.
#[derive(Debug, thiserror::Error)]
pub enum CodecError {
    #[error("packet message_type was NONE / no payload")]
    MissingPayload,
    #[error("unknown HostMessage variant: {0:?}")]
    UnknownHostVariant(HostMessage),
    #[error("unknown BotMessage variant: {0:?}")]
    UnknownBotVariant(BotMessage),
    #[error("required inner table missing: {0}")]
    MissingTable(&'static str),
}

// ── SearchLimits ────────────────────────────────────

/// Extract search limits. Each axis: 0 (= unset on the wire) maps to `None`.
pub fn extract_search_limits(s: &wire::SearchLimits<'_>) -> SearchLimits {
    let timeout_ms = if s.timeout_ms() == 0 {
        None
    } else {
        Some(s.timeout_ms())
    };
    let depth = if s.depth() == 0 {
        None
    } else {
        Some(s.depth())
    };
    let nodes = if s.nodes() == 0 {
        None
    } else {
        Some(s.nodes())
    };
    SearchLimits {
        timeout_ms,
        depth,
        nodes,
    }
}

/// Serialize search limits inline (struct-style payload — caller pulls fields
/// into a parent table's args).
fn serialize_search_limits<'a>(
    fbb: &mut FlatBufferBuilder<'a>,
    limits: &SearchLimits,
) -> WIPOffset<wire::SearchLimits<'a>> {
    wire::SearchLimits::create(
        fbb,
        &wire::SearchLimitsArgs {
            timeout_ms: limits.timeout_ms.unwrap_or(0),
            depth: limits.depth.unwrap_or(0),
            nodes: limits.nodes.unwrap_or(0),
        },
    )
}

// ── New HostMsg variant extractors ──────────────────

pub fn extract_welcome(w: &wire::Welcome<'_>) -> wire::Player {
    w.player_slot()
}

pub fn extract_go_preprocess(w: &wire::GoPreprocess<'_>) -> u64 {
    w.state_hash()
}

pub fn extract_advance(w: &wire::Advance<'_>) -> (Direction, Direction, u16, u64) {
    (
        wire_to_engine_direction(w.p1_dir()),
        wire_to_engine_direction(w.p2_dir()),
        w.turn(),
        w.new_hash(),
    )
}

pub fn extract_go(w: &wire::Go<'_>) -> Result<(u64, SearchLimits), CodecError> {
    let limits = w
        .limits()
        .ok_or(CodecError::MissingTable("Go.limits"))
        .map(|l| extract_search_limits(&l))?;
    Ok((w.state_hash(), limits))
}

pub fn extract_go_state(
    w: &wire::GoState<'_>,
) -> Result<(HashedTurnState, SearchLimits), CodecError> {
    let ts = w
        .turn_state()
        .ok_or(CodecError::MissingTable("GoState.turn_state"))?;
    let limits = w
        .limits()
        .ok_or(CodecError::MissingTable("GoState.limits"))
        .map(|l| extract_search_limits(&l))?;
    let owned = extract_turn_state(&ts);
    Ok((
        HashedTurnState::with_unverified_hash(owned, w.state_hash()),
        limits,
    ))
}

pub fn extract_full_state(w: &wire::FullState<'_>) -> Result<(MatchConfig, TurnState), CodecError> {
    let mc = w
        .match_config()
        .ok_or(CodecError::MissingTable("FullState.match_config"))?;
    let ts = w
        .turn_state()
        .ok_or(CodecError::MissingTable("FullState.turn_state"))?;
    Ok((extract_match_config(&mc), extract_turn_state(&ts)))
}

pub fn extract_protocol_error(w: &wire::ProtocolError<'_>) -> String {
    w.reason().unwrap_or("").to_owned()
}

pub fn extract_configure(
    w: &wire::Configure<'_>,
) -> Result<(Vec<(String, String)>, MatchConfig), CodecError> {
    let mc = w
        .match_config()
        .ok_or(CodecError::MissingTable("Configure.match_config"))?;
    let options: Vec<(String, String)> = w
        .options()
        .map(|opts| {
            (0..opts.len())
                .map(|i| {
                    let o = opts.get(i);
                    (
                        o.name().unwrap_or("").to_owned(),
                        o.value().unwrap_or("").to_owned(),
                    )
                })
                .collect()
        })
        .unwrap_or_default();
    Ok((options, extract_match_config(&mc)))
}

// ── New BotMsg variant extractors ───────────────────

pub fn extract_sync_ok(w: &wire::SyncOk<'_>) -> u64 {
    w.hash()
}

pub fn extract_resync(w: &wire::Resync<'_>) -> u64 {
    w.my_hash()
}

pub fn extract_provisional(w: &wire::Provisional<'_>) -> (Direction, wire::Player, u16, u64) {
    (
        wire_to_engine_direction(w.direction()),
        w.player(),
        w.turn(),
        w.state_hash(),
    )
}

pub fn extract_render_commands(w: &wire::RenderCommands<'_>) -> (wire::Player, u16, u64) {
    (w.player(), w.turn(), w.state_hash())
}

// ── Top-level dispatchers ───────────────────────────

/// Decode a `HostPacket` envelope into an owned [`HostMsg`].
pub fn extract_host_msg(packet: &wire::HostPacket<'_>) -> Result<HostMsg, CodecError> {
    match packet.message_type() {
        HostMessage::Welcome => packet
            .message_as_welcome()
            .ok_or(CodecError::MissingPayload)
            .map(|w| HostMsg::Welcome {
                player_slot: extract_welcome(&w),
            }),
        HostMessage::Configure => {
            let w = packet
                .message_as_configure()
                .ok_or(CodecError::MissingPayload)?;
            let (options, match_config) = extract_configure(&w)?;
            Ok(HostMsg::Configure {
                options,
                match_config: Box::new(match_config),
            })
        },
        HostMessage::GoPreprocess => packet
            .message_as_go_preprocess()
            .ok_or(CodecError::MissingPayload)
            .map(|w| HostMsg::GoPreprocess {
                state_hash: extract_go_preprocess(&w),
            }),
        HostMessage::Advance => {
            let w = packet
                .message_as_advance()
                .ok_or(CodecError::MissingPayload)?;
            let (p1_dir, p2_dir, turn, new_hash) = extract_advance(&w);
            Ok(HostMsg::Advance {
                p1_dir,
                p2_dir,
                turn,
                new_hash,
            })
        },
        HostMessage::Go => {
            let w = packet.message_as_go().ok_or(CodecError::MissingPayload)?;
            let (state_hash, limits) = extract_go(&w)?;
            Ok(HostMsg::Go { state_hash, limits })
        },
        HostMessage::GoState => {
            let w = packet
                .message_as_go_state()
                .ok_or(CodecError::MissingPayload)?;
            let (hts, limits) = extract_go_state(&w)?;
            let state_hash = hts.state_hash();
            Ok(HostMsg::GoState {
                turn_state: Box::new(hts.into_inner()),
                state_hash,
                limits,
            })
        },
        HostMessage::Stop => Ok(HostMsg::Stop),
        HostMessage::FullState => {
            let w = packet
                .message_as_full_state()
                .ok_or(CodecError::MissingPayload)?;
            let (match_config, turn_state) = extract_full_state(&w)?;
            Ok(HostMsg::FullState {
                match_config: Box::new(match_config),
                turn_state: Box::new(turn_state),
            })
        },
        HostMessage::ProtocolError => packet
            .message_as_protocol_error()
            .ok_or(CodecError::MissingPayload)
            .map(|w| HostMsg::ProtocolError {
                reason: extract_protocol_error(&w),
            }),
        HostMessage::GameOver => packet
            .message_as_game_over()
            .ok_or(CodecError::MissingPayload)
            .map(|w| {
                let go = extract_game_over(&w);
                HostMsg::GameOver {
                    result: go.result,
                    player1_score: go.player1_score,
                    player2_score: go.player2_score,
                }
            }),
        HostMessage::NONE => Err(CodecError::MissingPayload),
        other => Err(CodecError::UnknownHostVariant(other)),
    }
}

/// Decode a `BotPacket` envelope into an owned [`BotMsg`].
pub fn extract_bot_msg(packet: &wire::BotPacket<'_>) -> Result<BotMsg, CodecError> {
    match packet.message_type() {
        BotMessage::Identify => packet
            .message_as_identify()
            .ok_or(CodecError::MissingPayload)
            .map(|w| BotMsg::Identify {
                name: w.name().unwrap_or("").to_owned(),
                author: w.author().unwrap_or("").to_owned(),
                agent_id: w.agent_id().unwrap_or("").to_owned(),
                options: w.options().map(extract_option_defs).unwrap_or_default(),
            }),
        BotMessage::Ready => packet
            .message_as_ready()
            .ok_or(CodecError::MissingPayload)
            .map(|w| BotMsg::Ready {
                state_hash: w.state_hash(),
            }),
        BotMessage::PreprocessingDone => Ok(BotMsg::PreprocessingDone),
        BotMessage::SyncOk => packet
            .message_as_sync_ok()
            .ok_or(CodecError::MissingPayload)
            .map(|w| BotMsg::SyncOk {
                hash: extract_sync_ok(&w),
            }),
        BotMessage::Resync => packet
            .message_as_resync()
            .ok_or(CodecError::MissingPayload)
            .map(|w| BotMsg::Resync {
                my_hash: extract_resync(&w),
            }),
        BotMessage::Action => packet
            .message_as_action()
            .ok_or(CodecError::MissingPayload)
            .map(|w| BotMsg::Action {
                direction: wire_to_engine_direction(w.direction()),
                player: w.player(),
                turn: w.turn(),
                state_hash: w.state_hash(),
                think_ms: w.think_ms(),
            }),
        BotMessage::Provisional => {
            let w = packet
                .message_as_provisional()
                .ok_or(CodecError::MissingPayload)?;
            let (direction, player, turn, state_hash) = extract_provisional(&w);
            Ok(BotMsg::Provisional {
                direction,
                player,
                turn,
                state_hash,
            })
        },
        BotMessage::Info => packet
            .message_as_info()
            .ok_or(CodecError::MissingPayload)
            .map(|w| BotMsg::Info(extract_info(&w))),
        BotMessage::RenderCommands => {
            let w = packet
                .message_as_render_commands()
                .ok_or(CodecError::MissingPayload)?;
            let (player, turn, state_hash) = extract_render_commands(&w);
            Ok(BotMsg::RenderCommands {
                player,
                turn,
                state_hash,
            })
        },
        BotMessage::NONE => Err(CodecError::MissingPayload),
        other => Err(CodecError::UnknownBotVariant(other)),
    }
}

// ── Owned → wire serialization (full HostPacket / BotPacket bytes) ─

fn serialize_match_config<'a>(
    fbb: &mut FlatBufferBuilder<'a>,
    cfg: &MatchConfig,
) -> WIPOffset<wire::MatchConfig<'a>> {
    let walls_data: Vec<_> = cfg
        .walls
        .iter()
        .map(|(a, b)| {
            wire::Wall::create(
                fbb,
                &wire::WallArgs {
                    pos1: Some(&Vec2::new(a.x, a.y)),
                    pos2: Some(&Vec2::new(b.x, b.y)),
                },
            )
        })
        .collect();
    let walls = fbb.create_vector(&walls_data);

    let muds_data: Vec<_> = cfg
        .mud
        .iter()
        .map(|m| {
            wire::Mud::create(
                fbb,
                &wire::MudArgs {
                    pos1: Some(&Vec2::new(m.pos1.x, m.pos1.y)),
                    pos2: Some(&Vec2::new(m.pos2.x, m.pos2.y)),
                    value: m.turns,
                },
            )
        })
        .collect();
    let mud = fbb.create_vector(&muds_data);

    let cheese_vec: Vec<Vec2> = cfg.cheese.iter().map(|c| Vec2::new(c.x, c.y)).collect();
    let cheese = fbb.create_vector(&cheese_vec);

    wire::MatchConfig::create(
        fbb,
        &wire::MatchConfigArgs {
            width: cfg.width,
            height: cfg.height,
            max_turns: cfg.max_turns,
            walls: Some(walls),
            mud: Some(mud),
            cheese: Some(cheese),
            player1_start: Some(&Vec2::new(cfg.player1_start.x, cfg.player1_start.y)),
            player2_start: Some(&Vec2::new(cfg.player2_start.x, cfg.player2_start.y)),
            timing: cfg.timing,
            move_timeout_ms: cfg.move_timeout_ms,
            preprocessing_timeout_ms: cfg.preprocessing_timeout_ms,
        },
    )
}

fn serialize_turn_state<'a>(
    fbb: &mut FlatBufferBuilder<'a>,
    ts: &TurnState,
) -> WIPOffset<wire::TurnState<'a>> {
    let cheese_vec: Vec<Vec2> = ts.cheese.iter().map(|c| Vec2::new(c.x, c.y)).collect();
    let cheese = fbb.create_vector(&cheese_vec);
    wire::TurnState::create(
        fbb,
        &wire::TurnStateArgs {
            turn: ts.turn,
            player1_position: Some(&Vec2::new(ts.player1_position.x, ts.player1_position.y)),
            player2_position: Some(&Vec2::new(ts.player2_position.x, ts.player2_position.y)),
            player1_score: ts.player1_score,
            player2_score: ts.player2_score,
            player1_mud_turns: ts.player1_mud_turns,
            player2_mud_turns: ts.player2_mud_turns,
            cheese: Some(cheese),
            player1_last_move: engine_to_wire_direction(ts.player1_last_move),
            player2_last_move: engine_to_wire_direction(ts.player2_last_move),
        },
    )
}

fn serialize_info<'a>(fbb: &mut FlatBufferBuilder<'a>, info: &Info) -> WIPOffset<wire::Info<'a>> {
    let pv_dirs: Vec<wire::Direction> = info
        .pv
        .iter()
        .map(|d| engine_to_wire_direction(*d))
        .collect();
    let pv = if pv_dirs.is_empty() {
        None
    } else {
        Some(fbb.create_vector(&pv_dirs))
    };
    let message = if info.message.is_empty() {
        None
    } else {
        Some(fbb.create_string(&info.message))
    };
    let target = info.target.map(|c| Vec2::new(c.x, c.y));
    wire::Info::create(
        fbb,
        &wire::InfoArgs {
            player: info.player,
            multipv: info.multipv,
            target: target.as_ref(),
            depth: info.depth,
            nodes: info.nodes,
            score: info.score,
            pv,
            message,
            turn: info.turn,
            state_hash: info.state_hash,
        },
    )
}

/// Encode a [`HostMsg`] as a complete length-unprefixed `HostPacket` byte
/// buffer (caller wraps in transport framing).
///
/// Panics on legacy variants (which have no `HostMsg` representation) — those
/// can only originate from legacy host code paths and never reach this function.
pub fn serialize_host_msg(msg: &HostMsg) -> Vec<u8> {
    let mut fbb = FlatBufferBuilder::new();
    let (msg_type, offset) = match msg {
        HostMsg::Welcome { player_slot } => {
            let off = wire::Welcome::create(
                &mut fbb,
                &wire::WelcomeArgs {
                    player_slot: *player_slot,
                },
            );
            (HostMessage::Welcome, off.as_union_value())
        },
        HostMsg::Configure {
            options,
            match_config,
        } => {
            let opts_data: Vec<_> = options
                .iter()
                .map(|(name, value)| {
                    let n = fbb.create_string(name);
                    let v = fbb.create_string(value);
                    wire::OptionAssignment::create(
                        &mut fbb,
                        &wire::OptionAssignmentArgs {
                            name: Some(n),
                            value: Some(v),
                        },
                    )
                })
                .collect();
            let opts = fbb.create_vector(&opts_data);
            let mc = serialize_match_config(&mut fbb, match_config);
            let off = wire::Configure::create(
                &mut fbb,
                &wire::ConfigureArgs {
                    options: Some(opts),
                    match_config: Some(mc),
                },
            );
            (HostMessage::Configure, off.as_union_value())
        },
        HostMsg::GoPreprocess { state_hash } => {
            let off = wire::GoPreprocess::create(
                &mut fbb,
                &wire::GoPreprocessArgs {
                    state_hash: *state_hash,
                },
            );
            (HostMessage::GoPreprocess, off.as_union_value())
        },
        HostMsg::Advance {
            p1_dir,
            p2_dir,
            turn,
            new_hash,
        } => {
            let off = wire::Advance::create(
                &mut fbb,
                &wire::AdvanceArgs {
                    p1_dir: engine_to_wire_direction(*p1_dir),
                    p2_dir: engine_to_wire_direction(*p2_dir),
                    turn: *turn,
                    new_hash: *new_hash,
                },
            );
            (HostMessage::Advance, off.as_union_value())
        },
        HostMsg::Go { state_hash, limits } => {
            let lim = serialize_search_limits(&mut fbb, limits);
            let off = wire::Go::create(
                &mut fbb,
                &wire::GoArgs {
                    state_hash: *state_hash,
                    limits: Some(lim),
                },
            );
            (HostMessage::Go, off.as_union_value())
        },
        HostMsg::GoState {
            turn_state,
            state_hash,
            limits,
        } => {
            let ts = serialize_turn_state(&mut fbb, turn_state);
            let lim = serialize_search_limits(&mut fbb, limits);
            let off = wire::GoState::create(
                &mut fbb,
                &wire::GoStateArgs {
                    turn_state: Some(ts),
                    state_hash: *state_hash,
                    limits: Some(lim),
                },
            );
            (HostMessage::GoState, off.as_union_value())
        },
        HostMsg::Stop => {
            let off = wire::Stop::create(&mut fbb, &wire::StopArgs {});
            (HostMessage::Stop, off.as_union_value())
        },
        HostMsg::FullState {
            match_config,
            turn_state,
        } => {
            let mc = serialize_match_config(&mut fbb, match_config);
            let ts = serialize_turn_state(&mut fbb, turn_state);
            let off = wire::FullState::create(
                &mut fbb,
                &wire::FullStateArgs {
                    match_config: Some(mc),
                    turn_state: Some(ts),
                },
            );
            (HostMessage::FullState, off.as_union_value())
        },
        HostMsg::ProtocolError { reason } => {
            let r = fbb.create_string(reason);
            let off =
                wire::ProtocolError::create(&mut fbb, &wire::ProtocolErrorArgs { reason: Some(r) });
            (HostMessage::ProtocolError, off.as_union_value())
        },
        HostMsg::GameOver {
            result,
            player1_score,
            player2_score,
        } => {
            let off = wire::GameOver::create(
                &mut fbb,
                &wire::GameOverArgs {
                    result: *result,
                    player1_score: *player1_score,
                    player2_score: *player2_score,
                },
            );
            (HostMessage::GameOver, off.as_union_value())
        },
    };
    let packet = wire::HostPacket::create(
        &mut fbb,
        &wire::HostPacketArgs {
            message_type: msg_type,
            message: Some(offset),
        },
    );
    fbb.finish(packet, None);
    fbb.finished_data().to_vec()
}

/// Encode a [`BotMsg`] as a complete length-unprefixed `BotPacket` byte buffer.
pub fn serialize_bot_msg(msg: &BotMsg) -> Vec<u8> {
    let mut fbb = FlatBufferBuilder::new();
    let (msg_type, offset) = match msg {
        BotMsg::Identify {
            name,
            author,
            agent_id,
            options,
        } => {
            let n = fbb.create_string(name);
            let a = fbb.create_string(author);
            let id = fbb.create_string(agent_id);
            let opts_data: Vec<_> = options
                .iter()
                .map(|o| {
                    let oname = fbb.create_string(&o.name);
                    let dval = fbb.create_string(&o.default_value);
                    let choices_data: Vec<_> =
                        o.choices.iter().map(|s| fbb.create_string(s)).collect();
                    let choices = fbb.create_vector(&choices_data);
                    wire::OptionDef::create(
                        &mut fbb,
                        &wire::OptionDefArgs {
                            name: Some(oname),
                            type_: o.option_type,
                            default_value: Some(dval),
                            min: o.min,
                            max: o.max,
                            choices: Some(choices),
                        },
                    )
                })
                .collect();
            let opts = fbb.create_vector(&opts_data);
            let off = wire::Identify::create(
                &mut fbb,
                &wire::IdentifyArgs {
                    name: Some(n),
                    author: Some(a),
                    options: Some(opts),
                    agent_id: Some(id),
                },
            );
            (BotMessage::Identify, off.as_union_value())
        },
        BotMsg::Ready { state_hash } => {
            let off = wire::Ready::create(
                &mut fbb,
                &wire::ReadyArgs {
                    state_hash: *state_hash,
                },
            );
            (BotMessage::Ready, off.as_union_value())
        },
        BotMsg::PreprocessingDone => {
            let off = wire::PreprocessingDone::create(&mut fbb, &wire::PreprocessingDoneArgs {});
            (BotMessage::PreprocessingDone, off.as_union_value())
        },
        BotMsg::SyncOk { hash } => {
            let off = wire::SyncOk::create(&mut fbb, &wire::SyncOkArgs { hash: *hash });
            (BotMessage::SyncOk, off.as_union_value())
        },
        BotMsg::Resync { my_hash } => {
            let off = wire::Resync::create(&mut fbb, &wire::ResyncArgs { my_hash: *my_hash });
            (BotMessage::Resync, off.as_union_value())
        },
        BotMsg::Action {
            direction,
            player,
            turn,
            state_hash,
            think_ms,
        } => {
            let off = wire::Action::create(
                &mut fbb,
                &wire::ActionArgs {
                    direction: engine_to_wire_direction(*direction),
                    player: *player,
                    turn: *turn,
                    think_ms: *think_ms,
                    state_hash: *state_hash,
                },
            );
            (BotMessage::Action, off.as_union_value())
        },
        BotMsg::Provisional {
            direction,
            player,
            turn,
            state_hash,
        } => {
            let off = wire::Provisional::create(
                &mut fbb,
                &wire::ProvisionalArgs {
                    direction: engine_to_wire_direction(*direction),
                    player: *player,
                    turn: *turn,
                    state_hash: *state_hash,
                },
            );
            (BotMessage::Provisional, off.as_union_value())
        },
        BotMsg::Info(info) => {
            let off = serialize_info(&mut fbb, info);
            (BotMessage::Info, off.as_union_value())
        },
        BotMsg::RenderCommands {
            player,
            turn,
            state_hash,
        } => {
            let off = wire::RenderCommands::create(
                &mut fbb,
                &wire::RenderCommandsArgs {
                    player: *player,
                    turn: *turn,
                    state_hash: *state_hash,
                },
            );
            (BotMessage::RenderCommands, off.as_union_value())
        },
    };
    let packet = wire::BotPacket::create(
        &mut fbb,
        &wire::BotPacketArgs {
            message_type: msg_type,
            message: Some(offset),
        },
    );
    fbb.finish(packet, None);
    fbb.finished_data().to_vec()
}

#[cfg(test)]
mod tests {
    use super::*;
    use flatbuffers::FlatBufferBuilder;
    use pyrat_wire::{Direction as WireDir, GameResult, HostPacket, Player, TimingMode};

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
            },
        );
        fbb.finish(ts, None);
        let buf = fbb.finished_data();
        let ts = flatbuffers::root::<wire::TurnState>(buf).unwrap();

        let owned = extract_turn_state(&ts);
        assert_eq!(owned.turn, 42);
        assert_eq!(owned.player1_position, Coordinates::new(10, 7));
        assert_eq!(owned.player2_position, Coordinates::new(0, 0));
        assert!((owned.player1_score - 3.0).abs() < f32::EPSILON);
        assert!((owned.player2_score - 2.5).abs() < f32::EPSILON);
        assert_eq!(owned.player1_mud_turns, 0);
        assert_eq!(owned.player2_mud_turns, 2);
        assert_eq!(owned.cheese.len(), 2);
        assert_eq!(owned.player1_last_move, pyrat::Direction::Up);
        assert_eq!(owned.player2_last_move, pyrat::Direction::Right);
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

    // ── Round-trip tests for new HostMsg / BotMsg variants ──

    use pyrat_wire::BotPacket;

    fn sample_match_config() -> MatchConfig {
        MatchConfig {
            width: 7,
            height: 5,
            max_turns: 100,
            walls: vec![(Coordinates::new(1, 0), Coordinates::new(1, 1))],
            mud: vec![MudEntry {
                pos1: Coordinates::new(2, 0),
                pos2: Coordinates::new(2, 1),
                turns: 3,
            }],
            cheese: vec![Coordinates::new(3, 3)],
            player1_start: Coordinates::new(0, 0),
            player2_start: Coordinates::new(6, 4),
            timing: TimingMode::Wait,
            move_timeout_ms: 250,
            preprocessing_timeout_ms: 5000,
        }
    }

    fn sample_turn_state() -> TurnState {
        TurnState {
            turn: 7,
            player1_position: Coordinates::new(2, 1),
            player2_position: Coordinates::new(5, 3),
            player1_score: 1.5,
            player2_score: 2.0,
            player1_mud_turns: 1,
            player2_mud_turns: 0,
            cheese: vec![Coordinates::new(3, 3), Coordinates::new(4, 2)],
            player1_last_move: Direction::Up,
            player2_last_move: Direction::Down,
        }
    }

    fn sample_info() -> Info {
        Info {
            player: Player::Player2,
            multipv: 1,
            target: Some(Coordinates::new(3, 3)),
            depth: 5,
            nodes: 1234,
            score: Some(0.75),
            pv: vec![Direction::Up, Direction::Right],
            message: "best line".to_owned(),
            turn: 7,
            state_hash: 0xCAFE_BABE,
        }
    }

    /// Round-trip a HostMsg through serialize → wire bytes → extract → equal.
    fn host_roundtrip(msg: HostMsg) -> HostMsg {
        let bytes = serialize_host_msg(&msg);
        let packet = flatbuffers::root::<HostPacket>(&bytes).unwrap();
        extract_host_msg(&packet).expect("decode")
    }

    fn bot_roundtrip(msg: BotMsg) -> BotMsg {
        let bytes = serialize_bot_msg(&msg);
        let packet = flatbuffers::root::<BotPacket>(&bytes).unwrap();
        extract_bot_msg(&packet).expect("decode")
    }

    #[test]
    fn welcome_roundtrips() {
        let out = host_roundtrip(HostMsg::Welcome {
            player_slot: Player::Player2,
        });
        match out {
            HostMsg::Welcome { player_slot } => assert_eq!(player_slot, Player::Player2),
            other => panic!("expected Welcome, got {other:?}"),
        }
    }

    #[test]
    fn configure_roundtrips() {
        let original = HostMsg::Configure {
            options: vec![
                ("Threads".to_owned(), "4".to_owned()),
                ("Hash".to_owned(), "128".to_owned()),
            ],
            match_config: Box::new(sample_match_config()),
        };
        let out = host_roundtrip(original);
        match out {
            HostMsg::Configure {
                options,
                match_config,
            } => {
                assert_eq!(
                    options,
                    vec![
                        ("Threads".to_owned(), "4".to_owned()),
                        ("Hash".to_owned(), "128".to_owned()),
                    ]
                );
                assert_eq!(match_config.width, 7);
                assert_eq!(match_config.cheese.len(), 1);
                assert_eq!(match_config.mud[0].turns, 3);
            },
            other => panic!("expected Configure, got {other:?}"),
        }
    }

    #[test]
    fn go_preprocess_roundtrips() {
        let out = host_roundtrip(HostMsg::GoPreprocess {
            state_hash: 0xDEAD_BEEF,
        });
        match out {
            HostMsg::GoPreprocess { state_hash } => assert_eq!(state_hash, 0xDEAD_BEEF),
            other => panic!("expected GoPreprocess, got {other:?}"),
        }
    }

    #[test]
    fn advance_roundtrips() {
        let out = host_roundtrip(HostMsg::Advance {
            p1_dir: Direction::Up,
            p2_dir: Direction::Stay,
            turn: 42,
            new_hash: 0xFEED_FACE_1234_5678,
        });
        match out {
            HostMsg::Advance {
                p1_dir,
                p2_dir,
                turn,
                new_hash,
            } => {
                assert_eq!(p1_dir, Direction::Up);
                assert_eq!(p2_dir, Direction::Stay);
                assert_eq!(turn, 42);
                assert_eq!(new_hash, 0xFEED_FACE_1234_5678);
            },
            other => panic!("expected Advance, got {other:?}"),
        }
    }

    #[test]
    fn go_roundtrips_with_limits() {
        let out = host_roundtrip(HostMsg::Go {
            state_hash: 0xAA_BB_CC,
            limits: SearchLimits {
                timeout_ms: Some(250),
                depth: Some(8),
                nodes: None,
            },
        });
        match out {
            HostMsg::Go { state_hash, limits } => {
                assert_eq!(state_hash, 0xAA_BB_CC);
                assert_eq!(limits.timeout_ms, Some(250));
                assert_eq!(limits.depth, Some(8));
                assert_eq!(limits.nodes, None);
            },
            other => panic!("expected Go, got {other:?}"),
        }
    }

    #[test]
    fn go_roundtrips_with_infinite_limits() {
        // All None on the owned side ⇄ all-zero on the wire ⇄ all None back.
        let out = host_roundtrip(HostMsg::Go {
            state_hash: 1,
            limits: SearchLimits {
                timeout_ms: None,
                depth: None,
                nodes: None,
            },
        });
        match out {
            HostMsg::Go { limits, .. } => {
                assert!(limits.timeout_ms.is_none());
                assert!(limits.depth.is_none());
                assert!(limits.nodes.is_none());
            },
            other => panic!("expected Go, got {other:?}"),
        }
    }

    #[test]
    fn go_state_roundtrips() {
        let out = host_roundtrip(HostMsg::GoState {
            turn_state: Box::new(sample_turn_state()),
            state_hash: 0x1234_5678_9ABC_DEF0,
            limits: SearchLimits {
                timeout_ms: Some(500),
                depth: None,
                nodes: Some(50_000),
            },
        });
        match out {
            HostMsg::GoState {
                turn_state,
                state_hash,
                limits,
            } => {
                assert_eq!(state_hash, 0x1234_5678_9ABC_DEF0);
                assert_eq!(turn_state.turn, 7);
                assert_eq!(turn_state.player1_position, Coordinates::new(2, 1));
                assert_eq!(turn_state.player1_last_move, Direction::Up);
                assert_eq!(turn_state.cheese.len(), 2);
                assert_eq!(limits.timeout_ms, Some(500));
                assert_eq!(limits.nodes, Some(50_000));
            },
            other => panic!("expected GoState, got {other:?}"),
        }
    }

    #[test]
    fn full_state_roundtrips() {
        let out = host_roundtrip(HostMsg::FullState {
            match_config: Box::new(sample_match_config()),
            turn_state: Box::new(sample_turn_state()),
        });
        match out {
            HostMsg::FullState {
                match_config,
                turn_state,
            } => {
                assert_eq!(match_config.width, 7);
                assert_eq!(turn_state.turn, 7);
                assert_eq!(turn_state.player1_position, Coordinates::new(2, 1));
            },
            other => panic!("expected FullState, got {other:?}"),
        }
    }

    #[test]
    fn protocol_error_roundtrips() {
        let out = host_roundtrip(HostMsg::ProtocolError {
            reason: "unexpected message in WaitingForReady".to_owned(),
        });
        match out {
            HostMsg::ProtocolError { reason } => {
                assert_eq!(reason, "unexpected message in WaitingForReady");
            },
            other => panic!("expected ProtocolError, got {other:?}"),
        }
    }

    #[test]
    fn stop_roundtrips() {
        match host_roundtrip(HostMsg::Stop) {
            HostMsg::Stop => {},
            other => panic!("expected Stop, got {other:?}"),
        }
    }

    #[test]
    fn game_over_roundtrips() {
        let out = host_roundtrip(HostMsg::GameOver {
            result: GameResult::Player1,
            player1_score: 5.5,
            player2_score: 3.0,
        });
        match out {
            HostMsg::GameOver {
                result,
                player1_score,
                player2_score,
            } => {
                assert_eq!(result, GameResult::Player1);
                assert!((player1_score - 5.5).abs() < f32::EPSILON);
                assert!((player2_score - 3.0).abs() < f32::EPSILON);
            },
            other => panic!("expected GameOver, got {other:?}"),
        }
    }

    // ── BotMsg round-trips ──────────────────────────

    #[test]
    fn identify_roundtrips() {
        let original = BotMsg::Identify {
            name: "TestBot".to_owned(),
            author: "Author".to_owned(),
            agent_id: "agent-1".to_owned(),
            options: vec![OptionDef {
                name: "Hash".to_owned(),
                option_type: pyrat_wire::OptionType::Spin,
                default_value: "128".to_owned(),
                min: 16,
                max: 1024,
                choices: vec![],
            }],
        };
        let out = bot_roundtrip(original);
        match out {
            BotMsg::Identify {
                name,
                author,
                agent_id,
                options,
            } => {
                assert_eq!(name, "TestBot");
                assert_eq!(author, "Author");
                assert_eq!(agent_id, "agent-1");
                assert_eq!(options.len(), 1);
                assert_eq!(options[0].name, "Hash");
                assert_eq!(options[0].min, 16);
            },
            other => panic!("expected Identify, got {other:?}"),
        }
    }

    #[test]
    fn ready_roundtrips_with_hash() {
        let out = bot_roundtrip(BotMsg::Ready {
            state_hash: 0xABCD_EF01_2345_6789,
        });
        match out {
            BotMsg::Ready { state_hash } => {
                assert_eq!(state_hash, 0xABCD_EF01_2345_6789);
            },
            other => panic!("expected Ready, got {other:?}"),
        }
    }

    #[test]
    fn preprocessing_done_roundtrips() {
        match bot_roundtrip(BotMsg::PreprocessingDone) {
            BotMsg::PreprocessingDone => {},
            other => panic!("expected PreprocessingDone, got {other:?}"),
        }
    }

    #[test]
    fn sync_ok_roundtrips() {
        let out = bot_roundtrip(BotMsg::SyncOk {
            hash: 0x1111_2222_3333_4444,
        });
        match out {
            BotMsg::SyncOk { hash } => assert_eq!(hash, 0x1111_2222_3333_4444),
            other => panic!("expected SyncOk, got {other:?}"),
        }
    }

    #[test]
    fn resync_roundtrips() {
        let out = bot_roundtrip(BotMsg::Resync {
            my_hash: 0x9999_8888,
        });
        match out {
            BotMsg::Resync { my_hash } => assert_eq!(my_hash, 0x9999_8888),
            other => panic!("expected Resync, got {other:?}"),
        }
    }

    #[test]
    fn action_roundtrips_with_hash() {
        let out = bot_roundtrip(BotMsg::Action {
            direction: Direction::Right,
            player: Player::Player1,
            turn: 12,
            state_hash: 0xDEAD_BEEF,
            think_ms: 47,
        });
        match out {
            BotMsg::Action {
                direction,
                player,
                turn,
                state_hash,
                think_ms,
            } => {
                assert_eq!(direction, Direction::Right);
                assert_eq!(player, Player::Player1);
                assert_eq!(turn, 12);
                assert_eq!(state_hash, 0xDEAD_BEEF);
                assert_eq!(think_ms, 47);
            },
            other => panic!("expected Action, got {other:?}"),
        }
    }

    #[test]
    fn provisional_roundtrips() {
        let out = bot_roundtrip(BotMsg::Provisional {
            direction: Direction::Down,
            player: Player::Player2,
            turn: 9,
            state_hash: 0xFACE_FEED,
        });
        match out {
            BotMsg::Provisional {
                direction,
                player,
                turn,
                state_hash,
            } => {
                assert_eq!(direction, Direction::Down);
                assert_eq!(player, Player::Player2);
                assert_eq!(turn, 9);
                assert_eq!(state_hash, 0xFACE_FEED);
            },
            other => panic!("expected Provisional, got {other:?}"),
        }
    }

    #[test]
    fn info_roundtrips() {
        let out = bot_roundtrip(BotMsg::Info(sample_info()));
        match out {
            BotMsg::Info(info) => {
                assert_eq!(info.player, Player::Player2);
                assert_eq!(info.depth, 5);
                assert_eq!(info.nodes, 1234);
                assert_eq!(info.pv, vec![Direction::Up, Direction::Right]);
                assert_eq!(info.message, "best line");
                assert_eq!(info.state_hash, 0xCAFE_BABE);
                assert!((info.score.unwrap() - 0.75).abs() < f32::EPSILON);
            },
            other => panic!("expected Info, got {other:?}"),
        }
    }

    #[test]
    fn render_commands_roundtrips() {
        let out = bot_roundtrip(BotMsg::RenderCommands {
            player: Player::Player1,
            turn: 3,
            state_hash: 0x42,
        });
        match out {
            BotMsg::RenderCommands {
                player,
                turn,
                state_hash,
            } => {
                assert_eq!(player, Player::Player1);
                assert_eq!(turn, 3);
                assert_eq!(state_hash, 0x42);
            },
            other => panic!("expected RenderCommands, got {other:?}"),
        }
    }

    // ── Hash round-trip on BotMsg::Action specifically ───

    /// State hash on Action must round-trip without truncation across the
    /// wire's u64 default field — load-bearing for protocol sync.
    #[test]
    fn action_state_hash_preserves_full_u64() {
        let h: u64 = 0xFFFF_EEEE_DDDD_CCCC;
        let out = bot_roundtrip(BotMsg::Action {
            direction: Direction::Stay,
            player: Player::Player1,
            turn: 0,
            state_hash: h,
            think_ms: 0,
        });
        if let BotMsg::Action { state_hash, .. } = out {
            assert_eq!(state_hash, h);
        }
    }
}
