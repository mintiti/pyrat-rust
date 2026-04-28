//! PyO3 bridge: protocol messages as Python dicts.
//!
//! Python sees each `HostMsg`/`BotMsg` variant as a kind-tagged dict
//! (`{"kind": "Welcome", "player_slot": 1}`). Field names mirror the Rust
//! enum exactly; coordinates are `(int, int)` tuples; directions, player
//! slots, timing modes, option types, and game results are ints; optional
//! fields use Python `None`.
//!
//! The codec is the only place that touches FlatBuffers wire format — these
//! functions delegate to `pyrat_protocol::{extract_host_msg, extract_bot_msg,
//! serialize_host_msg, serialize_bot_msg}`.

use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;
use pyo3::types::{PyBytes, PyDict, PyList};

use pyrat::{Coordinates, Direction};
use pyrat_protocol::{
    extract_bot_msg, extract_host_msg, serialize_bot_msg, serialize_host_msg, BotMsg, GameOver,
    GameResult, HostMsg, Info, MatchConfig, MudEntry, OptionDef, OptionType, Player, SearchLimits,
    TimingMode, TurnState,
};
use pyrat_wire::{self as wire};

// ── PyO3 entry points ───────────────────────────────

/// Decode a length-stripped `HostPacket` frame to a Python dict.
#[pyfunction]
pub fn parse_host_frame<'py>(py: Python<'py>, buf: &[u8]) -> PyResult<Bound<'py, PyDict>> {
    let packet = flatbuffers::root::<wire::HostPacket>(buf)
        .map_err(|e| PyValueError::new_err(format!("verify error: {e}")))?;
    let msg = extract_host_msg(&packet)
        .map_err(|e| PyValueError::new_err(format!("codec error: {e}")))?;
    host_msg_to_pydict(py, &msg)
}

/// Decode a length-stripped `BotPacket` frame to a Python dict (test helper).
#[pyfunction]
pub fn parse_bot_frame<'py>(py: Python<'py>, buf: &[u8]) -> PyResult<Bound<'py, PyDict>> {
    let packet = flatbuffers::root::<wire::BotPacket>(buf)
        .map_err(|e| PyValueError::new_err(format!("verify error: {e}")))?;
    let msg =
        extract_bot_msg(&packet).map_err(|e| PyValueError::new_err(format!("codec error: {e}")))?;
    bot_msg_to_pydict(py, &msg)
}

/// Serialize a Python dict as a `BotPacket` frame.
#[pyfunction]
#[pyo3(name = "serialize_bot_msg")]
pub fn py_serialize_bot_msg<'py>(
    py: Python<'py>,
    dict: &Bound<'py, PyDict>,
) -> PyResult<Bound<'py, PyBytes>> {
    let msg = pydict_to_bot_msg(dict)?;
    Ok(PyBytes::new(py, &serialize_bot_msg(&msg)))
}

/// Serialize a Python dict as a `HostPacket` frame (test helper).
#[pyfunction]
#[pyo3(name = "serialize_host_msg")]
pub fn py_serialize_host_msg<'py>(
    py: Python<'py>,
    dict: &Bound<'py, PyDict>,
) -> PyResult<Bound<'py, PyBytes>> {
    let msg = pydict_to_host_msg(dict)?;
    Ok(PyBytes::new(py, &serialize_host_msg(&msg)))
}

// ── HostMsg ↔ dict ──────────────────────────────────

fn host_msg_to_pydict<'py>(py: Python<'py>, msg: &HostMsg) -> PyResult<Bound<'py, PyDict>> {
    let d = PyDict::new(py);
    match msg {
        HostMsg::Welcome { player_slot } => {
            d.set_item("kind", "Welcome")?;
            d.set_item("player_slot", player_to_int(*player_slot))?;
        },
        HostMsg::Configure {
            options,
            match_config,
        } => {
            d.set_item("kind", "Configure")?;
            let opt_list = PyList::empty(py);
            for (name, value) in options {
                opt_list.append((name.as_str(), value.as_str()))?;
            }
            d.set_item("options", opt_list)?;
            d.set_item("match_config", match_config_to_pydict(py, match_config)?)?;
        },
        HostMsg::GoPreprocess { state_hash } => {
            d.set_item("kind", "GoPreprocess")?;
            d.set_item("state_hash", *state_hash)?;
        },
        HostMsg::Advance {
            p1_dir,
            p2_dir,
            turn,
            new_hash,
        } => {
            d.set_item("kind", "Advance")?;
            d.set_item("p1_dir", dir_to_int(*p1_dir))?;
            d.set_item("p2_dir", dir_to_int(*p2_dir))?;
            d.set_item("turn", *turn)?;
            d.set_item("new_hash", *new_hash)?;
        },
        HostMsg::Go { state_hash, limits } => {
            d.set_item("kind", "Go")?;
            d.set_item("state_hash", *state_hash)?;
            d.set_item("limits", search_limits_to_pydict(py, limits)?)?;
        },
        HostMsg::GoState {
            turn_state,
            state_hash,
            limits,
        } => {
            d.set_item("kind", "GoState")?;
            d.set_item("turn_state", turn_state_to_pydict(py, turn_state)?)?;
            d.set_item("state_hash", *state_hash)?;
            d.set_item("limits", search_limits_to_pydict(py, limits)?)?;
        },
        HostMsg::Stop => {
            d.set_item("kind", "Stop")?;
        },
        HostMsg::FullState {
            match_config,
            turn_state,
        } => {
            d.set_item("kind", "FullState")?;
            d.set_item("match_config", match_config_to_pydict(py, match_config)?)?;
            d.set_item("turn_state", turn_state_to_pydict(py, turn_state)?)?;
        },
        HostMsg::ProtocolError { reason } => {
            d.set_item("kind", "ProtocolError")?;
            d.set_item("reason", reason.as_str())?;
        },
        HostMsg::GameOver {
            result,
            player1_score,
            player2_score,
        } => {
            d.set_item("kind", "GameOver")?;
            d.set_item("result", game_result_to_int(*result))?;
            d.set_item("player1_score", *player1_score)?;
            d.set_item("player2_score", *player2_score)?;
        },
    }
    Ok(d)
}

fn pydict_to_host_msg(dict: &Bound<'_, PyDict>) -> PyResult<HostMsg> {
    let kind = get_kind(dict)?;
    match kind.as_str() {
        "Welcome" => Ok(HostMsg::Welcome {
            player_slot: int_to_player(get_int(dict, "player_slot")?)?,
        }),
        "Configure" => {
            let options = get_options_pairs(dict, "options")?;
            let match_config = pydict_to_match_config(&get_dict(dict, "match_config")?)?;
            Ok(HostMsg::Configure {
                options,
                match_config: Box::new(match_config),
            })
        },
        "GoPreprocess" => Ok(HostMsg::GoPreprocess {
            state_hash: get_u64(dict, "state_hash")?,
        }),
        "Advance" => Ok(HostMsg::Advance {
            p1_dir: int_to_dir(get_int(dict, "p1_dir")?)?,
            p2_dir: int_to_dir(get_int(dict, "p2_dir")?)?,
            turn: get_u16(dict, "turn")?,
            new_hash: get_u64(dict, "new_hash")?,
        }),
        "Go" => Ok(HostMsg::Go {
            state_hash: get_u64(dict, "state_hash")?,
            limits: pydict_to_search_limits(&get_dict(dict, "limits")?)?,
        }),
        "GoState" => {
            let turn_state = pydict_to_turn_state(&get_dict(dict, "turn_state")?)?;
            Ok(HostMsg::GoState {
                turn_state: Box::new(turn_state),
                state_hash: get_u64(dict, "state_hash")?,
                limits: pydict_to_search_limits(&get_dict(dict, "limits")?)?,
            })
        },
        "Stop" => Ok(HostMsg::Stop),
        "FullState" => {
            let match_config = pydict_to_match_config(&get_dict(dict, "match_config")?)?;
            let turn_state = pydict_to_turn_state(&get_dict(dict, "turn_state")?)?;
            Ok(HostMsg::FullState {
                match_config: Box::new(match_config),
                turn_state: Box::new(turn_state),
            })
        },
        "ProtocolError" => Ok(HostMsg::ProtocolError {
            reason: get_str(dict, "reason")?,
        }),
        "GameOver" => Ok(HostMsg::GameOver {
            result: int_to_game_result(get_int(dict, "result")?)?,
            player1_score: get_f32(dict, "player1_score")?,
            player2_score: get_f32(dict, "player2_score")?,
        }),
        other => Err(PyValueError::new_err(format!(
            "unknown HostMsg kind {other:?}"
        ))),
    }
}

// ── BotMsg ↔ dict ───────────────────────────────────

fn bot_msg_to_pydict<'py>(py: Python<'py>, msg: &BotMsg) -> PyResult<Bound<'py, PyDict>> {
    let d = PyDict::new(py);
    match msg {
        BotMsg::Identify {
            name,
            author,
            agent_id,
            options,
        } => {
            d.set_item("kind", "Identify")?;
            d.set_item("name", name.as_str())?;
            d.set_item("author", author.as_str())?;
            d.set_item("agent_id", agent_id.as_str())?;
            let opt_list = PyList::empty(py);
            for opt in options {
                opt_list.append(option_def_to_pydict(py, opt)?)?;
            }
            d.set_item("options", opt_list)?;
        },
        BotMsg::Ready { state_hash } => {
            d.set_item("kind", "Ready")?;
            d.set_item("state_hash", *state_hash)?;
        },
        BotMsg::PreprocessingDone => {
            d.set_item("kind", "PreprocessingDone")?;
        },
        BotMsg::SyncOk { hash } => {
            d.set_item("kind", "SyncOk")?;
            d.set_item("hash", *hash)?;
        },
        BotMsg::Resync { my_hash } => {
            d.set_item("kind", "Resync")?;
            d.set_item("my_hash", *my_hash)?;
        },
        BotMsg::Action {
            direction,
            player,
            turn,
            state_hash,
            think_ms,
        } => {
            d.set_item("kind", "Action")?;
            d.set_item("direction", dir_to_int(*direction))?;
            d.set_item("player", player_to_int(*player))?;
            d.set_item("turn", *turn)?;
            d.set_item("state_hash", *state_hash)?;
            d.set_item("think_ms", *think_ms)?;
        },
        BotMsg::Provisional {
            direction,
            player,
            turn,
            state_hash,
        } => {
            d.set_item("kind", "Provisional")?;
            d.set_item("direction", dir_to_int(*direction))?;
            d.set_item("player", player_to_int(*player))?;
            d.set_item("turn", *turn)?;
            d.set_item("state_hash", *state_hash)?;
        },
        BotMsg::Info(info) => {
            d.set_item("kind", "Info")?;
            fill_info_fields(py, &d, info)?;
        },
        BotMsg::RenderCommands {
            player,
            turn,
            state_hash,
        } => {
            d.set_item("kind", "RenderCommands")?;
            d.set_item("player", player_to_int(*player))?;
            d.set_item("turn", *turn)?;
            d.set_item("state_hash", *state_hash)?;
        },
    }
    Ok(d)
}

fn pydict_to_bot_msg(dict: &Bound<'_, PyDict>) -> PyResult<BotMsg> {
    let kind = get_kind(dict)?;
    match kind.as_str() {
        "Identify" => {
            let opt_items = dict
                .get_item("options")?
                .ok_or_else(|| PyValueError::new_err("missing field: options"))?;
            let opt_list = opt_items.downcast::<PyList>()?;
            let mut options = Vec::with_capacity(opt_list.len());
            for item in opt_list.iter() {
                let item_dict = item.downcast::<PyDict>()?;
                options.push(pydict_to_option_def(item_dict)?);
            }
            Ok(BotMsg::Identify {
                name: get_str(dict, "name")?,
                author: get_str(dict, "author")?,
                agent_id: get_str(dict, "agent_id")?,
                options,
            })
        },
        "Ready" => Ok(BotMsg::Ready {
            state_hash: get_u64(dict, "state_hash")?,
        }),
        "PreprocessingDone" => Ok(BotMsg::PreprocessingDone),
        "SyncOk" => Ok(BotMsg::SyncOk {
            hash: get_u64(dict, "hash")?,
        }),
        "Resync" => Ok(BotMsg::Resync {
            my_hash: get_u64(dict, "my_hash")?,
        }),
        "Action" => Ok(BotMsg::Action {
            direction: int_to_dir(get_int(dict, "direction")?)?,
            player: int_to_player(get_int(dict, "player")?)?,
            turn: get_u16(dict, "turn")?,
            state_hash: get_u64(dict, "state_hash")?,
            think_ms: get_u32(dict, "think_ms")?,
        }),
        "Provisional" => Ok(BotMsg::Provisional {
            direction: int_to_dir(get_int(dict, "direction")?)?,
            player: int_to_player(get_int(dict, "player")?)?,
            turn: get_u16(dict, "turn")?,
            state_hash: get_u64(dict, "state_hash")?,
        }),
        "Info" => Ok(BotMsg::Info(pydict_to_info(dict)?)),
        "RenderCommands" => Ok(BotMsg::RenderCommands {
            player: int_to_player(get_int(dict, "player")?)?,
            turn: get_u16(dict, "turn")?,
            state_hash: get_u64(dict, "state_hash")?,
        }),
        other => Err(PyValueError::new_err(format!(
            "unknown BotMsg kind {other:?}"
        ))),
    }
}

// ── MatchConfig ↔ dict ──────────────────────────────

fn match_config_to_pydict<'py>(py: Python<'py>, cfg: &MatchConfig) -> PyResult<Bound<'py, PyDict>> {
    let d = PyDict::new(py);
    d.set_item("width", cfg.width)?;
    d.set_item("height", cfg.height)?;
    d.set_item("max_turns", cfg.max_turns)?;

    let walls = PyList::empty(py);
    for (a, b) in &cfg.walls {
        walls.append(((a.x, a.y), (b.x, b.y)))?;
    }
    d.set_item("walls", walls)?;

    let mud = PyList::empty(py);
    for entry in &cfg.mud {
        mud.append((
            (entry.pos1.x, entry.pos1.y),
            (entry.pos2.x, entry.pos2.y),
            entry.turns,
        ))?;
    }
    d.set_item("mud", mud)?;

    let cheese = PyList::empty(py);
    for c in &cfg.cheese {
        cheese.append((c.x, c.y))?;
    }
    d.set_item("cheese", cheese)?;

    d.set_item("player1_start", (cfg.player1_start.x, cfg.player1_start.y))?;
    d.set_item("player2_start", (cfg.player2_start.x, cfg.player2_start.y))?;

    let controlled = PyList::empty(py);
    for p in &cfg.controlled_players {
        controlled.append(player_to_int(*p))?;
    }
    d.set_item("controlled_players", controlled)?;

    d.set_item("timing", timing_to_int(cfg.timing))?;
    d.set_item("move_timeout_ms", cfg.move_timeout_ms)?;
    d.set_item("preprocessing_timeout_ms", cfg.preprocessing_timeout_ms)?;
    Ok(d)
}

fn pydict_to_match_config(dict: &Bound<'_, PyDict>) -> PyResult<MatchConfig> {
    let walls_any = dict
        .get_item("walls")?
        .ok_or_else(|| PyValueError::new_err("missing field: walls"))?;
    let walls_list = walls_any.downcast::<PyList>()?;
    let mut walls = Vec::with_capacity(walls_list.len());
    for item in walls_list.iter() {
        let ((x1, y1), (x2, y2)): ((u8, u8), (u8, u8)) = item.extract()?;
        walls.push((Coordinates::new(x1, y1), Coordinates::new(x2, y2)));
    }

    let mud_any = dict
        .get_item("mud")?
        .ok_or_else(|| PyValueError::new_err("missing field: mud"))?;
    let mud_list = mud_any.downcast::<PyList>()?;
    let mut mud = Vec::with_capacity(mud_list.len());
    for item in mud_list.iter() {
        let ((x1, y1), (x2, y2), turns): ((u8, u8), (u8, u8), u8) = item.extract()?;
        mud.push(MudEntry {
            pos1: Coordinates::new(x1, y1),
            pos2: Coordinates::new(x2, y2),
            turns,
        });
    }

    let cheese_any = dict
        .get_item("cheese")?
        .ok_or_else(|| PyValueError::new_err("missing field: cheese"))?;
    let cheese_list = cheese_any.downcast::<PyList>()?;
    let mut cheese = Vec::with_capacity(cheese_list.len());
    for item in cheese_list.iter() {
        let (x, y): (u8, u8) = item.extract()?;
        cheese.push(Coordinates::new(x, y));
    }

    let (p1x, p1y): (u8, u8) = dict
        .get_item("player1_start")?
        .ok_or_else(|| PyValueError::new_err("missing field: player1_start"))?
        .extract()?;
    let (p2x, p2y): (u8, u8) = dict
        .get_item("player2_start")?
        .ok_or_else(|| PyValueError::new_err("missing field: player2_start"))?
        .extract()?;

    let controlled_any = dict
        .get_item("controlled_players")?
        .ok_or_else(|| PyValueError::new_err("missing field: controlled_players"))?;
    let controlled_list = controlled_any.downcast::<PyList>()?;
    let mut controlled_players = Vec::with_capacity(controlled_list.len());
    for item in controlled_list.iter() {
        let raw: u8 = item.extract()?;
        controlled_players.push(int_to_player(raw)?);
    }

    Ok(MatchConfig {
        width: get_u8(dict, "width")?,
        height: get_u8(dict, "height")?,
        max_turns: get_u16(dict, "max_turns")?,
        walls,
        mud,
        cheese,
        player1_start: Coordinates::new(p1x, p1y),
        player2_start: Coordinates::new(p2x, p2y),
        controlled_players,
        timing: int_to_timing(get_int(dict, "timing")?)?,
        move_timeout_ms: get_u32(dict, "move_timeout_ms")?,
        preprocessing_timeout_ms: get_u32(dict, "preprocessing_timeout_ms")?,
    })
}

// ── TurnState ↔ dict ────────────────────────────────

/// Per-turn state without `state_hash` — the canonical hash lives at the
/// parent message level (Advance, Go, GoState, FullState carry their own
/// `state_hash`). Mirrors `pyrat_protocol::TurnState`, which is the field-
/// only struct the enum variants box.
fn turn_state_to_pydict<'py>(py: Python<'py>, ts: &TurnState) -> PyResult<Bound<'py, PyDict>> {
    let d = PyDict::new(py);
    d.set_item("turn", ts.turn)?;
    d.set_item(
        "player1_position",
        (ts.player1_position.x, ts.player1_position.y),
    )?;
    d.set_item(
        "player2_position",
        (ts.player2_position.x, ts.player2_position.y),
    )?;
    d.set_item("player1_score", ts.player1_score)?;
    d.set_item("player2_score", ts.player2_score)?;
    d.set_item("player1_mud_turns", ts.player1_mud_turns)?;
    d.set_item("player2_mud_turns", ts.player2_mud_turns)?;

    let cheese = PyList::empty(py);
    for c in &ts.cheese {
        cheese.append((c.x, c.y))?;
    }
    d.set_item("cheese", cheese)?;

    d.set_item("player1_last_move", dir_to_int(ts.player1_last_move))?;
    d.set_item("player2_last_move", dir_to_int(ts.player2_last_move))?;
    Ok(d)
}

fn pydict_to_turn_state(dict: &Bound<'_, PyDict>) -> PyResult<TurnState> {
    let cheese_any = dict
        .get_item("cheese")?
        .ok_or_else(|| PyValueError::new_err("missing field: cheese"))?;
    let cheese_list = cheese_any.downcast::<PyList>()?;
    let mut cheese = Vec::with_capacity(cheese_list.len());
    for item in cheese_list.iter() {
        let (x, y): (u8, u8) = item.extract()?;
        cheese.push(Coordinates::new(x, y));
    }

    let (p1x, p1y): (u8, u8) = dict
        .get_item("player1_position")?
        .ok_or_else(|| PyValueError::new_err("missing field: player1_position"))?
        .extract()?;
    let (p2x, p2y): (u8, u8) = dict
        .get_item("player2_position")?
        .ok_or_else(|| PyValueError::new_err("missing field: player2_position"))?
        .extract()?;

    Ok(TurnState {
        turn: get_u16(dict, "turn")?,
        player1_position: Coordinates::new(p1x, p1y),
        player2_position: Coordinates::new(p2x, p2y),
        player1_score: get_f32(dict, "player1_score")?,
        player2_score: get_f32(dict, "player2_score")?,
        player1_mud_turns: get_u8(dict, "player1_mud_turns")?,
        player2_mud_turns: get_u8(dict, "player2_mud_turns")?,
        cheese,
        player1_last_move: int_to_dir(get_int(dict, "player1_last_move")?)?,
        player2_last_move: int_to_dir(get_int(dict, "player2_last_move")?)?,
    })
}

// ── SearchLimits ↔ dict ─────────────────────────────

fn search_limits_to_pydict<'py>(
    py: Python<'py>,
    limits: &SearchLimits,
) -> PyResult<Bound<'py, PyDict>> {
    let d = PyDict::new(py);
    d.set_item("timeout_ms", limits.timeout_ms)?;
    d.set_item("depth", limits.depth)?;
    d.set_item("nodes", limits.nodes)?;
    Ok(d)
}

fn pydict_to_search_limits(dict: &Bound<'_, PyDict>) -> PyResult<SearchLimits> {
    Ok(SearchLimits {
        timeout_ms: get_optional_u32(dict, "timeout_ms")?,
        depth: get_optional_u16(dict, "depth")?,
        nodes: get_optional_u32(dict, "nodes")?,
    })
}

// ── Info ↔ dict ─────────────────────────────────────

fn fill_info_fields(py: Python<'_>, d: &Bound<'_, PyDict>, info: &Info) -> PyResult<()> {
    d.set_item("player", player_to_int(info.player))?;
    d.set_item("multipv", info.multipv)?;
    d.set_item("target", info.target.map(|c| (c.x, c.y)))?;
    d.set_item("depth", info.depth)?;
    d.set_item("nodes", info.nodes)?;
    d.set_item("score", info.score)?;
    let pv = PyList::empty(py);
    for dir in &info.pv {
        pv.append(dir_to_int(*dir))?;
    }
    d.set_item("pv", pv)?;
    d.set_item("message", info.message.as_str())?;
    d.set_item("turn", info.turn)?;
    d.set_item("state_hash", info.state_hash)?;
    Ok(())
}

fn pydict_to_info(dict: &Bound<'_, PyDict>) -> PyResult<Info> {
    let pv_any = dict
        .get_item("pv")?
        .ok_or_else(|| PyValueError::new_err("missing field: pv"))?;
    let pv_list = pv_any.downcast::<PyList>()?;
    let mut pv = Vec::with_capacity(pv_list.len());
    for item in pv_list.iter() {
        let raw: u8 = item.extract()?;
        pv.push(int_to_dir(raw)?);
    }

    let target = match dict.get_item("target")? {
        Some(t) if !t.is_none() => {
            let (x, y): (u8, u8) = t.extract()?;
            Some(Coordinates::new(x, y))
        },
        _ => None,
    };

    Ok(Info {
        player: int_to_player(get_int(dict, "player")?)?,
        multipv: get_u16(dict, "multipv")?,
        target,
        depth: get_u16(dict, "depth")?,
        nodes: get_u32(dict, "nodes")?,
        score: get_optional_f32(dict, "score")?,
        pv,
        message: get_str(dict, "message")?,
        turn: get_u16(dict, "turn")?,
        state_hash: get_u64(dict, "state_hash")?,
    })
}

// ── OptionDef ↔ dict ────────────────────────────────

fn option_def_to_pydict<'py>(py: Python<'py>, opt: &OptionDef) -> PyResult<Bound<'py, PyDict>> {
    let d = PyDict::new(py);
    d.set_item("name", opt.name.as_str())?;
    d.set_item("option_type", option_type_to_int(opt.option_type))?;
    d.set_item("default_value", opt.default_value.as_str())?;
    d.set_item("min", opt.min)?;
    d.set_item("max", opt.max)?;
    let choices = PyList::empty(py);
    for c in &opt.choices {
        choices.append(c.as_str())?;
    }
    d.set_item("choices", choices)?;
    Ok(d)
}

fn pydict_to_option_def(dict: &Bound<'_, PyDict>) -> PyResult<OptionDef> {
    let choices_any = dict
        .get_item("choices")?
        .ok_or_else(|| PyValueError::new_err("missing field: choices"))?;
    let choices_list = choices_any.downcast::<PyList>()?;
    let mut choices = Vec::with_capacity(choices_list.len());
    for item in choices_list.iter() {
        choices.push(item.extract::<String>()?);
    }
    Ok(OptionDef {
        name: get_str(dict, "name")?,
        option_type: int_to_option_type(get_int(dict, "option_type")?)?,
        default_value: get_str(dict, "default_value")?,
        min: get_i32(dict, "min")?,
        max: get_i32(dict, "max")?,
        choices,
    })
}

// ── Primitive helpers ───────────────────────────────

fn dir_to_int(d: Direction) -> u8 {
    d as u8
}

fn int_to_dir(raw: u8) -> PyResult<Direction> {
    Direction::try_from(raw).map_err(|_| {
        PyValueError::new_err(format!(
            "invalid direction {raw}, expected 0-4 (UP, RIGHT, DOWN, LEFT, STAY)"
        ))
    })
}

fn player_to_int(p: Player) -> u8 {
    p.0
}

fn int_to_player(raw: u8) -> PyResult<Player> {
    if raw <= 1 {
        Ok(Player(raw))
    } else {
        Err(PyValueError::new_err(format!(
            "invalid player {raw}, expected 0 (Player1) or 1 (Player2)"
        )))
    }
}

fn timing_to_int(t: TimingMode) -> u8 {
    t.0
}

fn int_to_timing(raw: u8) -> PyResult<TimingMode> {
    if raw <= 1 {
        Ok(TimingMode(raw))
    } else {
        Err(PyValueError::new_err(format!(
            "invalid timing mode {raw}, expected 0 (Wait) or 1 (Clock)"
        )))
    }
}

fn option_type_to_int(t: OptionType) -> u8 {
    t.0
}

fn int_to_option_type(raw: u8) -> PyResult<OptionType> {
    if raw <= 4 {
        Ok(OptionType(raw))
    } else {
        Err(PyValueError::new_err(format!(
            "invalid option type {raw}, expected 0-4"
        )))
    }
}

fn game_result_to_int(r: GameResult) -> u8 {
    r.0
}

fn int_to_game_result(raw: u8) -> PyResult<GameResult> {
    if raw <= 2 {
        Ok(GameResult(raw))
    } else {
        Err(PyValueError::new_err(format!(
            "invalid game result {raw}, expected 0-2"
        )))
    }
}

// ── Field extractors ────────────────────────────────

fn get_kind(dict: &Bound<'_, PyDict>) -> PyResult<String> {
    dict.get_item("kind")?
        .ok_or_else(|| PyValueError::new_err("missing field: kind"))?
        .extract()
}

fn get_int(dict: &Bound<'_, PyDict>, name: &str) -> PyResult<u8> {
    get_required(dict, name)?.extract()
}

fn get_u8(dict: &Bound<'_, PyDict>, name: &str) -> PyResult<u8> {
    get_required(dict, name)?.extract()
}

fn get_u16(dict: &Bound<'_, PyDict>, name: &str) -> PyResult<u16> {
    get_required(dict, name)?.extract()
}

fn get_u32(dict: &Bound<'_, PyDict>, name: &str) -> PyResult<u32> {
    get_required(dict, name)?.extract()
}

fn get_u64(dict: &Bound<'_, PyDict>, name: &str) -> PyResult<u64> {
    get_required(dict, name)?.extract()
}

fn get_i32(dict: &Bound<'_, PyDict>, name: &str) -> PyResult<i32> {
    get_required(dict, name)?.extract()
}

fn get_f32(dict: &Bound<'_, PyDict>, name: &str) -> PyResult<f32> {
    get_required(dict, name)?.extract()
}

fn get_str(dict: &Bound<'_, PyDict>, name: &str) -> PyResult<String> {
    get_required(dict, name)?.extract()
}

fn get_dict<'py>(dict: &Bound<'py, PyDict>, name: &str) -> PyResult<Bound<'py, PyDict>> {
    let value = get_required(dict, name)?;
    value.downcast_into::<PyDict>().map_err(Into::into)
}

fn get_options_pairs(dict: &Bound<'_, PyDict>, name: &str) -> PyResult<Vec<(String, String)>> {
    let value = get_required(dict, name)?;
    let list = value.downcast::<PyList>()?;
    let mut pairs = Vec::with_capacity(list.len());
    for item in list.iter() {
        let (k, v): (String, String) = item.extract()?;
        pairs.push((k, v));
    }
    Ok(pairs)
}

fn get_optional_u32(dict: &Bound<'_, PyDict>, name: &str) -> PyResult<Option<u32>> {
    match dict.get_item(name)? {
        Some(v) if !v.is_none() => Ok(Some(v.extract()?)),
        _ => Ok(None),
    }
}

fn get_optional_u16(dict: &Bound<'_, PyDict>, name: &str) -> PyResult<Option<u16>> {
    match dict.get_item(name)? {
        Some(v) if !v.is_none() => Ok(Some(v.extract()?)),
        _ => Ok(None),
    }
}

fn get_optional_f32(dict: &Bound<'_, PyDict>, name: &str) -> PyResult<Option<f32>> {
    match dict.get_item(name)? {
        Some(v) if !v.is_none() => Ok(Some(v.extract()?)),
        _ => Ok(None),
    }
}

fn get_required<'py>(dict: &Bound<'py, PyDict>, name: &str) -> PyResult<Bound<'py, pyo3::PyAny>> {
    dict.get_item(name)?
        .ok_or_else(|| PyValueError::new_err(format!("missing field: {name}")))
}

// `GameOver` is unused as a free-standing type at the BotMsg level but is
// part of the protocol vocabulary. Keep the import alive without dead_code
// noise.
#[allow(dead_code)]
fn _keep_imports(_g: GameOver) {}
